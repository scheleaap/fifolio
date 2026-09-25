//! Integration layer: `fifolio-core`'s resolved dependency graph, read from `Cargo.lock`
//! [TST-003].
//!
//! Domain rules, the FIFO engine, rounding, importers and report shaping are tested without a
//! server or a terminal [ARC-024]. That is a property of the manifests, so it is checked here
//! rather than asserted in a doc comment: adding `axum` or `ratatui` to `fifolio-core` must
//! fail a test, not merely read oddly.
//!
//! `Cargo.lock` is used rather than `cargo metadata` because it needs no subprocess, no
//! network and no platform triple. Its per-package dependency lists are the union over
//! normal, build and dev dependencies and over every platform, so this check is conservative:
//! it can report a crate the host build would not link, never miss one it would.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use cargo_lock::{Lockfile, Package};

/// The crates that would mean a server or a terminal had entered the graph. Every one of them
/// is used elsewhere in the workspace, so each is one edge away from appearing here by
/// accident: `axum`, `hyper` and `tower-http` in `fifolio-server`, `ratatui`, `crossterm` and
/// `reqwest` in `fifolio-cli`.
const SERVER_AND_TERMINAL_CRATES: [&str; 6] = [
    "axum",
    "hyper",
    "tower-http",
    "ratatui",
    "crossterm",
    "reqwest",
];

/// Domain logic is tested without a server or a terminal in the dependency graph [ARC-024].
#[test]
fn core_depends_on_no_server_and_no_terminal_crate() {
    let reachable = reachable_from("fifolio-core");
    let found: Vec<&str> = SERVER_AND_TERMINAL_CRATES
        .into_iter()
        .filter(|crate_name| reachable.contains(*crate_name))
        .collect();

    assert!(
        found.is_empty(),
        "fifolio-core reaches {found:?}; ARC-024 keeps servers and terminals out of core"
    );
}

/// The walk finds what is there: `fifolio-server` does reach `axum`, so an empty result above
/// is a fact about core and not a walk that silently found nothing.
#[test]
fn the_walk_reports_a_crate_that_is_really_in_a_graph() {
    assert!(reachable_from("fifolio-server").contains("axum"));
}

/// The server is the only process that opens the database [ARC-003]: the CLI cannot, because
/// neither the storage layer nor a SQLite driver is in its graph.
#[test]
fn the_cli_cannot_open_the_database() {
    let reachable = reachable_from("fifolio-cli");
    assert!(reachable.contains("fifolio-cli"), "the walk found the CLI");
    let found: Vec<&str> = ["fifolio-core", "sqlx", "libsqlite3-sys"]
        .into_iter()
        .filter(|crate_name| reachable.contains(*crate_name))
        .collect();
    assert!(
        found.is_empty(),
        "fifolio-cli reaches {found:?}; ARC-003 leaves the database to the server"
    );
}

/// Every package name reachable from `root`, `root` included.
fn reachable_from(root: &str) -> BTreeSet<String> {
    let lockfile = Lockfile::load(workspace_root().join("Cargo.lock"))
        .expect("the workspace lock file is readable");
    let packages: BTreeMap<String, &Package> = lockfile
        .packages
        .iter()
        .map(|package| (package.name.to_string(), package))
        .collect();

    // A depth-first walk with an explicit stack; the graph is acyclic by construction, but
    // `seen` is what terminates it, a diamond otherwise being re-walked once per path.
    let mut seen = BTreeSet::new();
    let mut pending = vec![root.to_owned()];
    while let Some(name) = pending.pop() {
        if let Some(package) = packages.get(&name)
            && seen.insert(name)
        {
            pending.extend(
                package
                    .dependencies
                    .iter()
                    .map(|dependency| dependency.name.to_string()),
            );
        }
    }
    seen
}

fn workspace_root() -> &'static Path {
    // `crates/fifolio-core` up two levels; `CARGO_MANIFEST_DIR` is absolute.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the manifest directory is two levels below the workspace root")
}
