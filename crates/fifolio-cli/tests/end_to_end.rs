//! End-to-end layer: the real binaries, a real database file and a real port [TST-006].
//!
//! No library call and no in-process router: everything here goes through a spawned process,
//! which is the only layer that exercises argument parsing and the two processes agreeing.
//!
//! The CLI is still a stub, so what is asserted of it is that the harness finds and runs it and
//! that it fails loudly; FIF-042 replaces it, and TST-007's scenario (import a fixture, resolve
//! the queue, attribute, report) lands with it. The server is real: its arguments, the address
//! it binds and the spec it serves are asserted against the spawned binary.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::Command;
use fifolio_test_support::{TempDb, free_port};
use predicates::str::contains;

/// Long enough for a debug build to start and migrate a fresh database on a loaded CI runner.
const STARTUP: Duration = Duration::from_secs(30);

/// A running `fifolio-server`, killed when dropped so a failing assertion leaks no process.
struct Server {
    child: Child,
    port: u16,
}

impl Server {
    /// Spawns the server with `args` in `dir` and waits until it accepts connections.
    fn start(dir: &Path, port: u16, args: &[&str]) -> Self {
        let child = std::process::Command::new(assert_cmd::cargo::cargo_bin("fifolio-server"))
            .current_dir(dir)
            .args(["--port", &port.to_string()])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn fifolio-server");
        let mut server = Self { child, port };
        let deadline = Instant::now() + STARTUP;
        while TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err() {
            if let Some(status) = server.child.try_wait().expect("poll the server") {
                panic!("fifolio-server exited before listening: {status}");
            }
            assert!(
                Instant::now() < deadline,
                "fifolio-server did not start listening"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        server
    }

    async fn get_json(&self, path: &str) -> serde_json::Value {
        let response = reqwest::get(format!("http://127.0.0.1:{}{path}", self.port))
            .await
            .expect("the server answers");
        assert!(
            response.status().is_success(),
            "{path}: {}",
            response.status()
        );
        response.json().await.expect("the body is JSON")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn openapi_printed() -> serde_json::Value {
    let output = Command::cargo_bin("fifolio-server")
        .expect("the fifolio-server binary is built by `cargo test --workspace`")
        .arg("openapi")
        .output()
        .expect("run fifolio-server openapi");
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("`openapi` prints JSON")
}

/// The server listens on the port it is given, over the database it is given, which it has
/// created and opened itself, and serves the spec `fifolio-server openapi` prints [SRV-001,
/// SRV-003, SRV-004, SRV-005, SRV-006, ARC-003].
#[tokio::test]
async fn the_server_serves_its_spec_on_the_given_port_over_the_given_database() {
    let db = TempDb::new();
    let database = db.path().display().to_string();
    assert!(!db.path().exists());

    let server = Server::start(Path::new("."), free_port(), &["--database", &database]);

    assert!(
        db.path().exists(),
        "the server creates the database it opens"
    );
    assert_eq!(server.get_json("/openapi.json").await, openapi_printed());
}

/// Without `--database` the file is `./fifolio.db`, relative to the working directory the
/// server was started in [SRV-004].
#[test]
fn the_default_database_is_fifolio_db_in_the_working_directory() {
    let dir = tempfile::tempdir().expect("a working directory");

    let _server = Server::start(dir.path(), free_port(), &[]);

    assert!(dir.path().join("fifolio.db").exists());
}

/// The server binds 127.0.0.1 and nothing else: the rest of the loopback range and IPv6
/// loopback refuse the connection a wildcard bind would accept [ARC-022].
#[test]
fn the_server_binds_127_0_0_1_only() {
    let db = TempDb::new();
    let port = free_port();

    let _server = Server::start(
        Path::new("."),
        port,
        &["--database", &db.path().display().to_string()],
    );

    let elsewhere = [
        SocketAddr::from((Ipv4Addr::new(127, 0, 0, 2), port)),
        SocketAddr::from((Ipv6Addr::LOCALHOST, port)),
    ];
    for address in elsewhere {
        assert!(
            TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_err(),
            "{address} accepted a connection"
        );
    }
}

/// `openapi` prints the spec and exits without opening the database, so it neither creates
/// the file nor needs the port, which another process may hold [SRV-005].
#[test]
fn openapi_prints_the_spec_without_opening_the_database_or_binding() {
    let db = TempDb::new();
    let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("hold a port");
    let port = held.local_addr().expect("the held address").port();

    let output = Command::cargo_bin("fifolio-server")
        .expect("the fifolio-server binary is built by `cargo test --workspace`")
        .args(["--database", &db.path().display().to_string()])
        .args(["--port", &port.to_string()])
        .arg("openapi")
        .timeout(STARTUP)
        .output()
        .expect("run fifolio-server openapi");

    assert!(output.status.success(), "{output:?}");
    let spec: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(spec["info"]["title"], "fifolio-server");
    assert!(spec["paths"]["/openapi.json"].is_object());
    assert!(!db.path().exists(), "`openapi` opened the database");
}

/// A port already taken is reported and the process fails rather than serving elsewhere
/// [SRV-003].
#[test]
fn a_taken_port_is_refused_by_name() {
    let db = TempDb::new();
    let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("hold a port");
    let port = held.local_addr().expect("the held address").port();

    Command::cargo_bin("fifolio-server")
        .expect("the fifolio-server binary is built by `cargo test --workspace`")
        .args(["--database", &db.path().display().to_string()])
        .args(["--port", &port.to_string()])
        .timeout(STARTUP)
        .assert()
        .failure()
        .stderr(contains(format!("cannot listen on 127.0.0.1:{port}")));
}

/// A database that cannot be opened stops the server before it listens [SRV-004, ARC-003].
#[test]
fn an_unopenable_database_is_refused_before_listening() {
    let dir = tempfile::tempdir().expect("a directory");
    let not_a_directory = dir.path().join("file");
    std::fs::write(&not_a_directory, b"").expect("write a plain file");
    let database = not_a_directory.join("fifolio.db");

    Command::cargo_bin("fifolio-server")
        .expect("the fifolio-server binary is built by `cargo test --workspace`")
        .args(["--database", &database.display().to_string()])
        .args(["--port", &free_port().to_string()])
        .timeout(STARTUP)
        .assert()
        .failure()
        .stderr(contains("cannot open the database"));
}

#[test]
fn the_cli_binary_is_built_and_fails_loudly() {
    Command::new(env!("CARGO_BIN_EXE_fifolio-cli"))
        .assert()
        .failure()
        .stderr(contains("not implemented yet"));
}
