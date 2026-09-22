//! End-to-end layer: the real binaries, a real database file and a real port [TST-006].
//!
//! No library call and no in-process router: everything here goes through a spawned process,
//! which is the only layer that exercises argument parsing and the two processes agreeing.
//!
//! Both binaries are stubs today, so what is asserted is that the harness finds and runs them
//! and that they fail loudly. FIF-032 and FIF-042 replace the stubs, and TST-007's scenario
//! (import a fixture, resolve the queue, attribute, report) lands with them.

use assert_cmd::Command;
use fifolio_test_support::{TempDb, free_port};
use predicates::str::contains;

#[test]
fn the_server_binary_is_built_and_fails_loudly() {
    let db = TempDb::new();
    let port = free_port();

    // The stub exits before reading `argv`, so these arguments are the shape FIF-032 will
    // parse, not something this test can yet assert on. The database and the port become real
    // there, and this test's name stops being the whole of what it checks.
    Command::cargo_bin("fifolio-server")
        .expect("the fifolio-server binary is built by `cargo test --workspace`")
        .args(["--database", &db.path().display().to_string()])
        .args(["--port", &port.to_string()])
        .assert()
        .failure()
        .stderr(contains("not implemented yet"));
}

#[test]
fn the_cli_binary_is_built_and_fails_loudly() {
    Command::new(env!("CARGO_BIN_EXE_fifolio-cli"))
        .assert()
        .failure()
        .stderr(contains("not implemented yet"));
}
