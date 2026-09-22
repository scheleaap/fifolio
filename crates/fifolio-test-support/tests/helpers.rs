//! Integration layer: the shared helpers against the real filesystem and the real network
//! stack, in process [TST-003].
//!
//! These live here rather than in a `#[cfg(test)] mod tests` because they do I/O, which the
//! unit layer forbids [TST-001]. The unit-layer demonstration stays in `src/lib.rs`.

use std::net::TcpListener;

use fifolio_test_support::{TempDb, free_port};

/// Two temporary databases never collide and the directory goes away with the value, so
/// integration tests can run in parallel without cleaning up after each other [TST-003].
#[test]
fn temp_dbs_are_distinct_and_removed_on_drop() {
    let first = TempDb::new();
    let second = TempDb::new();
    assert_ne!(first.path(), second.path());
    assert!(first.url().starts_with("sqlite://"));

    let parent = first
        .path()
        .parent()
        .expect("a parent directory")
        .to_owned();
    assert!(parent.exists());
    drop(first);
    assert!(!parent.exists());
}

/// A free port is one a test can actually bind, and two calls hand out different ports, which
/// is what keeps the end-to-end layer parallel-safe [TST-006].
#[test]
fn free_ports_are_bindable_and_distinct() {
    let first = free_port();
    let second = free_port();
    assert_ne!(first, second);

    // Binding both at once is the property an end-to-end test needs: two servers, one suite.
    let held = TcpListener::bind(("127.0.0.1", first)).expect("the first reported port is free");
    TcpListener::bind(("127.0.0.1", second)).expect("the second reported port is free");
    drop(held);
}
