//! Integration layer [TST-003]: importing a file into an account and storing what it yields,
//! against a real temporary database.
//!
//! The Trade Republic fixtures supply the files a user posts; the refusals and the oversold sell
//! use files built for them. Nothing here asserts an amount: the fixtures' amounts are perturbed
//! [TST-014].

use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use fifolio_core::entities::{
    Account, Isin, Quotation, RecordIdentity, Security, SecurityType, SourceFormat,
};
use fifolio_core::identity::{IdentitySource, identify};
use fifolio_core::import::trade_republic::{HEADERS, TradeRepublic};
use fifolio_core::import::{Ground, ImportError};
use fifolio_core::import_service::{ImportFileError, Imported, import_file};
use fifolio_core::storage::{Database, StorageError};
use fifolio_test_support::TempDb;
use sqlx::sqlite::SqlitePool;

/// The 2022 export: two buys, of a fund and of a stock, and five cash rows.
const FIXTURE_2022: &str = "transactions_2022-01-01_2022-12-31.csv";

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/trade-republic")
        .join(name);
    fs::read(&path).unwrap_or_else(|_| panic!("read the fixture {}", path.display()))
}

fn account() -> Account {
    Account::new("Trade Republic", "DE0001")
}

fn now() -> DateTime<Utc> {
    "2026-01-02T09:00:00Z".parse().expect("a timestamp")
}

/// A Trade Republic export of the given rows, each naming only the columns it populates.
fn export(rows: &[&[(&str, &str)]]) -> Vec<u8> {
    let quoted = |values: Vec<&str>| format!("\"{}\"\n", values.join("\",\""));
    let body: String = rows
        .iter()
        .map(|row| {
            quoted(
                HEADERS
                    .iter()
                    .map(|header| {
                        row.iter()
                            .find(|(name, _)| name == header)
                            .map_or("", |(_, value)| *value)
                    })
                    .collect(),
            )
        })
        .collect();
    format!("{}{body}", quoted(HEADERS.to_vec())).into_bytes()
}

/// A database holding the target account, and a pool beside it to count rows with.
struct Fixture {
    _db: TempDb,
    database: Database,
    pool: SqlitePool,
}

impl Fixture {
    async fn new() -> Self {
        let db = TempDb::new();
        let database = Database::open(db.path()).await.expect("open the database");
        database
            .accounts()
            .insert(&account())
            .await
            .expect("the account");
        let pool = SqlitePool::connect(&db.url()).await.expect("connect");
        Self {
            _db: db,
            database,
            pool,
        }
    }

    async fn import(&self, content: &[u8]) -> Result<Imported, ImportFileError> {
        self.import_as("transactions.csv", content).await
    }

    async fn import_as(&self, filename: &str, content: &[u8]) -> Result<Imported, ImportFileError> {
        import_file(
            &self.database,
            &TradeRepublic,
            &account(),
            filename,
            content,
            now(),
        )
        .await
    }

    /// How many rows `table` holds; the names are this file's own constants, never input.
    async fn count(&self, table: &'static str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!("select count(*) from {table}")))
            .fetch_one(&self.pool)
            .await
            .expect("count")
    }

    /// Every stored record's identity, owning batch and first batch, in identity order.
    async fn records(&self) -> Vec<(String, i64, i64)> {
        sqlx::query_as(
            "select identity, batch_id, first_batch_id from source_record order by identity",
        )
        .fetch_all(&self.pool)
        .await
        .expect("the records")
    }
}

/// A Trade Republic buy of the stock `isin`, stated as of `datetime`.
fn buy<'a>(transaction_id: &'a str, isin: &'a str, datetime: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        ("datetime", datetime),
        ("date", &datetime[..10]),
        ("category", "TRADING"),
        ("type", "BUY"),
        ("asset_class", "STOCK"),
        ("name", "Bought"),
        ("symbol", isin),
        ("shares", "35.0000000000"),
        ("price", "75.090000"),
        ("amount", "-2628.150000"),
        ("fee", "-1.00"),
        ("currency", "EUR"),
        ("transaction_id", transaction_id),
    ]
}

fn identity(transaction_id: &str) -> RecordIdentity {
    identify(&account(), &IdentitySource::BrokerReference(transaction_id))
}

/// A file's buys are stored as source records owned by the batch the import created, which
/// records the file's name, format and counts; its cash rows are counted and leave no record
/// [SRV-012], [SRV-016], [SRV-019], [DOM-017], [DOM-046].
#[tokio::test]
async fn an_import_stores_its_batch_and_the_records_of_its_stored_rows() {
    let f = Fixture::new().await;

    let imported = f
        .import(&fixture(FIXTURE_2022))
        .await
        .expect("the file imports");

    let batch = f
        .database
        .import_batches()
        .find(imported.batch())
        .await
        .expect("the lookup")
        .expect("the batch is stored");
    assert_eq!(batch.account(), &account());
    assert_eq!(batch.filename(), "transactions.csv");
    assert_eq!(batch.format(), SourceFormat::TradeRepublicDeCsv);
    assert_eq!(batch.imported_at(), now());
    assert_eq!(
        (
            batch.counts().derived,
            batch.counts().pending,
            batch.counts().non_position
        ),
        (2, 0, 5)
    );

    assert_eq!(f.count("source_record").await, 2);
    for stored in imported.import().stored() {
        assert_eq!(
            f.database
                .source_records()
                .find(stored.record().identity())
                .await
                .expect("the lookup"),
            Some(stored.record().clone())
        );
    }
    let owners: Vec<i64> = f
        .records()
        .await
        .iter()
        .map(|(_, owner, _)| *owner)
        .collect();
    assert_eq!(owners, [imported.batch().get(); 2]);
}

/// Every ISIN the stored rows name that is not stored yet is created, flagged auto-created and
/// needing review, with the type and quotation its `asset_class` maps to [SRV-014], [DOM-006],
/// [DOM-126], [IMP-TR-020].
#[tokio::test]
async fn an_import_creates_the_unknown_securities_flagged_for_review() {
    let f = Fixture::new().await;

    let imported = f
        .import(&fixture(FIXTURE_2022))
        .await
        .expect("the file imports");

    assert_eq!(
        imported.created(),
        [Isin::new("XF0000000152"), Isin::new("XF0000000079")]
    );
    let securities = f.database.securities().list().await.expect("the list");
    let stored: Vec<(&str, SecurityType, Quotation, bool, bool)> = securities
        .iter()
        .map(|security| {
            (
                security.isin().as_str(),
                security.security_type(),
                security.quotation(),
                security.is_auto_created(),
                security.needs_review(),
            )
        })
        .collect();
    assert_eq!(
        stored,
        [
            (
                "XF0000000079",
                SecurityType::Fund,
                Quotation::PerUnit,
                true,
                true
            ),
            (
                "XF0000000152",
                SecurityType::Stock,
                Quotation::PerUnit,
                true,
                true
            ),
        ]
    );
}

/// A security already stored is left as it is — its name, type, quotation and flags — and is
/// not reported as created [SRV-014], [SRV-011], [DOM-006].
#[tokio::test]
async fn an_import_leaves_a_stored_security_alone() {
    let f = Fixture::new().await;
    let entered = Security::new(
        Isin::new("XF0000000152"),
        "Entered by hand",
        SecurityType::Etf,
        Quotation::PerUnit,
    );
    f.database
        .securities()
        .insert(&entered)
        .await
        .expect("the security");

    let imported = f
        .import(&fixture(FIXTURE_2022))
        .await
        .expect("the file imports");

    assert_eq!(imported.created(), [Isin::new("XF0000000079")]);
    assert_eq!(
        f.database
            .securities()
            .find(&Isin::new("XF0000000152"))
            .await
            .expect("the lookup"),
        Some(entered)
    );
}

/// Posting a file again changes no stored record or security: no record is added, each keeps
/// its owner and first supplier, and a security the user corrected and reviewed in between stays
/// corrected and reviewed. The second import still creates a batch of its own, which owns
/// nothing (DEC-111, provisional) [SRV-015], [DOM-022], [SRV-019].
#[tokio::test]
async fn posting_a_file_again_changes_no_record_or_security() {
    let f = Fixture::new().await;
    let first = f
        .import(&fixture(FIXTURE_2022))
        .await
        .expect("the first import");
    let isin = Isin::new("XF0000000079");
    f.database
        .securities()
        .update(&isin, "Corrected", SecurityType::Etf, Quotation::PerUnit)
        .await
        .expect("the correction");
    f.database
        .securities()
        .mark_reviewed(&isin)
        .await
        .expect("the review");
    let records = f.records().await;
    let securities = f.database.securities().list().await.expect("the list");

    let second = f
        .import(&fixture(FIXTURE_2022))
        .await
        .expect("the second import");

    assert_eq!(f.records().await, records);
    assert!(
        records
            .iter()
            .all(|(_, owner, first_batch)| *owner == first.batch().get()
                && *first_batch == first.batch().get())
    );
    assert_eq!(
        f.database.securities().list().await.expect("the list"),
        securities
    );
    assert!(second.created().is_empty());
    assert_eq!(second.import(), first.import());
    assert_ne!(second.batch(), first.batch());
    assert_eq!(f.count("import_batch").await, 2);
}

/// A later export of the same account overlaps the first: under another name, it stores only
/// the row the first lacked, owned by its own batch, and creates only that row's security. The
/// rows already stored keep their owner and first supplier, so re-posting is idempotent per
/// record identity, not per file [SRV-015], [DOM-022], [SRV-014].
#[tokio::test]
async fn a_later_overlapping_export_stores_only_its_new_rows() {
    let f = Fixture::new().await;
    let a = buy("a", "XF0000000152", "2024-05-02T06:01:14.891Z");
    let b = buy("b", "XF0000000079", "2024-05-03T06:01:14.891Z");
    let c = buy("c", "XF0000000999", "2024-05-04T06:01:14.891Z");
    let first = f
        .import_as("first.csv", &export(&[&a, &b]))
        .await
        .expect("the first import");

    let second = f
        .import_as("later.csv", &export(&[&a, &b, &c]))
        .await
        .expect("the later import");

    let records = f.records().await;
    let owner_of = |transaction_id: &str| {
        let expected = identity(transaction_id);
        records
            .iter()
            .find(|(record, _, _)| record == expected.as_str())
            .map(|(_, owner, first_batch)| (*owner, *first_batch))
            .unwrap_or_else(|| panic!("{transaction_id} is stored"))
    };
    assert_eq!(records.len(), 3);
    assert_eq!(owner_of("a"), (first.batch().get(), first.batch().get()));
    assert_eq!(owner_of("b"), (first.batch().get(), first.batch().get()));
    assert_eq!(owner_of("c"), (second.batch().get(), second.batch().get()));
    assert_eq!(second.created(), [Isin::new("XF0000000999")]);
}

/// Idempotence is scoped to the account [DOM-024]: the same file posted into a second account
/// stores every record again, owned by that account's batch, beside the first account's. The
/// securities, keyed by ISIN alone [DOM-071], already exist and are not created again
/// [SRV-015], [SRV-014].
#[tokio::test]
async fn the_same_file_into_another_account_stores_its_records_again() {
    let f = Fixture::new().await;
    let other = Account::new("Trade Republic", "DE0002");
    f.database
        .accounts()
        .insert(&other)
        .await
        .expect("the other account");
    let file = export(&[
        &buy("a", "XF0000000152", "2024-05-02T06:01:14.891Z"),
        &buy("b", "XF0000000079", "2024-05-03T06:01:14.891Z"),
    ]);
    let first = f.import(&file).await.expect("the first import");

    let second = import_file(
        &f.database,
        &TradeRepublic,
        &other,
        "transactions.csv",
        &file,
        now(),
    )
    .await
    .expect("the import into the other account");

    let records = f.records().await;
    let owner_of = |account: &Account, transaction_id: &str| {
        let expected = identify(account, &IdentitySource::BrokerReference(transaction_id));
        records
            .iter()
            .find(|(record, _, _)| record == expected.as_str())
            .map(|(_, owner, first_batch)| (*owner, *first_batch))
            .unwrap_or_else(|| panic!("{transaction_id} is stored for {account:?}"))
    };
    assert_eq!(records.len(), 4);
    for transaction_id in ["a", "b"] {
        assert_eq!(
            owner_of(&account(), transaction_id),
            (first.batch().get(), first.batch().get())
        );
        assert_eq!(
            owner_of(&other, transaction_id),
            (second.batch().get(), second.batch().get())
        );
    }
    assert_eq!(second.import().counts().derived, 2);
    assert!(second.created().is_empty(), "{:?}", second.created());
    assert_eq!(f.count("security").await, 2);
}

/// A file with a failed row stores nothing at all — no batch, no record, no security — and the
/// refusal names every failed row [SRV-058].
#[tokio::test]
async fn a_refused_file_stores_nothing() {
    let f = Fixture::new().await;
    // BOND maps to no security type (DEC-077), so each bond buy is a failed row.
    let bond = |transaction_id, isin| {
        let mut row = buy(transaction_id, isin, "2024-05-02T06:01:14.891Z");
        row.retain(|(name, _)| *name != "asset_class");
        row.push(("asset_class", "BOND"));
        row
    };

    let refusal = f
        .import(&export(&[
            &bond("x", "XF0000000301"),
            &buy("a", "XF0000000152", "2024-05-03T06:01:14.891Z"),
            &bond("y", "XF0000000302"),
        ]))
        .await
        .expect_err("the bond buys are refused");

    let ImportFileError::Import(ImportError::Refused { grounds }) = &refusal else {
        panic!("a refusal, not {refusal}");
    };
    let [Ground::FailedRows { failures }] = grounds.as_slice() else {
        panic!("only failed rows, not {grounds:?}");
    };
    assert_eq!(failures.len(), 2);
    for table in ["import_batch", "source_record", "security"] {
        assert_eq!(f.count(table).await, 0, "{table}");
    }
}

/// An account that is not stored is refused before the file is read, and nothing is stored
/// [SRV-012].
#[tokio::test]
async fn an_unknown_account_is_refused() {
    let f = Fixture::new().await;

    let refusal = import_file(
        &f.database,
        &TradeRepublic,
        &Account::new("Trade Republic", "nobody"),
        "transactions.csv",
        b"not even a Trade Republic file",
        now(),
    )
    .await
    .expect_err("the account is not stored");

    assert!(
        matches!(
            &refusal,
            ImportFileError::Storage(StorageError::UnknownAccount { broker, id })
                if broker == "Trade Republic" && id == "nobody"
        ),
        "{refusal:?}"
    );
    for table in ["import_batch", "source_record", "security"] {
        assert_eq!(f.count(table).await, 0, "{table}");
    }
}

/// A sell of more than the account has ever held imports like any other row: holdings are not
/// checked on import, and the shortfall surfaces at attribution [SRV-018].
#[tokio::test]
async fn a_sell_exceeding_the_holdings_imports() {
    let f = Fixture::new().await;
    let sell: &[(&str, &str)] = &[
        ("datetime", "2024-05-02T06:01:14.891Z"),
        ("date", "2024-05-02"),
        ("category", "TRADING"),
        ("type", "SELL"),
        ("asset_class", "STOCK"),
        ("name", "Never bought"),
        ("symbol", "XF0000000999"),
        ("shares", "-35.0000000000"),
        ("price", "75.090000"),
        ("amount", "2628.150000"),
        ("fee", "-1.00"),
        ("currency", "EUR"),
        ("transaction_id", "sell-of-nothing"),
    ];

    let imported = f.import(&export(&[sell])).await.expect("the sell imports");

    assert_eq!(imported.import().counts().derived, 1);
    assert!(
        f.database
            .source_records()
            .find(&identity("sell-of-nothing"))
            .await
            .expect("the lookup")
            .is_some()
    );
    assert_eq!(imported.created(), [Isin::new("XF0000000999")]);
}
