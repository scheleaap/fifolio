//! Integration layer: `fifolio-core`'s storage against a real temporary SQLite database, in
//! process [TST-003], [TST-004].
//!
//! Every test opens its own file through the shared helper, so nothing here depends on a fixed
//! path and the suite runs in parallel. No test reaches the network.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use chrono::{DateTime, NaiveDate, Utc};
use fifolio_core::decimal::{FxRate, Money, Quantity, QuotedPrice};
use fifolio_core::entities::{
    Account, ImportBatch, ImportCounts, Isin, Order, Quotation, RecordIdentity, Security,
    SecurityType, SourceFormat, SourceRecord,
};
use fifolio_core::identity::{IdentitySource, identify};
use fifolio_core::manual_entry::{Election, ManualEntry, Ratio, Supplied};
use fifolio_core::ordering::{BatchAge, Leg, OrderKey, RecordPosition};
use fifolio_core::storage::{
    BatchId, DEFAULT_DATABASE_PATH, Database, ManualEntryId, Placement, RecordHandle, StorageError,
    TransactionId,
};
use fifolio_core::transaction::{
    Buy, BuyOrigin, DateProvenance, Derivation, Expiration, Sell, Split, Transaction, TransferIn,
    TransferInSource, TransferOut,
};
use fifolio_core::valuation::{Conversion, Currency, RateSource, Valued};
use fifolio_test_support::TempDb;
use rust_decimal_macros::dec;
use sqlx::sqlite::SqlitePool;
use sqlx::{Row, query};
use vec1::vec1;

fn account() -> Account {
    Account::new("Saxo", "69900/1000000")
}

fn isin() -> Isin {
    Isin::new("NL0000009538")
}

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 5, 2).expect("a valid date")
}

fn cite(reference: &str) -> RecordIdentity {
    identify(&account(), &IdentitySource::BrokerReference(reference))
}

/// The derivation of the worked example's two rows, which are stored in `batch` first: a
/// transaction is derived only from records storage holds [DOM-047].
async fn derivation(database: &Database, batch: BatchId) -> Derivation {
    Derivation::new(
        date(),
        vec1![
            store_record(database, batch, "4100200300", &[]).await,
            store_record(database, batch, "4100200301", &[]).await,
        ],
    )
}

/// A reverse split, whose ratio has no finite decimal expansion: storing it as anything but the
/// integer pair would not read back as itself [DOM-113].
fn one_for_three() -> Ratio {
    Ratio::new(
        NonZeroU32::MIN,
        NonZeroU32::new(3).expect("a non-zero denominator"),
    )
}

/// The Saxo worked example's conversion: USD booked, the rate stated per EUR [DOM-086].
fn conversion() -> Conversion {
    Conversion::new(
        Currency::new("USD"),
        FxRate::new(dec!(1.074500)),
        RateSource::Ecb,
        date(),
    )
}

fn money(native: rust_decimal::Decimal, eur: rust_decimal::Decimal) -> Valued<Money> {
    Valued::new(Money::new(native), Money::new(eur))
}

fn price(native: rust_decimal::Decimal, eur: rust_decimal::Decimal) -> Valued<QuotedPrice> {
    Valued::new(QuotedPrice::new(native), QuotedPrice::new(eur))
}

async fn open() -> (TempDb, Database) {
    let db = TempDb::new();
    let database = Database::open(db.path())
        .await
        .expect("open the temporary database");
    (db, database)
}

/// The account, the security and the import a transaction or a source record refers to.
///
/// A placement carries foreign keys to all three [DOM-066], [DOM-072], [DOM-119], so the rows
/// must be there before anything is placed against them; the security is written first, which is
/// the order the initial schema left to this item.
async fn place(database: &Database) -> (Placement, BatchId) {
    database
        .accounts()
        .insert(&account())
        .await
        .expect("the account");
    database
        .securities()
        .insert(&Security::auto_created(
            isin(),
            "NN Group",
            SecurityType::Stock,
            Quotation::PerUnit,
        ))
        .await
        .expect("the security");
    let batch = database
        .import_batches()
        .insert(&batch(SourceFormat::SaxoNlXlsx))
        .await
        .expect("the batch");
    (Placement::derived(account(), isin(), batch), batch)
}

/// The tables a migration from empty must create, by name.
async fn table_names(path: &std::path::Path) -> Vec<String> {
    let pool = SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .expect("connect to the migrated database");
    let names = query("select name from sqlite_master where type = 'table' order by name")
        .fetch_all(&pool)
        .await
        .expect("read the schema")
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();
    pool.close().await;
    names
}

/// Migration from empty: the file is created on first open and the schema is applied
/// [ARC-011], [ARC-012], [ARC-014], [TST-004].
#[tokio::test]
async fn opening_an_absent_file_creates_it_and_migrates_from_empty() {
    let db = TempDb::new();
    assert!(!db.path().exists(), "nothing exists before the first open");

    let database = Database::open(db.path()).await.expect("first run");
    database.close().await;

    assert!(db.path().exists(), "the file is created on first run");

    let tables = table_names(&db.path()).await;
    for expected in [
        "account",
        "security",
        "source_record",
        "import_batch",
        "transaction_record",
        "transaction_citation",
        "transaction_buy",
        "transaction_transfer_in",
        "transaction_sell",
        "transaction_expiration",
        "transaction_transfer_out",
        "manual_entry",
        "manual_entry_answer",
        "fx_rate",
        "transaction_placement",
        "attribution",
        "attribution_allocation",
        "emitted_transfer_in",
        "_sqlx_migrations",
    ] {
        assert!(
            tables.iter().any(|name| name == expected),
            "migrating from empty must create {expected}, found {tables:?}"
        );
    }
}

/// Migrations are versioned, so opening a database that is already current applies nothing and
/// leaves the data alone [ARC-012].
#[tokio::test]
async fn reopening_a_migrated_database_applies_nothing_further() {
    let db = TempDb::new();

    let first = Database::open(db.path()).await.expect("first run");
    first.accounts().insert(&account()).await.expect("insert");
    first.close().await;

    let second = Database::open(db.path()).await.expect("second run");
    let survivor = second
        .accounts()
        .find("Saxo", "69900/1000000")
        .await
        .expect("read back");
    second.close().await;

    assert_eq!(survivor, Some(account()), "a second open is not a reset");
}

/// The database file is `./fifolio.db` unless a caller names another, and naming another is all
/// it takes [ARC-013].
#[tokio::test]
async fn the_default_path_is_overridable() {
    assert_eq!(DEFAULT_DATABASE_PATH, "./fifolio.db");

    let db = TempDb::new();
    let elsewhere = db.path().with_file_name("somewhere-else.db");
    let database = Database::open(&elsewhere).await.expect("open elsewhere");
    database.close().await;

    assert!(elsewhere.exists());
}

/// A security's ISIN is unique, and the second insert says so rather than failing opaquely
/// [DOM-071], [TST-004].
#[tokio::test]
async fn a_second_security_with_the_same_isin_is_refused() {
    let (_db, database) = open().await;

    let first = Security::new(isin(), "Philips", SecurityType::Stock, Quotation::PerUnit);
    // A different name, type and provenance: it is the ISIN alone that collides.
    let second = Security::auto_created(
        isin(),
        "Koninklijke Philips",
        SecurityType::Etf,
        Quotation::PerUnit,
    );

    database.securities().insert(&first).await.expect("insert");
    let refused = database.securities().insert(&second).await;

    match refused {
        Err(StorageError::DuplicateIsin { isin: reported }) => {
            assert_eq!(reported, "NL0000009538");
        }
        other => panic!("a duplicate ISIN must be refused, got {other:?}"),
    }

    assert_eq!(
        database
            .securities()
            .find(&isin())
            .await
            .expect("read back"),
        Some(first),
        "the refused insert leaves the stored security untouched"
    );

    // The allowed side of the same constraint: it is the ISIN that must be unique and nothing
    // else, so a second security differing only in its ISIN is stored.
    let third = Security::new(
        Isin::new("NL0000009165"),
        "Koninklijke Philips",
        SecurityType::Stock,
        Quotation::PerUnit,
    );
    database.securities().insert(&third).await.expect("insert");
    assert_eq!(
        database
            .securities()
            .find(third.isin())
            .await
            .expect("read back"),
        Some(third)
    );
}

/// Accounts and securities round-trip, every security type included: the stored codes are
/// written out by hand, and two variants sharing one code would read back as each other
/// [DOM-004], [DOM-005], [DOM-006], [TST-004].
#[tokio::test]
async fn accounts_and_securities_round_trip() {
    let (_db, database) = open().await;

    database
        .accounts()
        .insert(&account())
        .await
        .expect("insert");

    for (index, security_type) in [
        SecurityType::Stock,
        SecurityType::Bond,
        SecurityType::Etf,
        SecurityType::Fund,
        SecurityType::Derivative,
        SecurityType::Other,
    ]
    .into_iter()
    .enumerate()
    {
        let security = Security::auto_created(
            Isin::new(format!("NL000000916{index}")),
            "NL 7.5% 2023",
            security_type,
            Quotation::PercentOfPar,
        );
        database
            .securities()
            .insert(&security)
            .await
            .expect("insert");

        assert_eq!(
            database
                .securities()
                .find(security.isin())
                .await
                .expect("read back"),
            Some(security),
            "every security type must read back as itself"
        );
    }

    assert_eq!(
        database
            .accounts()
            .find("Saxo", "69900/1000000")
            .await
            .expect("read back"),
        Some(account())
    );
    assert_eq!(
        database
            .accounts()
            .find("Saxo", "absent")
            .await
            .expect("read back"),
        None
    );
}

/// A source record round-trips with its raw line and its parsed fields [DOM-007], [TST-004].
#[tokio::test]
async fn a_source_record_round_trips_with_its_parsed_fields() {
    let (_db, database) = open().await;
    let record = SourceRecord::new(
        cite("4100200300"),
        Order::new(12),
        "\"2024-05-02\",\"BUY\",\"IE000Y77LGG9\",\"55\"",
        BTreeMap::from([
            ("Acties".to_owned(), "Koop".to_owned()),
            ("Positie-ID".to_owned(), "1234567".to_owned()),
        ]),
    );

    let (_placement, batch) = place(&database).await;
    database
        .source_records()
        .insert(batch, &record)
        .await
        .expect("insert");

    assert_eq!(
        database
            .source_records()
            .find(record.identity())
            .await
            .expect("read back"),
        Some(record)
    );
}

fn imported_at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_714_608_000, 0).expect("a valid timestamp")
}

fn batch(format: SourceFormat) -> ImportBatch {
    ImportBatch::new(
        account(),
        "Transactions_2024.xlsx",
        format,
        imported_at(),
        ImportCounts {
            derived: 7,
            pending: 2,
            non_position: 48,
        },
    )
}

/// An import batch round-trips with its account, its file and its four counts, for either
/// source format [DOM-017], [DOM-043], [TST-004].
#[tokio::test]
async fn an_import_batch_round_trips_with_its_counts() {
    let (_db, database) = open().await;
    database
        .accounts()
        .insert(&account())
        .await
        .expect("insert");

    for format in [SourceFormat::SaxoNlXlsx, SourceFormat::TradeRepublicDeCsv] {
        let batch = batch(format);
        let id = database
            .import_batches()
            .insert(&batch)
            .await
            .expect("insert");

        assert_eq!(
            database.import_batches().find(id).await.expect("read back"),
            Some(batch),
            "every source format must read back as itself"
        );
    }
}

/// Batches list with their keys, in the order they were imported, each reading back as stored
/// [SRV-020], [DOM-017], [TST-004].
#[tokio::test]
async fn import_batches_list_in_import_order_with_their_keys() {
    let (_db, database) = open().await;
    database
        .accounts()
        .insert(&account())
        .await
        .expect("insert");
    assert_eq!(database.import_batches().list().await.expect("list"), []);

    let saxo = batch(SourceFormat::SaxoNlXlsx);
    let trade_republic = batch(SourceFormat::TradeRepublicDeCsv);
    let first = database
        .import_batches()
        .insert(&saxo)
        .await
        .expect("insert");
    let second = database
        .import_batches()
        .insert(&trade_republic)
        .await
        .expect("insert");

    assert_eq!(
        database.import_batches().list().await.expect("list"),
        [(first, saxo), (second, trade_republic)]
    );
}

/// Foreign keys are enforced rather than merely declared: SQLite leaves them off per
/// connection, so a batch naming an account that was never stored must be refused [ARC-012],
/// [TST-004].
#[tokio::test]
async fn a_batch_for_an_account_that_was_never_stored_is_refused() {
    let (_db, database) = open().await;

    let refused = database
        .import_batches()
        .insert(&batch(SourceFormat::SaxoNlXlsx))
        .await;

    assert!(
        matches!(refused, Err(StorageError::Database(_))),
        "a batch without its account is refused, got {refused:?}"
    );
}

/// Each of the six transaction variants round-trips with its own fields, its native/EUR pairs
/// and its conversion, a split its integer ratio [DOM-010], [DOM-028], [DOM-029], [DOM-085],
/// [DOM-113], [TST-004].
#[tokio::test]
async fn every_transaction_variant_round_trips() {
    let (_db, database) = open().await;
    let (placement, batch) = place(&database).await;
    let derivation = derivation(&database, batch).await;

    let variants: Vec<Transaction> = vec![
        Buy::new(
            derivation.clone(),
            Quantity::new(dec!(55)),
            price(dec!(4.18), dec!(3.89)),
            money(dec!(230.00), dec!(214.05)),
            money(dec!(8.00), dec!(7.45)),
            BuyOrigin::Purchase,
            conversion(),
        )
        .into(),
        Buy::new(
            derivation.clone(),
            Quantity::new(dec!(3)),
            price(dec!(26.10), dec!(26.10)),
            money(dec!(78.30), dec!(78.30)),
            money(dec!(0.00), dec!(0.00)),
            BuyOrigin::StockDividend,
            Conversion::native(date()),
        )
        .into(),
        TransferIn::new(
            derivation.clone(),
            Quantity::new(dec!(12.50000000)),
            money(dec!(1000.00), dec!(930.67)),
            money(dec!(0.00), dec!(0.00)),
            NaiveDate::from_ymd_opt(2019, 3, 14).expect("a valid date"),
            DateProvenance::Inherited,
            TransferInSource::CorporateAction,
            conversion(),
        )
        .into(),
        // The other half of the three enums the transfer-in carries: a rename that collapsed
        // two codes into one would make this variant read back as the one above.
        TransferIn::new(
            derivation.clone(),
            Quantity::new(dec!(12.50000000)),
            money(dec!(1000.00), dec!(930.67)),
            money(dec!(0.00), dec!(0.00)),
            NaiveDate::from_ymd_opt(2019, 3, 14).expect("a valid date"),
            DateProvenance::TransferDate,
            TransferInSource::Broker,
            Conversion::new(
                Currency::new("USD"),
                FxRate::new(dec!(1.074500)),
                RateSource::Broker,
                date(),
            ),
        )
        .into(),
        Sell::new(
            derivation.clone(),
            Quantity::new(dec!(55)),
            price(dec!(5.00), dec!(4.65)),
            money(dec!(275.00), dec!(255.93)),
            money(dec!(8.00), dec!(7.45)),
            conversion(),
        )
        .into(),
        Expiration::new(
            derivation.clone(),
            money(dec!(0.00), dec!(0.00)),
            money(dec!(0.00), dec!(0.00)),
            conversion(),
        )
        .into(),
        TransferOut::new(
            derivation.clone(),
            Quantity::new(dec!(55)),
            money(dec!(15.00), dec!(13.96)),
            one_for_three(),
            conversion(),
            isin(),
        )
        .into(),
        Split::new(
            Derivation::new(
                date(),
                vec1![store_record(&database, batch, "4100200302", &[]).await],
            ),
            one_for_three(),
        )
        .into(),
    ];

    for transaction in variants {
        let id = database
            .transactions()
            .insert(&placement, &transaction)
            .await
            .expect("insert");

        assert_eq!(
            database.transactions().find(id).await.expect("read back"),
            Some(transaction.clone()),
            "a variant must read back as itself"
        );
    }
}

/// Nothing is created from nothing: the handle a transaction is derived from is issued by storing
/// the record, and the stored transaction cites the record the handle was issued for [DOM-047],
/// [TST-004].
///
/// That a derivation cannot be written from no handle, from a bare identity or from a handle
/// made by hand is asserted by the `compile_fail` examples on `Derivation::new`; this is the
/// other half, that the one way in leads from a stored record.
#[tokio::test]
async fn a_transaction_is_derived_only_from_a_record_storage_holds() {
    let (_db, database) = open().await;
    let (placement, batch) = place(&database).await;
    let record = SourceRecord::new(cite("4100200300"), Order::new(1), "raw", BTreeMap::new());

    let handle = database
        .source_records()
        .insert(batch, &record)
        .await
        .expect("the record");
    assert_eq!(handle.identity(), record.identity());

    let id = database
        .transactions()
        .insert(
            &placement,
            &Split::new(Derivation::new(date(), vec1![handle]), one_for_three()).into(),
        )
        .await
        .expect("insert");
    let stored = database
        .transactions()
        .find(id)
        .await
        .expect("read back")
        .expect("a stored transaction");

    assert_eq!(stored.cites(), [record.identity().clone()]);
}

/// A handle kept past an import undo of its batch names a record that is no longer stored, and a
/// transaction derived from it is refused: a citation may outlive its record [DOM-099], but a
/// derivation is made only from a record storage holds when it is stored [DOM-047].
#[tokio::test]
async fn a_transaction_derived_from_a_record_an_undo_removed_is_refused() {
    let (_db, database) = open().await;
    let (_, batch) = place(&database).await;
    let record = SourceRecord::new(cite("4100200300"), Order::new(1), "raw", BTreeMap::new());
    let handle = database
        .source_records()
        .insert(batch, &record)
        .await
        .expect("the record");
    database
        .import_batches()
        .delete(batch)
        .await
        .expect("nothing cites the record yet, so the undo goes through");
    // The batch is gone, so the transaction is placed against none.
    let placement = Placement::emitted(account(), isin());

    let refused = database
        .transactions()
        .insert(
            &placement,
            &Split::new(Derivation::new(date(), vec1![handle]), one_for_three()).into(),
        )
        .await;

    match refused {
        Err(StorageError::UnknownRecord { identity }) => {
            assert_eq!(identity, record.identity().as_str());
        }
        other => panic!("a derivation from a removed record must be refused, got {other:?}"),
    }
}

/// No handle, and so no position, is issued for a record that was never stored or that an undo
/// removed: a transaction reconnected to it cannot take a stale place in the order [DOM-011],
/// [DOM-047].
#[tokio::test]
async fn a_record_not_stored_or_undone_yields_no_handle() {
    let (_db, database) = open().await;
    let (_, batch) = place(&database).await;
    assert_eq!(
        database
            .source_records()
            .handle(&cite("never-stored"))
            .await
            .expect("read the handle"),
        None,
        "a record never stored has no handle"
    );

    store_record(&database, batch, "undone", &[]).await;
    database
        .import_batches()
        .delete(batch)
        .await
        .expect("nothing cites the record, so the undo goes through");

    assert_eq!(
        database
            .source_records()
            .handle(&cite("undone"))
            .await
            .expect("read the handle"),
        None,
        "a record its batch's undo removed has no handle"
    );
}

/// A handle issued by one database names a record the other never stored, and is refused there
/// on the same rule [DOM-047].
#[tokio::test]
async fn a_handle_issued_by_another_database_is_refused() {
    let (_issuer_db, issuer) = open().await;
    let (_, issuer_batch) = place(&issuer).await;
    let handle = store_record(&issuer, issuer_batch, "4100200300", &[]).await;
    let (_db, database) = open().await;
    let (placement, _) = place(&database).await;

    let refused = database
        .transactions()
        .insert(
            &placement,
            &Split::new(Derivation::new(date(), vec1![handle]), one_for_three()).into(),
        )
        .await;

    assert!(
        matches!(refused, Err(StorageError::UnknownRecord { .. })),
        "a record this database does not hold must be refused, got {refused:?}"
    );
}

/// A derivation is refused whole when any of its records is not stored, whichever citation it
/// is: nothing of the transaction is left behind, neither its header, its detail row nor the
/// citations written before the unknown one [DOM-047], [TST-004].
#[tokio::test]
async fn a_derivation_with_one_unknown_record_among_several_leaves_nothing_stored() {
    let (db, database) = open().await;
    let (placement, batch) = place(&database).await;
    let stored = store_record(&database, batch, "4100200300", &[]).await;
    let (_issuer_db, issuer) = open().await;
    let (_, issuer_batch) = place(&issuer).await;
    let foreign = store_record(&issuer, issuer_batch, "4100200399", &[]).await;
    let unknown = foreign.identity().clone();

    for handles in [
        vec1![stored.clone(), foreign.clone()],
        vec1![foreign.clone(), stored.clone()],
    ] {
        // A buy, so that a detail row is written before the citations are.
        let buy: Transaction = Buy::new(
            Derivation::new(date(), handles),
            Quantity::new(dec!(55)),
            price(dec!(4.18), dec!(3.89)),
            money(dec!(230.00), dec!(214.05)),
            money(dec!(8.00), dec!(7.45)),
            BuyOrigin::Purchase,
            conversion(),
        )
        .into();

        match database.transactions().insert(&placement, &buy).await {
            Err(StorageError::UnknownRecord { identity }) => {
                assert_eq!(identity, unknown.as_str());
            }
            other => panic!("a derivation from an unknown record must be refused, got {other:?}"),
        }
    }

    let pool = SqlitePool::connect(&format!("sqlite://{}", db.path().display()))
        .await
        .expect("connect");
    for (table, count) in [
        (
            "transaction_record",
            "select count(*) as n from transaction_record",
        ),
        (
            "transaction_citation",
            "select count(*) as n from transaction_citation",
        ),
        (
            "transaction_buy",
            "select count(*) as n from transaction_buy",
        ),
    ] {
        let rows: i64 = query(count).fetch_one(&pool).await.expect("count").get("n");
        assert_eq!(rows, 0, "a refused insert leaves no row in {table}");
    }
    pool.close().await;
}

/// A record that was not written yields no handle, so nothing can be derived from it [DOM-047].
#[tokio::test]
async fn a_record_that_was_not_written_yields_no_handle() {
    let (_db, database) = open().await;
    let record = SourceRecord::new(cite("4100200300"), Order::new(1), "raw", BTreeMap::new());

    let refused = database
        .source_records()
        .insert(BatchId::new(999), &record)
        .await;

    assert!(
        refused.is_err(),
        "a record in a batch that does not exist must not be stored, got {refused:?}"
    );
}

/// A transaction's citations keep the caller's order, which is the shape a multi-row corporate
/// action had [DOM-016], [TST-004].
#[tokio::test]
async fn citations_keep_their_order() {
    let (_db, database) = open().await;
    let (placement, batch) = place(&database).await;
    let mut cites = Vec::new();
    for reference in ["c", "a", "b"] {
        cites.push(store_record(&database, batch, reference, &[]).await);
    }
    let transaction: Transaction = Split::new(
        Derivation::new(date(), cites.try_into().expect("three records")),
        one_for_three(),
    )
    .into();

    let id = database
        .transactions()
        .insert(&placement, &transaction)
        .await
        .expect("insert");
    let stored = database
        .transactions()
        .find(id)
        .await
        .expect("read back")
        .expect("a stored transaction");

    assert_eq!(stored.cites(), [cite("c"), cite("a"), cite("b")]);
}

/// A transaction that was never stored is absent rather than an error [TST-004].
#[tokio::test]
async fn an_absent_transaction_reads_back_as_none() {
    let (_db, database) = open().await;
    let (placement, batch) = place(&database).await;
    let id = database
        .transactions()
        .insert(
            &placement,
            &Split::new(derivation(&database, batch).await, one_for_three()).into(),
        )
        .await
        .expect("insert");
    let absent = TransactionId::new(id.get() + 1);

    assert_eq!(
        database.transactions().find(absent).await.expect("read"),
        None
    );
}

/// Each of the five supplied shapes round-trips, with the records it answers in the order the
/// queue showed them [DOM-097], [DOM-098], [DOM-099], [TST-004].
#[tokio::test]
async fn every_manual_entry_shape_round_trips() {
    let (_db, database) = open().await;
    database
        .accounts()
        .insert(&account())
        .await
        .expect("insert");

    let ratio = Ratio::new(
        NonZeroU32::new(3).expect("a non-zero numerator"),
        NonZeroU32::new(1).expect("a non-zero denominator"),
    );
    let shapes = [
        Supplied::Election(Election::Stock {
            shares: Quantity::new(dec!(3.00000000)),
        }),
        Supplied::Election(Election::Cash),
        Supplied::Split(ratio),
        Supplied::Exchange {
            target: Isin::new("US0378331005"),
            ratio,
        },
        Supplied::Disposal {
            quantity: Quantity::new(dec!(100)),
            target: Some(Isin::new("CA8934631091")),
        },
        Supplied::Disposal {
            quantity: Quantity::new(dec!(100)),
            target: None,
        },
    ];

    for supplied in shapes {
        let entry = ManualEntry::new(
            account(),
            isin(),
            supplied,
            [cite("4100200300"), cite("4100200301")],
        );

        let id = database
            .manual_entries()
            .insert(&entry)
            .await
            .expect("insert");

        assert_eq!(
            database.manual_entries().find(id).await.expect("read back"),
            Some(entry)
        );
    }
}

/// Persistence is a boundary at which a value must already be at its scale [ARC-009],
/// [ARC-010]: a figure carrying more decimals than its kind is refused, not truncated
/// [TST-004].
#[tokio::test]
async fn a_value_not_at_its_scale_is_refused_rather_than_truncated() {
    let (db, database) = open().await;
    let (placement, batch) = place(&database).await;

    // 214.054 is four cents and a half of a cent; storing it would round a figure the caller
    // never rounded, and the stored gross is what every calculation reads [DOM-085].
    let unscaled: Transaction = Buy::new(
        derivation(&database, batch).await,
        Quantity::new(dec!(55)),
        price(dec!(4.18), dec!(3.89)),
        money(dec!(230.00), dec!(214.054)),
        money(dec!(8.00), dec!(7.45)),
        BuyOrigin::Purchase,
        conversion(),
    )
    .into();

    match database.transactions().insert(&placement, &unscaled).await {
        Err(StorageError::UnscaledValue {
            field,
            value,
            scale,
        }) => {
            assert_eq!(field, "gross");
            assert_eq!(value, "214.054");
            assert_eq!(scale, 2);
        }
        other => panic!("an unscaled figure must be refused, got {other:?}"),
    }

    let rows: i64 = {
        let pool = SqlitePool::connect(&format!("sqlite://{}", db.path().display()))
            .await
            .expect("connect");
        let count = query("select count(*) as n from transaction_record")
            .fetch_one(&pool)
            .await
            .expect("count")
            .get("n");
        pool.close().await;
        count
    };
    assert_eq!(rows, 0, "a refused insert leaves no header row behind");
}

/// The same boundary on a quantity, whose scale is 8 rather than 2 [ARC-007], [ARC-009].
#[tokio::test]
async fn a_quantity_beyond_eight_decimals_is_refused() {
    let (_db, database) = open().await;

    let entry = ManualEntry::new(
        account(),
        isin(),
        Supplied::Election(Election::Stock {
            shares: Quantity::new(dec!(3.000000005)),
        }),
        [cite("4100200300")],
    );
    database
        .accounts()
        .insert(&account())
        .await
        .expect("insert");

    match database.manual_entries().insert(&entry).await {
        Err(StorageError::UnscaledValue { field, scale, .. }) => {
            assert_eq!(field, "shares");
            assert_eq!(scale, 8);
        }
        other => panic!("an unscaled quantity must be refused, got {other:?}"),
    }
}

/// The same boundary on a unit price, whose scale is 6: dividing a booked value by a quantity
/// is where a seventh decimal appears, and a price is what a cost basis is rebuilt from
/// [ARC-007], [ARC-009], [ARC-010].
#[tokio::test]
async fn a_unit_price_beyond_six_decimals_is_refused() {
    let (_db, database) = open().await;
    let (placement, batch) = place(&database).await;

    let transaction: Transaction = Sell::new(
        derivation(&database, batch).await,
        Quantity::new(dec!(55)),
        price(dec!(4.1812345), dec!(3.89)),
        money(dec!(229.97), dec!(213.95)),
        money(dec!(8.00), dec!(7.45)),
        conversion(),
    )
    .into();

    match database
        .transactions()
        .insert(&placement, &transaction)
        .await
    {
        Err(StorageError::UnscaledValue { field, scale, .. }) => {
            assert_eq!(field, "unit_price");
            assert_eq!(scale, 6);
        }
        other => panic!("an unscaled unit price must be refused, got {other:?}"),
    }
}

/// The same boundary on an FX rate, whose scale is 6: a rate derived from a broker-stated pair
/// is where a seventh decimal appears [ARC-007], [ARC-009], [ARC-010].
#[tokio::test]
async fn an_fx_rate_beyond_six_decimals_is_refused() {
    let (_db, database) = open().await;
    let (placement, batch) = place(&database).await;

    let transaction: Transaction = Expiration::new(
        derivation(&database, batch).await,
        money(dec!(0.00), dec!(0.00)),
        money(dec!(0.00), dec!(0.00)),
        Conversion::new(
            Currency::new("USD"),
            FxRate::new(dec!(1.0745001)),
            RateSource::Ecb,
            date(),
        ),
    )
    .into();

    match database
        .transactions()
        .insert(&placement, &transaction)
        .await
    {
        Err(StorageError::UnscaledValue { field, scale, .. }) => {
            assert_eq!(field, "conversion_rate");
            assert_eq!(scale, 6);
        }
        other => panic!("an unscaled rate must be refused, got {other:?}"),
    }
}

/// A money figure shorter than its scale is padded on the way in, so every stored amount reads
/// alike [ARC-007].
#[tokio::test]
async fn a_short_money_figure_is_stored_padded() {
    let (db, database) = open().await;
    let (placement, batch) = place(&database).await;
    let transaction: Transaction = Expiration::new(
        derivation(&database, batch).await,
        money(dec!(1825.5), dec!(1825.5)),
        money(dec!(0), dec!(0)),
        Conversion::native(date()),
    )
    .into();

    database
        .transactions()
        .insert(&placement, &transaction)
        .await
        .expect("insert");

    let pool = SqlitePool::connect(&format!("sqlite://{}", db.path().display()))
        .await
        .expect("connect");
    let stored: String = query("select gross_native from transaction_expiration")
        .fetch_one(&pool)
        .await
        .expect("read the column")
        .get("gross_native");
    pool.close().await;

    assert_eq!(stored, "1825.50");
}

/// A connection to the file behind an open database, for the tests that have to write a row no
/// repository would write.
async fn raw(db: &TempDb) -> SqlitePool {
    SqlitePool::connect(&format!("sqlite://{}", db.path().display()))
        .await
        .expect("connect to the temporary database")
}

/// A key that was never stored reads back as `None` from every repository, rather than as an
/// error [TST-004].
#[tokio::test]
async fn an_absent_row_reads_back_as_none() {
    let (_db, database) = open().await;

    assert_eq!(
        database.securities().find(&isin()).await.expect("read"),
        None
    );
    assert_eq!(
        database
            .source_records()
            .find(&cite("absent"))
            .await
            .expect("read"),
        None
    );
    assert_eq!(
        database
            .import_batches()
            .find(BatchId::new(7))
            .await
            .expect("read"),
        None
    );
    assert_eq!(
        database
            .manual_entries()
            .find(ManualEntryId::new(7))
            .await
            .expect("read"),
        None
    );
}

/// A stored code this version does not know is reported, never guessed at: a column silently
/// read as the nearest variant is a security type, a transaction kind or a supplied shape that
/// changed by itself [TST-004].
#[tokio::test]
async fn a_stored_code_this_version_cannot_read_is_reported() {
    let (db, database) = open().await;
    database
        .accounts()
        .insert(&account())
        .await
        .expect("insert");

    let security = Security::new(isin(), "Philips", SecurityType::Stock, Quotation::PerUnit);
    database
        .securities()
        .insert(&security)
        .await
        .expect("insert");
    let batch = database
        .import_batches()
        .insert(&batch(SourceFormat::SaxoNlXlsx))
        .await
        .expect("the batch the cited records belong to");
    let transaction = database
        .transactions()
        .insert(
            &Placement::emitted(account(), isin()),
            &Split::new(derivation(&database, batch).await, one_for_three()).into(),
        )
        .await
        .expect("insert");
    let entry = database
        .manual_entries()
        .insert(&ManualEntry::new(
            account(),
            isin(),
            Supplied::Election(Election::Cash),
            [cite("4100200300")],
        ))
        .await
        .expect("insert");

    let pool = raw(&db).await;
    for statement in [
        "update security set security_type = 'commodity'",
        "update transaction_record set kind = 'merger'",
        "update manual_entry set supplied_kind = 'guess'",
    ] {
        query(statement).execute(&pool).await.expect("edit the row");
    }
    pool.close().await;

    for (field, read) in [
        (
            "security_type",
            database.securities().find(&isin()).await.err(),
        ),
        (
            "kind",
            database.transactions().find(transaction).await.err(),
        ),
        (
            "supplied_kind",
            database.manual_entries().find(entry).await.err(),
        ),
    ] {
        match read {
            Some(StorageError::CorruptValue {
                field: reported, ..
            }) => {
                assert_eq!(reported, field);
            }
            other => panic!("an unknown {field} must be reported, got {other:?}"),
        }
    }
}

/// A column that no longer holds a decimal is reported too, rather than read as zero
/// [TST-004].
#[tokio::test]
async fn a_stored_figure_that_is_not_a_decimal_is_reported() {
    let (db, database) = open().await;
    let (placement, batch) = place(&database).await;
    let id = database
        .transactions()
        .insert(
            &placement,
            &Sell::new(
                derivation(&database, batch).await,
                Quantity::new(dec!(55)),
                price(dec!(5.00), dec!(4.65)),
                money(dec!(275.00), dec!(255.93)),
                money(dec!(8.00), dec!(7.45)),
                conversion(),
            )
            .into(),
        )
        .await
        .expect("insert");

    let pool = raw(&db).await;
    query("update transaction_sell set quantity = 'fifty-five'")
        .execute(&pool)
        .await
        .expect("edit the row");
    pool.close().await;

    match database.transactions().find(id).await {
        Err(StorageError::CorruptValue { field, value }) => {
            assert_eq!(field, "quantity");
            assert_eq!(value, "fifty-five");
        }
        other => panic!("an unreadable figure must be reported, got {other:?}"),
    }
}

/// Reading back is a construction path too, and it builds nothing from nothing: a stored
/// transaction whose citations are gone is refused, not read back as derived from no record
/// [DOM-047], [TST-004].
#[tokio::test]
async fn a_stored_transaction_without_citations_is_refused_on_reading_back() {
    let (db, database) = open().await;
    let (placement, batch) = place(&database).await;
    let id = database
        .transactions()
        .insert(
            &placement,
            &Split::new(derivation(&database, batch).await, one_for_three()).into(),
        )
        .await
        .expect("insert");

    let pool = raw(&db).await;
    query("delete from transaction_citation where transaction_id = ?")
        .bind(id.get())
        .execute(&pool)
        .await
        .expect("remove the citations");
    pool.close().await;

    match database.transactions().find(id).await {
        Err(StorageError::CorruptValue { field, value }) => {
            assert_eq!(field, "transaction_citation");
            assert_eq!(value, id.to_string());
        }
        other => panic!("a transaction citing nothing must be refused, got {other:?}"),
    }
}

/// A `transfer_out` stored without its target and ratio, as one written before migration 0008
/// would be, reads back as a missing detail row, corrupt, rather than as a transfer to nowhere
/// [DOM-090], [TST-004].
#[tokio::test]
async fn a_transfer_out_without_its_target_is_refused_on_reading_back() {
    let (db, database) = open().await;
    let (placement, batch) = place(&database).await;
    let id = database
        .transactions()
        .insert(
            &placement,
            &TransferOut::new(
                derivation(&database, batch).await,
                Quantity::new(dec!(55)),
                money(dec!(0.00), dec!(0.00)),
                one_for_three(),
                conversion(),
                isin(),
            )
            .into(),
        )
        .await
        .expect("insert");

    let pool = raw(&db).await;
    query("delete from transaction_transfer_out_target where transaction_id = ?")
        .bind(id.get())
        .execute(&pool)
        .await
        .expect("remove the target row");
    pool.close().await;

    match database.transactions().find(id).await {
        Err(StorageError::CorruptValue { field, value }) => {
            assert_eq!(field, "transaction_id");
            assert_eq!(value, id.to_string());
        }
        other => panic!("a transfer to nowhere must be refused, got {other:?}"),
    }
}

/// A transaction's stored place in the canonical order is guarded too: a leg that is neither a
/// lead nor a trailing leg, and an `order` no file can have, are reported rather than read as some
/// other position [DOM-011], [TST-004].
#[tokio::test]
async fn a_stored_place_this_version_cannot_read_is_reported() {
    let (db, database) = open().await;
    let (placement, batch) = place(&database).await;
    let id = database
        .transactions()
        .insert(
            &placement,
            &Split::new(derivation(&database, batch).await, one_for_three()).into(),
        )
        .await
        .expect("insert");

    for (statement, field, value) in [
        (
            "update transaction_record set leg = 2, ordering = 0",
            "leg",
            "2",
        ),
        (
            "update transaction_record set leg = 0, ordering = -1",
            "ordering",
            "-1",
        ),
    ] {
        let pool = raw(&db).await;
        query(statement).execute(&pool).await.expect("edit the row");
        pool.close().await;

        match database.transactions().find(id).await {
            Err(StorageError::CorruptValue {
                field: reported,
                value: stored,
            }) => {
                assert_eq!(reported, field);
                assert_eq!(stored, value);
            }
            other => panic!("an unreadable {field} must be reported, got {other:?}"),
        }
    }
}

/// A split's ratio edited by hand to one `Ratio` cannot hold, zero or past a `u32`, is reported
/// rather than read back as some other split [DOM-113], [TST-004].
#[tokio::test]
async fn a_hand_edited_split_ratio_is_reported() {
    let (db, database) = open().await;
    let (placement, batch) = place(&database).await;
    let id = database
        .transactions()
        .insert(
            &placement,
            &Split::new(
                Derivation::new(
                    date(),
                    vec1![store_record(&database, batch, "4100200302", &[]).await],
                ),
                one_for_three(),
            )
            .into(),
        )
        .await
        .expect("insert");

    for (statement, field, value) in [
        (
            "update transaction_split set ratio_numerator = 0",
            "ratio_numerator",
            "0",
        ),
        (
            "update transaction_split set ratio_numerator = 1, ratio_denominator = 4294967296",
            "ratio_denominator",
            "4294967296",
        ),
    ] {
        let pool = raw(&db).await;
        query(statement).execute(&pool).await.expect("edit the row");
        pool.close().await;

        match database.transactions().find(id).await {
            Err(StorageError::CorruptValue {
                field: reported,
                value: stored,
            }) => {
                assert_eq!(reported, field);
                assert_eq!(stored, value);
            }
            other => panic!("an unreadable {field} must be reported, got {other:?}"),
        }
    }
}

/// The schema's nullable columns and its one structured column are guarded too: a missing
/// `shares`, a ratio part of zero and a `parsed` blob that is no longer JSON are reported rather
/// than read as a default [TST-004].
#[tokio::test]
async fn a_hand_edited_nullable_or_structured_column_is_reported() {
    let (db, database) = open().await;
    let (_placement, batch) = place(&database).await;

    let entry =
        |supplied, reference| ManualEntry::new(account(), isin(), supplied, [cite(reference)]);
    let stock = database
        .manual_entries()
        .insert(&entry(
            Supplied::Election(Election::Stock {
                shares: Quantity::new(dec!(3.00000000)),
            }),
            "4100200300",
        ))
        .await
        .expect("insert");
    let split = database
        .manual_entries()
        .insert(&entry(
            Supplied::Split(Ratio::new(
                NonZeroU32::new(3).expect("a non-zero numerator"),
                NonZeroU32::new(1).expect("a non-zero denominator"),
            )),
            "4100200301",
        ))
        .await
        .expect("insert");
    let record = SourceRecord::new(
        cite("4100200302"),
        Order::new(1),
        "\"2024-05-02\",\"BUY\"",
        BTreeMap::new(),
    );
    database
        .source_records()
        .insert(batch, &record)
        .await
        .expect("insert");

    let pool = raw(&db).await;
    for (statement, id) in [
        (
            "update manual_entry set shares = null where id = ?",
            stock.get(),
        ),
        (
            "update manual_entry set ratio_numerator = 0 where id = ?",
            split.get(),
        ),
    ] {
        query(statement)
            .bind(id)
            .execute(&pool)
            .await
            .expect("edit the row");
    }
    query("update source_record set parsed = 'not json'")
        .execute(&pool)
        .await
        .expect("edit the row");
    pool.close().await;

    for (field, read) in [
        ("shares", database.manual_entries().find(stock).await.err()),
        (
            "ratio_numerator",
            database.manual_entries().find(split).await.err(),
        ),
        (
            "parsed",
            database
                .source_records()
                .find(record.identity())
                .await
                .err(),
        ),
    ] {
        match read {
            Some(StorageError::CorruptValue {
                field: reported, ..
            }) => {
                assert_eq!(reported, field);
            }
            other => panic!("a hand-edited {field} must be reported, got {other:?}"),
        }
    }
}

/// Only the ISIN constraint becomes [`StorageError::DuplicateIsin`]; any other failure stays the
/// failure it was, so a database problem is never reported as a domain rule [DOM-071].
#[tokio::test]
async fn a_driver_failure_is_not_reported_as_a_duplicate_isin() {
    let (_db, database) = open().await;
    database.close().await;

    let refused = database
        .securities()
        .insert(&Security::new(
            isin(),
            "Philips",
            SecurityType::Stock,
            Quotation::PerUnit,
        ))
        .await;

    assert!(
        matches!(refused, Err(StorageError::Database(_))),
        "a closed pool is a driver failure, got {refused:?}"
    );
}

/// A source record of `batch` whose parsed fields carry `fields`, and storage's handle on it.
async fn store_record(
    database: &Database,
    batch: BatchId,
    reference: &str,
    fields: &[(&str, &str)],
) -> RecordHandle {
    let record = SourceRecord::new(
        cite(reference),
        Order::new(1),
        "raw",
        fields
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
    );
    database
        .source_records()
        .insert(batch, &record)
        .await
        .expect("insert the record")
}

/// A second account with the same broker and id is refused by name, not as a driver error;
/// the same id under another broker is a different account [SRV-007], [TST-004].
#[tokio::test]
async fn a_second_account_with_the_same_key_is_refused() {
    let (_db, database) = open().await;
    database
        .accounts()
        .insert(&account())
        .await
        .expect("insert");

    let refused = database.accounts().insert(&account()).await;
    assert!(
        matches!(
            &refused,
            Err(StorageError::DuplicateAccount { broker, id })
                if broker == "Saxo" && id == "69900/1000000"
        ),
        "{refused:?}"
    );

    let other_broker = Account::new("Trade Republic", "69900/1000000");
    database
        .accounts()
        .insert(&other_broker)
        .await
        .expect("insert");
    assert_eq!(
        database.accounts().list().await.expect("list"),
        vec![account(), other_broker]
    );
}

/// An account nothing refers to is deleted; one that is not stored is reported as unknown
/// [SRV-007], [TST-004].
#[tokio::test]
async fn an_unreferenced_account_is_deleted() {
    let (_db, database) = open().await;
    database
        .accounts()
        .insert(&account())
        .await
        .expect("insert");

    database
        .accounts()
        .delete(&account())
        .await
        .expect("delete");

    assert_eq!(database.accounts().list().await.expect("list"), vec![]);
    assert!(matches!(
        database.accounts().delete(&account()).await,
        Err(StorageError::UnknownAccount { .. })
    ));
}

/// An account a source record was imported into is not deleted, and the refusal counts what
/// holds it [SRV-008], [TST-004].
#[tokio::test]
async fn an_account_referenced_by_a_source_record_is_not_deleted() {
    let (_db, database) = open().await;
    let (_placement, batch) = place(&database).await;
    store_record(&database, batch, "r1", &[("Acties", "Koop")]).await;
    store_record(&database, batch, "r2", &[("Acties", "Verkoop")]).await;

    let refused = database.accounts().delete(&account()).await;

    match refused {
        Err(StorageError::AccountReferenced {
            source_records,
            batches,
            manual_entries,
            transactions,
            ..
        }) => assert_eq!(
            (source_records, batches, manual_entries, transactions),
            (2, 1, 0, 0)
        ),
        other => panic!("a referenced account must be refused, got {other:?}"),
    }
    assert_eq!(
        database.accounts().list().await.expect("list"),
        vec![account()]
    );
}

/// A manual entry or a transaction naming an account holds it as a record does: both carry a
/// foreign key to it, and deleting it from under them is not an option [SRV-008], [TST-004].
#[tokio::test]
async fn an_account_named_by_an_entry_or_a_transaction_is_not_deleted() {
    let (_db, database) = open().await;
    database
        .accounts()
        .insert(&account())
        .await
        .expect("insert");
    database
        .manual_entries()
        .insert(&ManualEntry::new(
            account(),
            isin(),
            Supplied::Election(Election::Cash),
            [cite("r1")],
        ))
        .await
        .expect("insert the entry");

    let refused = database.accounts().delete(&account()).await;
    assert!(
        matches!(
            refused,
            Err(StorageError::AccountReferenced {
                source_records: 0,
                batches: 0,
                manual_entries: 1,
                transactions: 0,
                ..
            })
        ),
        "{refused:?}"
    );

    let (_db, database) = open().await;
    let other = Account::new("Saxo", "other");
    database.accounts().insert(&other).await.expect("insert");
    let (_placement, batch) = place(&database).await;
    // A transaction no import derived, as an emitted transfer_in is, holds an account that
    // owns no batch and no record.
    database
        .transactions()
        .insert(
            &Placement::emitted(other.clone(), isin()),
            &Split::new(derivation(&database, batch).await, one_for_three()).into(),
        )
        .await
        .expect("insert the transaction");

    let refused = database.accounts().delete(&other).await;
    assert!(
        matches!(
            refused,
            Err(StorageError::AccountReferenced {
                source_records: 0,
                batches: 0,
                manual_entries: 0,
                transactions: 1,
                ..
            })
        ),
        "{refused:?}"
    );
}

/// An unreferenced account takes a new key; a referenced one keeps its key, since the
/// identities of its records are scoped to it; and a key another account holds is refused
/// [SRV-007], [DOM-024], [TST-004].
#[tokio::test]
async fn an_account_is_renamed_only_while_unreferenced() {
    let (_db, database) = open().await;
    let typo = Account::new("Saxo", "69900/100000");
    database.accounts().insert(&typo).await.expect("insert");

    database
        .accounts()
        .rename(&typo, &account())
        .await
        .expect("rename");
    assert_eq!(
        database.accounts().list().await.expect("list"),
        vec![account()]
    );

    let taken = Account::new("Saxo", "taken");
    database.accounts().insert(&taken).await.expect("insert");
    assert!(matches!(
        database.accounts().rename(&taken, &account()).await,
        Err(StorageError::DuplicateAccount { id, .. }) if id == "69900/1000000"
    ));
    assert!(matches!(
        database.accounts().rename(&typo, &account()).await,
        Err(StorageError::UnknownAccount { .. })
    ));

    let batch = database
        .import_batches()
        .insert(&batch(SourceFormat::SaxoNlXlsx))
        .await
        .expect("the batch");
    store_record(&database, batch, "r1", &[]).await;
    assert!(matches!(
        database.accounts().rename(&account(), &typo).await,
        Err(StorageError::AccountReferenced {
            source_records: 1,
            ..
        })
    ));
    assert!(
        database
            .accounts()
            .find("Saxo", "69900/1000000")
            .await
            .expect("read back")
            .is_some()
    );
}

/// A batch that owns no record, or a manual entry, holds an account against a change of key as
/// against a deletion: SRV-008 names each as a ground of its own [SRV-008], [DEC-089],
/// [TST-004].
#[tokio::test]
async fn a_batch_or_an_entry_alone_holds_an_account_against_rename_and_delete() {
    let (_db, database) = open().await;
    database
        .accounts()
        .insert(&account())
        .await
        .expect("insert");
    database
        .import_batches()
        .insert(&batch(SourceFormat::SaxoNlXlsx))
        .await
        .expect("a batch owning no record");
    let renamed = Account::new("Saxo", "renamed");

    for refused in [
        database.accounts().delete(&account()).await,
        database.accounts().rename(&account(), &renamed).await,
    ] {
        assert!(
            matches!(
                refused,
                Err(StorageError::AccountReferenced {
                    source_records: 0,
                    batches: 1,
                    manual_entries: 0,
                    transactions: 0,
                    ..
                })
            ),
            "{refused:?}"
        );
    }

    let (_db, database) = open().await;
    database
        .accounts()
        .insert(&account())
        .await
        .expect("insert");
    database
        .manual_entries()
        .insert(&ManualEntry::new(
            account(),
            isin(),
            Supplied::Election(Election::Cash),
            [cite("r1")],
        ))
        .await
        .expect("insert the entry");
    let refused = database.accounts().rename(&account(), &renamed).await;
    assert!(
        matches!(
            refused,
            Err(StorageError::AccountReferenced {
                manual_entries: 1,
                ..
            })
        ),
        "{refused:?}"
    );
    assert_eq!(
        database.accounts().list().await.expect("list"),
        vec![account()]
    );
}

/// Writing a referenced account back under the key it already has changes neither broker nor
/// id, so it is not the change SRV-008 refuses; an absent account is still a 404's error
/// [SRV-008], [DEC-089], [TST-004].
#[tokio::test]
async fn a_referenced_account_renamed_to_its_own_key_is_unchanged() {
    let (_db, database) = open().await;
    let (_placement, batch) = place(&database).await;
    store_record(&database, batch, "r1", &[]).await;

    database
        .accounts()
        .rename(&account(), &account())
        .await
        .expect("the same key is not a change");
    assert_eq!(
        database.accounts().list().await.expect("list"),
        vec![account()]
    );

    let absent = Account::new("Saxo", "absent");
    assert!(matches!(
        database.accounts().rename(&absent, &absent).await,
        Err(StorageError::UnknownAccount { .. })
    ));
}

/// Name, type and quotation are replaced, each independently of the others, and the
/// auto-created flag survives the edit [SRV-011], [DOM-006], [DOM-037], [TST-004].
#[tokio::test]
async fn a_security_is_updated_and_keeps_its_provenance() {
    let (_db, database) = open().await;
    let imported = Security::auto_created(
        isin(),
        "NL 7.5% 2023",
        SecurityType::Stock,
        Quotation::PerUnit,
    );
    database
        .securities()
        .insert(&imported)
        .await
        .expect("insert");

    let updated = database
        .securities()
        .update(
            &isin(),
            "Netherlands 7.5% 2023",
            SecurityType::Bond,
            Quotation::PercentOfPar,
        )
        .await
        .expect("update");

    let expected = Security::auto_created(
        isin(),
        "Netherlands 7.5% 2023",
        SecurityType::Bond,
        Quotation::PercentOfPar,
    );
    assert_eq!(updated, expected);
    assert_eq!(
        database
            .securities()
            .find(&isin())
            .await
            .expect("read back"),
        Some(expected)
    );
    assert!(matches!(
        database
            .securities()
            .update(
                &Isin::new("XS0000000001"),
                "",
                SecurityType::Bond,
                Quotation::PerUnit
            )
            .await,
        Err(StorageError::UnknownSecurity { isin }) if isin == "XS0000000001"
    ));
}

/// An imported security is stored needing review; an edit leaves it so, and marking it
/// reviewed clears it and nothing else, however often it is asked [DOM-126], [SRV-057],
/// [DOM-006], [TST-004].
#[tokio::test]
async fn only_marking_a_security_reviewed_clears_needs_review() {
    let (_db, database) = open().await;
    database
        .securities()
        .insert(&Security::auto_created(
            isin(),
            "NL 7.5% 2023",
            SecurityType::Other,
            Quotation::PerUnit,
        ))
        .await
        .expect("insert");

    let edited = database
        .securities()
        .update(
            &isin(),
            "NL 7.5% 2023",
            SecurityType::Bond,
            Quotation::PercentOfPar,
        )
        .await
        .expect("update");
    assert!(edited.needs_review());

    let expected = Security::auto_created(
        isin(),
        "NL 7.5% 2023",
        SecurityType::Bond,
        Quotation::PercentOfPar,
    )
    .reviewed();
    for _ in 0..2 {
        assert_eq!(
            database
                .securities()
                .mark_reviewed(&isin())
                .await
                .expect("mark reviewed"),
            expected
        );
    }
    assert_eq!(
        database.securities().list().await.expect("list"),
        vec![expected.clone()]
    );

    let edited_again = database
        .securities()
        .update(
            &isin(),
            "NL 7.5% 2023",
            SecurityType::Bond,
            Quotation::PerUnit,
        )
        .await
        .expect("update");
    assert!(
        !edited_again.needs_review(),
        "an edit does not set it either"
    );
    assert!(matches!(
        database
            .securities()
            .mark_reviewed(&Isin::new("XS0000000001"))
            .await,
        Err(StorageError::UnknownSecurity { isin }) if isin == "XS0000000001"
    ));
}

/// Needs review is independent of provenance, so a user-entered security carrying it reads
/// back as it is stored, and marking it reviewed clears it [DOM-126], [SRV-057], [TST-004].
#[tokio::test]
async fn a_user_entered_security_needing_review_is_read_as_stored() {
    let (db, database) = open().await;
    let by_user = Security::new(isin(), "Philips", SecurityType::Stock, Quotation::PerUnit);
    database
        .securities()
        .insert(&by_user)
        .await
        .expect("insert");

    query("update security set needs_review = 1")
        .execute(&raw(&db).await)
        .await
        .expect("edit the row by hand");

    let found = database
        .securities()
        .find(&isin())
        .await
        .expect("find")
        .expect("the security is stored");
    assert!(found.needs_review());
    assert!(!found.is_auto_created());
    assert_eq!(found.reviewed(), by_user);
    assert_eq!(
        database
            .securities()
            .mark_reviewed(&isin())
            .await
            .expect("mark reviewed"),
        by_user
    );
}

/// A security auto-created before needs review was stored had never been reviewed, so the
/// migration leaves it needing review, and a user-entered one not [DOM-126], [TST-004].
#[tokio::test]
async fn the_migration_marks_earlier_auto_created_securities_as_needing_review() {
    let db = TempDb::new();
    let earlier = tempfile::tempdir().expect("a directory for the earlier migrations");
    let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    for name in [
        "0001_initial.sql",
        "0002_fx_rate.sql",
        "0003_invariants.sql",
        "0004_no_failed_count.sql",
    ] {
        std::fs::copy(migrations.join(name), earlier.path().join(name)).expect("copy a migration");
    }
    let pool = SqlitePool::connect(&format!("sqlite://{}?mode=rwc", db.path().display()))
        .await
        .expect("create the database");
    sqlx::migrate::Migrator::new(earlier.path())
        .await
        .expect("read the earlier migrations")
        .run(&pool)
        .await
        .expect("migrate to the schema before needs review");
    query(
        "insert into security (isin, name, security_type, quotation, auto_created)
         values ('NL0000009538', 'Philips', 'stock', 'per_unit', 1),
                ('US0378331005', 'Apple', 'stock', 'per_unit', 0)",
    )
    .execute(&pool)
    .await
    .expect("insert securities under the earlier schema");
    pool.close().await;

    let database = Database::open(db.path()).await.expect("migrate to current");

    let needs_review: Vec<_> = database
        .securities()
        .list()
        .await
        .expect("list")
        .iter()
        .map(|security| (security.isin().as_str().to_owned(), security.needs_review()))
        .collect();
    assert_eq!(
        needs_review,
        vec![
            ("NL0000009538".to_owned(), true),
            ("US0378331005".to_owned(), false),
        ]
    );
}

/// Records stored before the oldest supplier was a fact of its own take their owner as it, which
/// until a re-import moves ownership is the batch that first supplied them; transactions take the
/// position of the lowest record they cite [DOM-011], [DOM-111], [TST-004].
///
/// The edges: two cited records of one `order` from different batches, where the older batch
/// wins, while a higher `order` from an older batch still loses; a record no batch owns, whose age stays 0; and a transaction citing only a record no
/// longer stored, whose position stays (0, 0), as early as any can be.
#[tokio::test]
async fn the_migration_fills_the_canonical_order_of_earlier_rows() {
    let db = TempDb::new();
    let earlier = tempfile::tempdir().expect("a directory for the earlier migrations");
    let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    for name in [
        "0001_initial.sql",
        "0002_fx_rate.sql",
        "0003_invariants.sql",
        "0004_no_failed_count.sql",
        "0005_needs_review.sql",
    ] {
        std::fs::copy(migrations.join(name), earlier.path().join(name)).expect("copy a migration");
    }
    let pool = SqlitePool::connect(&format!("sqlite://{}?mode=rwc", db.path().display()))
        .await
        .expect("create the database");
    sqlx::migrate::Migrator::new(earlier.path())
        .await
        .expect("read the earlier migrations")
        .run(&pool)
        .await
        .expect("migrate to the schema before the canonical order");
    for statement in [
        "insert into account (broker, id) values ('Saxo', '69900/1000000')",
        "insert into import_batch
             (id, account_broker, account_id, filename, format, imported_at,
              derived, pending, non_position)
         values (3, 'Saxo', '69900/1000000', '2024.xlsx', 'saxo_nl_xlsx',
                 '2024-05-03T09:00:00Z', 1, 0, 0)",
        "insert into import_batch
             (id, account_broker, account_id, filename, format, imported_at,
              derived, pending, non_position)
         values (2, 'Saxo', '69900/1000000', '2023.xlsx', 'saxo_nl_xlsx',
                 '2024-05-03T09:00:00Z', 1, 0, 0)",
        "insert into import_batch
             (id, account_broker, account_id, filename, format, imported_at,
              derived, pending, non_position)
         values (1, 'Saxo', '69900/1000000', '2022.xlsx', 'saxo_nl_xlsx',
                 '2024-05-03T09:00:00Z', 1, 0, 0)",
        // Expirations, whose fields this schema already holds in full: a split now has a ratio
        // [DOM-113] and a transfer out a target and ratio [DOM-090] that no earlier row can
        // supply.
        "insert into transaction_record (id, kind, trade_date)
         values (1, 'expiration', '2024-05-02')",
        "insert into transaction_record (id, kind, trade_date)
         values (2, 'expiration', '2024-05-02')",
        "insert into transaction_expiration
             (transaction_id, gross_native, gross_eur, fees_native, fees_eur,
              conversion_currency, conversion_rate, conversion_source, conversion_rate_date)
         values (1, '0.00', '0.00', '0.00', '0.00', 'EUR', '1.000000', 'native', '2024-05-02'),
                (2, '0.00', '0.00', '0.00', '0.00', 'EUR', '1.000000', 'native', '2024-05-02')",
    ] {
        query(statement)
            .execute(&pool)
            .await
            .expect("write a row under the earlier schema");
    }
    // The cash row is cited first and sits later in the file. The older batch's row shares the
    // position row's `order` and is written after it, so only the batch age picks it. The oldest
    // batch's row has a higher `order`, so keying on the batch age first would pick it instead.
    for (ordinal, reference, ordering, batch) in [
        (0i64, "cash", 7i64, 3i64),
        (1, "position", 4, 3),
        (2, "position-older", 4, 2),
        (3, "later-in-oldest", 5, 1),
    ] {
        query(
            "insert into source_record (identity, ordering, raw, parsed, batch_id)
             values (?, ?, '', '{}', ?)",
        )
        .bind(cite(reference).as_str())
        .bind(ordering)
        .bind(batch)
        .execute(&pool)
        .await
        .expect("write a record under the earlier schema");
        query(
            "insert into transaction_citation (transaction_id, ordinal, record_identity)
             values (1, ?, ?)",
        )
        .bind(ordinal)
        .bind(cite(reference).as_str())
        .execute(&pool)
        .await
        .expect("write a citation under the earlier schema");
    }
    query("insert into source_record (identity, ordering, raw, parsed) values (?, 2, '', '{}')")
        .bind(cite("unowned").as_str())
        .execute(&pool)
        .await
        .expect("write a record no batch owns under the earlier schema");
    query(
        "insert into transaction_citation (transaction_id, ordinal, record_identity)
         values (2, 0, ?)",
    )
    .bind(cite("gone").as_str())
    .execute(&pool)
    .await
    .expect("write a citation of a record no longer stored under the earlier schema");
    pool.close().await;

    let database = Database::open(db.path()).await.expect("migrate to current");

    let handle = database
        .source_records()
        .handle(&cite("cash"))
        .await
        .expect("read the handle")
        .expect("the record is stored");
    assert_eq!(
        handle.position(),
        RecordPosition::new(Order::new(7), BatchAge::new(3))
    );
    let expiration = database
        .transactions()
        .find(TransactionId::new(1))
        .await
        .expect("read back")
        .expect("the transaction is stored");
    assert_eq!(
        expiration.order_key(),
        OrderKey::new(
            date(),
            RecordPosition::new(Order::new(4), BatchAge::new(2)),
            Leg::Lead
        ),
        "the lowest cited record's position, not the first cited, the older batch on a tie, \
         the order before the batch age"
    );

    let unowned = database
        .source_records()
        .handle(&cite("unowned"))
        .await
        .expect("read the handle")
        .expect("the record is stored");
    assert_eq!(
        unowned.position(),
        RecordPosition::new(Order::new(2), BatchAge::new(0)),
        "a record no batch owns keeps age 0"
    );
    let citing_nothing_stored = database
        .transactions()
        .find(TransactionId::new(2))
        .await
        .expect("read back")
        .expect("the transaction is stored");
    assert_eq!(
        citing_nothing_stored.order_key(),
        OrderKey::new(
            date(),
            RecordPosition::new(Order::new(0), BatchAge::new(0)),
            Leg::Lead
        ),
        "a transaction whose records are gone keeps the earliest position"
    );
}

/// Securities list in ISIN order, and one nothing names is deleted [SRV-007], [TST-004].
#[tokio::test]
async fn an_unreferenced_security_is_listed_and_deleted() {
    let (_db, database) = open().await;
    let later = Security::new(
        Isin::new("US0378331005"),
        "Apple",
        SecurityType::Stock,
        Quotation::PerUnit,
    );
    let earlier = Security::new(isin(), "Philips", SecurityType::Stock, Quotation::PerUnit);
    database.securities().insert(&later).await.expect("insert");
    database
        .securities()
        .insert(&earlier)
        .await
        .expect("insert");
    assert_eq!(
        database.securities().list().await.expect("list"),
        vec![earlier, later.clone()]
    );

    database.securities().delete(&isin()).await.expect("delete");

    assert_eq!(
        database.securities().list().await.expect("list"),
        vec![later]
    );
    assert!(matches!(
        database.securities().delete(&isin()).await,
        Err(StorageError::UnknownSecurity { .. })
    ));
}

/// A security any source record names — under either format's column, in a second role such
/// as a corporate action's target, or in another spelling — is not deleted, and a record
/// naming only another security does not hold it [SRV-009], [TST-004].
#[tokio::test]
async fn a_security_a_source_record_names_is_not_deleted() {
    let (_db, database) = open().await;
    let (_placement, batch) = place(&database).await;
    let target = Isin::new("US0378331005");
    let unnamed = Isin::new("DE0007164600");
    for other in [&target, &unnamed] {
        database
            .securities()
            .insert(&Security::new(
                other.clone(),
                "other",
                SecurityType::Stock,
                Quotation::PerUnit,
            ))
            .await
            .expect("insert");
    }
    store_record(
        &database,
        batch,
        "saxo",
        &[("Instrument ISIN", "NL0000009538"), ("Acties", "Koop")],
    )
    .await;
    store_record(&database, batch, "tr", &[("symbol", " nl0000009538")]).await;
    store_record(
        &database,
        batch,
        "exchange",
        &[
            ("Instrument ISIN", "XF0000000103"),
            ("Doel", "US0378331005"),
        ],
    )
    .await;

    let refused = database.securities().delete(&isin()).await;
    assert!(
        matches!(
            &refused,
            Err(StorageError::SecurityReferenced { isin, source_records: 2, transactions: 0 })
                if isin == "NL0000009538"
        ),
        "{refused:?}"
    );
    assert!(matches!(
        database.securities().delete(&target).await,
        Err(StorageError::SecurityReferenced {
            source_records: 1,
            ..
        })
    ));

    database
        .securities()
        .delete(&unnamed)
        .await
        .expect("a security no record names is deleted");
}

/// A value padded with whitespace SQLite's `trim` leaves alone — a non-breaking space, as Saxo
/// files carry [DOM-120], or a tab — still names the security, because the importer's
/// `Isin::new` strips it and so created the security under the bare ISIN [SRV-009], [TST-004].
#[tokio::test]
async fn a_security_named_with_unicode_padding_is_not_deleted() {
    let (_db, database) = open().await;
    let (_placement, batch) = place(&database).await;
    store_record(
        &database,
        batch,
        "nbsp",
        &[("Instrument ISIN", "NL0000009538\u{a0}")],
    )
    .await;
    store_record(&database, batch, "tab", &[("symbol", "\tnl0000009538")]).await;

    let refused = database.securities().delete(&isin()).await;

    assert!(
        matches!(
            refused,
            Err(StorageError::SecurityReferenced {
                source_records: 2,
                transactions: 0,
                ..
            })
        ),
        "{refused:?}"
    );
}

/// A transaction placed on a security holds it even with no record naming it, as an emitted
/// `transfer_in` is [SRV-009], [DOM-090], [TST-004].
#[tokio::test]
async fn a_security_a_transaction_is_placed_on_is_not_deleted() {
    let (_db, database) = open().await;
    let (_placement, batch) = place(&database).await;
    database
        .transactions()
        .insert(
            &Placement::emitted(account(), isin()),
            &Split::new(derivation(&database, batch).await, one_for_three()).into(),
        )
        .await
        .expect("insert the transaction");

    let refused = database.securities().delete(&isin()).await;

    assert!(
        matches!(
            refused,
            Err(StorageError::SecurityReferenced {
                source_records: 0,
                transactions: 1,
                ..
            })
        ),
        "{refused:?}"
    );
}

/// An emission stored before the inherited opening was a fact of its own takes the one allocated
/// opening of its `transfer_out` that holds its own place, which an emitted record takes from its
/// parcel (DEC-105) [DOM-096], [TST-004]. The edges guess nothing: an emission whose place two
/// allocated openings share (DEC-095), and one whose `transfer_out` has no attribution any more,
/// both stay null.
#[tokio::test]
async fn the_migration_fills_the_inherited_opening_of_earlier_emissions() {
    let db = TempDb::new();
    let earlier = tempfile::tempdir().expect("a directory for the earlier migrations");
    let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    for name in [
        "0001_initial.sql",
        "0002_fx_rate.sql",
        "0003_invariants.sql",
        "0004_no_failed_count.sql",
        "0005_needs_review.sql",
        "0006_canonical_order.sql",
        "0007_split_ratio.sql",
        "0008_transfer_out_target.sql",
    ] {
        std::fs::copy(migrations.join(name), earlier.path().join(name)).expect("copy a migration");
    }
    let pool = SqlitePool::connect(&format!("sqlite://{}?mode=rwc", db.path().display()))
        .await
        .expect("create the database");
    sqlx::migrate::Migrator::new(earlier.path())
        .await
        .expect("read the earlier migrations")
        .run(&pool)
        .await
        .expect("migrate to the schema before the inherited opening");
    // Only what the backfill reads: headers with their places, attributions, allocations and the
    // emission links. Transfer 10 consumed openings 1 and 2 and emitted 11 at 2's place; transfer
    // 20 consumed 3 and 4, which share a place, and emitted 21 there; transfer 30 emitted 31 and
    // its attribution is gone.
    for statement in [
        "insert into transaction_record (id, kind, trade_date, ordering, batch_age, leg) values
             (1, 'buy', '2024-01-01', 0, 1, 0),
             (2, 'buy', '2024-01-02', 0, 1, 0),
             (3, 'buy', '2024-01-03', 4, 1, 0),
             (4, 'buy', '2024-01-03', 4, 1, 0),
             (10, 'transfer_out', '2024-02-01', 0, 1, 0),
             (11, 'transfer_in', '2024-01-02', 0, 1, 0),
             (20, 'transfer_out', '2024-02-02', 0, 1, 0),
             (21, 'transfer_in', '2024-01-03', 4, 1, 0),
             (30, 'transfer_out', '2024-02-03', 0, 1, 0),
             (31, 'transfer_in', '2024-01-01', 0, 1, 0)",
        "insert into attribution (id, closing_transaction_id) values (1, 10), (2, 20)",
        "insert into attribution_allocation
             (attribution_id, ordinal, opening_transaction_id, quantity) values
             (1, 0, 1, '1'), (1, 1, 2, '1'), (2, 0, 3, '1'), (2, 1, 4, '1')",
        "insert into emitted_transfer_in (transfer_in_id, transfer_out_id) values
             (11, 10), (21, 20), (31, 30)",
    ] {
        query(statement)
            .execute(&pool)
            .await
            .expect("insert under the earlier schema");
    }
    pool.close().await;

    let database = Database::open(db.path()).await.expect("migrate to current");
    database.close().await;

    let pool = raw(&db).await;
    let inherited: Vec<(i64, Option<i64>)> = query(
        "select transfer_in_id, inherited_opening_id from emitted_transfer_in
         order by transfer_in_id",
    )
    .fetch_all(&pool)
    .await
    .expect("read the links")
    .iter()
    .map(|row| (row.get("transfer_in_id"), row.get("inherited_opening_id")))
    .collect();
    assert_eq!(inherited, vec![(11, Some(2)), (21, None), (31, None)]);
}

/// A record stored before the supplier relation existed takes its owner as its one supplier,
/// since no re-import recorded what it supplied (DEC-117, provisional); a record no batch owns
/// has none. A re-import afterwards adds itself beside that owner and takes the record over
/// [SRV-052], [TST-004].
#[tokio::test]
async fn the_migration_fills_each_records_supplier_from_its_owner() {
    let db = TempDb::new();
    let earlier = tempfile::tempdir().expect("a directory for the earlier migrations");
    let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    for name in [
        "0001_initial.sql",
        "0002_fx_rate.sql",
        "0003_invariants.sql",
        "0004_no_failed_count.sql",
        "0005_needs_review.sql",
        "0006_canonical_order.sql",
        "0007_split_ratio.sql",
        "0008_transfer_out_target.sql",
        "0009_inherited_opening.sql",
    ] {
        std::fs::copy(migrations.join(name), earlier.path().join(name)).expect("copy a migration");
    }
    let pool = SqlitePool::connect(&format!("sqlite://{}?mode=rwc", db.path().display()))
        .await
        .expect("create the database");
    sqlx::migrate::Migrator::new(earlier.path())
        .await
        .expect("read the earlier migrations")
        .run(&pool)
        .await
        .expect("migrate to the schema before the supplier relation");
    // Batch 2 is a re-import of batch 1's year, which under that schema owned nothing.
    for statement in [
        "insert into account (broker, id) values ('Saxo', '69900/1000000')",
        "insert into import_batch
             (id, account_broker, account_id, filename, format, imported_at,
              derived, pending, non_position)
         values (1, 'Saxo', '69900/1000000', '2024.xlsx', 'saxo_nl_xlsx',
                 '2024-05-03T09:00:00Z', 1, 0, 0),
                (2, 'Saxo', '69900/1000000', '2024.xlsx', 'saxo_nl_xlsx',
                 '2024-05-04T09:00:00Z', 1, 0, 0),
                (3, 'Saxo', '69900/1000000', '2024.xlsx', 'saxo_nl_xlsx',
                 '2024-05-05T09:00:00Z', 1, 0, 0)",
    ] {
        query(statement)
            .execute(&pool)
            .await
            .expect("insert under the earlier schema");
    }
    query(
        "insert into source_record (identity, ordering, raw, parsed, batch_id, first_batch_id)
         values (?, 0, '', '{}', 1, 1), (?, 1, '', '{}', null, 0)",
    )
    .bind(cite("owned").as_str())
    .bind(cite("unowned").as_str())
    .execute(&pool)
    .await
    .expect("write the records under the earlier schema");
    pool.close().await;

    let database = Database::open(db.path()).await.expect("migrate to current");
    let records = database.source_records();
    assert_eq!(
        records.suppliers(&cite("owned")).await.expect("suppliers"),
        [BatchId::new(1)],
        "the owner is the only supplier anything states; the re-import is not guessed"
    );
    assert!(
        records
            .suppliers(&cite("unowned"))
            .await
            .expect("suppliers")
            .is_empty()
    );

    let owned = records
        .find(&cite("owned"))
        .await
        .expect("read the record")
        .expect("the record is stored");
    assert!(
        !records
            .supply(BatchId::new(3), &owned)
            .await
            .expect("supply it again")
    );
    assert_eq!(
        records.suppliers(&cite("owned")).await.expect("suppliers"),
        [BatchId::new(1), BatchId::new(3)]
    );
    assert_eq!(
        records
            .handle(&cite("owned"))
            .await
            .expect("read the handle")
            .expect("the record is stored")
            .position(),
        RecordPosition::new(Order::new(0), BatchAge::new(1)),
        "the first supplier stays put (DEC-092)"
    );
    database.close().await;
}

/// Deleting a batch that supplied a record it did not own leaves the record with its owner and
/// drops only that batch from its suppliers: a deletion removes what the batch owns and nothing
/// else [SRV-021], [SRV-052], [TST-004].
#[tokio::test]
async fn deleting_a_supplier_that_does_not_own_a_record_leaves_its_owner() {
    let (_db, database) = open().await;
    let (_, first) = place(&database).await;
    let [second, third] = [
        database
            .import_batches()
            .insert(&batch(SourceFormat::SaxoNlXlsx))
            .await
            .expect("the second batch"),
        database
            .import_batches()
            .insert(&batch(SourceFormat::SaxoNlXlsx))
            .await
            .expect("the third batch"),
    ];
    let record = SourceRecord::new(cite("r1"), Order::new(1), "raw", BTreeMap::new());
    for supplier in [first, second, third] {
        database
            .source_records()
            .supply(supplier, &record)
            .await
            .expect("supply the record");
    }

    database
        .import_batches()
        .delete(second)
        .await
        .expect("the middle supplier owns nothing");

    assert_eq!(
        database
            .source_records()
            .suppliers(&cite("r1"))
            .await
            .expect("the suppliers"),
        vec![first, third]
    );
    database
        .import_batches()
        .delete(first)
        .await
        .expect("the first supplier owns nothing either");
    assert_eq!(
        database
            .source_records()
            .find(&cite("r1"))
            .await
            .expect("read back"),
        Some(record.clone()),
        "the owner still holds the record"
    );
    database
        .import_batches()
        .delete(third)
        .await
        .expect("the owner, now the only supplier");
    assert_eq!(
        database
            .source_records()
            .find(&cite("r1"))
            .await
            .expect("read back"),
        None
    );
}
