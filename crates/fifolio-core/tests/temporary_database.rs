//! Integration layer: the shared database helper against a real SQLite file, in process
//! [TST-003].
//!
//! The database is a real file in a temporary directory, created by the shared helper so that
//! nothing here depends on a fixed path and the suite runs in parallel.
//!
//! This is a demonstration of the harness, not coverage of `fifolio-core`: nothing below calls
//! into the crate, which holds no schema yet. Migrations, the invariants storage enforces and
//! batch deletion (TST-004) arrive with FIF-011, which is where this file starts naming ids.

use fifolio_test_support::TempDb;
use sqlx::sqlite::SqlitePool;
use sqlx::{Row, query};

#[tokio::test]
async fn the_temp_db_helper_opens_a_real_sqlite_file() {
    let db = TempDb::new();
    assert!(!db.path().exists(), "the file is created by connecting");

    let pool = SqlitePool::connect(&db.url())
        .await
        .expect("connect to the temporary database");
    query("create table probe (value text not null)")
        .execute(&pool)
        .await
        .expect("create a table");
    query("insert into probe (value) values (?)")
        .bind("fifolio")
        .execute(&pool)
        .await
        .expect("insert a row");

    let row = query("select value from probe")
        .fetch_one(&pool)
        .await
        .expect("read the row back");
    assert_eq!(row.get::<String, _>("value"), "fifolio");
    assert!(db.path().exists());

    pool.close().await;
}

#[tokio::test]
async fn each_temp_db_is_a_separate_sqlite_file() {
    let first = TempDb::new();
    let second = TempDb::new();

    let pool = SqlitePool::connect(&first.url()).await.expect("connect");
    query("create table probe (value text not null)")
        .execute(&pool)
        .await
        .expect("create a table");
    pool.close().await;

    // Isolation is what lets the suite run in parallel: the second database has never seen
    // the first one's schema.
    let other = SqlitePool::connect(&second.url()).await.expect("connect");
    let missing = query("select value from probe").fetch_one(&other).await;
    assert!(missing.is_err(), "a fresh database is empty");
    other.close().await;
}
