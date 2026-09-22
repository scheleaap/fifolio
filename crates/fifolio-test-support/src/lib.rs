//! Helpers shared by the test suites of every crate, and the place the test conventions are
//! written down.
//!
//! # The three layers
//!
//! | Layer | Scope | Where it lives | Demonstrated by |
//! | --- | --- | --- | --- |
//! | unit | one module, no I/O, no database, no network [TST-001] | a `#[cfg(test)] mod tests` inside the module under test | [`sqlite_url`]'s tests below |
//! | integration | a crate against its real dependencies, in process [TST-003] | `crates/<crate>/tests/` | `tests/helpers.rs` here, `fifolio-core/tests/temporary_database.rs`, `fifolio-server/tests/http_surface.rs` |
//! | end to end | the real binaries, a real database, a real port [TST-006] | `crates/fifolio-cli/tests/` | `fifolio-cli/tests/end_to_end.rs` |
//!
//! Integration tests get their database from [`TempDb`] and end-to-end tests their port from
//! [`free_port`], so no test depends on a fixed path or a fixed port and the suite stays
//! runnable in parallel.
//!
//! `fifolio-core` is exercised at the unit and integration layers without a server or a
//! terminal in the dependency graph [ARC-024]; only the end-to-end layer knows about
//! processes. What enforces that is `fifolio-core/tests/dependency_graph.rs`, not this
//! paragraph.
//!
//! The rules TST-002 enumerates — the FIFO proposal, the drift rule, the quotation factor, FX
//! selection, canonical ordering, idempotency and importer row classification — are unit
//! tested by the item that implements each of them, the first being FIF-004. None of them is
//! covered here: this crate owns the convention and the harness, not the coverage of any rule.
//!
//! # Naming the requirements a test covers
//!
//! Every test names the requirement ids it covers, so that specification coverage is
//! checkable rather than impressionistic, per `.claude/agents/README.md`. The form is the one
//! `design/` uses for a citation: the bracketed id in the test's own doc comment, one line,
//! ids separated by commas. An id is named only where something asserts it: a test that is
//! still a harness demonstration names no id it does not check, or a grep for the id reports
//! coverage that does not exist.
//!
//! ```ignore
//! /// A closing may only be attributed if every earlier closing is attributed [DOM-nnn].
//! #[test]
//! fn earlier_closing_unattributed_is_refused() { /* ... */ }
//! ```
//!
//! A module-level doc comment may carry the ids its whole file covers; an individual test then
//! names only what is specific to it. Ids are never renumbered, so a test naming a retired id
//! stays interpretable.

use std::net::TcpListener;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// A SQLite database in a directory that is removed when the value is dropped.
///
/// The file is not created here: the connection string asks SQLite to create it, which is the
/// path the application itself takes on first run.
pub struct TempDb {
    dir: TempDir,
}

impl TempDb {
    #[must_use]
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("a temporary directory for the test database"),
        }
    }

    /// The database file's path, for a binary that takes `--database`.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.dir.path().join("fifolio.db")
    }

    /// The `sqlx` connection string for [`TempDb::path`].
    #[must_use]
    pub fn url(&self) -> String {
        sqlite_url(&self.path())
    }
}

impl Default for TempDb {
    fn default() -> Self {
        Self::new()
    }
}

/// `mode=rwc` creates the file on first connection; without it `sqlx` refuses to open a
/// database that does not exist yet.
fn sqlite_url(path: &Path) -> String {
    format!("sqlite://{}?mode=rwc", path.display())
}

/// A TCP port that is free on the loopback interface at the moment of the call.
///
/// The kernel picks it by binding port 0, and the listener is closed before returning, so
/// there is a window in which something else could take it. That window is unavoidable: an
/// end-to-end test spawns a separate process and cannot hand it a bound socket. The kernel
/// does not reuse an ephemeral port immediately, which makes the window small enough to
/// ignore.
#[must_use]
pub fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("a free loopback port")
        .local_addr()
        .expect("the bound address")
        .port()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit layer: one module, no I/O [TST-001].
    #[test]
    fn sqlite_url_names_the_file_and_asks_for_creation() {
        assert_eq!(
            sqlite_url(Path::new("/tmp/fifolio-xyz/fifolio.db")),
            "sqlite:///tmp/fifolio-xyz/fifolio.db?mode=rwc"
        );
    }

    /// An awkward path is passed through unencoded, `sqlx` reading everything before the query
    /// string as a filename rather than as a URL path, so a space needs no escaping [TST-001].
    #[test]
    fn sqlite_url_passes_a_path_with_a_space_through_unencoded() {
        assert_eq!(
            sqlite_url(Path::new("/tmp/a dir/fifolio.db")),
            "sqlite:///tmp/a dir/fifolio.db?mode=rwc"
        );
    }
}
