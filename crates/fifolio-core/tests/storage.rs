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
use fifolio_core::storage::{
    BatchId, DEFAULT_DATABASE_PATH, Database, ManualEntryId, StorageError, TransactionId,
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

fn derivation() -> Derivation {
    Derivation::new(date(), [cite("4100200300"), cite("4100200301")])
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

    database
        .source_records()
        .insert(&record)
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
            failed: 0,
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
/// and its conversion [DOM-010], [DOM-028], [DOM-029], [DOM-085], [TST-004].
#[tokio::test]
async fn every_transaction_variant_round_trips() {
    let (_db, database) = open().await;

    let variants: Vec<Transaction> = vec![
        Buy::new(
            derivation(),
            Quantity::new(dec!(55)),
            price(dec!(4.18), dec!(3.89)),
            money(dec!(230.00), dec!(214.05)),
            money(dec!(8.00), dec!(7.45)),
            BuyOrigin::Purchase,
            conversion(),
        )
        .into(),
        Buy::new(
            derivation(),
            Quantity::new(dec!(3)),
            price(dec!(26.10), dec!(26.10)),
            money(dec!(78.30), dec!(78.30)),
            money(dec!(0.00), dec!(0.00)),
            BuyOrigin::StockDividend,
            Conversion::native(date()),
        )
        .into(),
        TransferIn::new(
            derivation(),
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
            derivation(),
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
            derivation(),
            Quantity::new(dec!(55)),
            price(dec!(5.00), dec!(4.65)),
            money(dec!(275.00), dec!(255.93)),
            money(dec!(8.00), dec!(7.45)),
            conversion(),
        )
        .into(),
        Expiration::new(
            derivation(),
            money(dec!(0.00), dec!(0.00)),
            money(dec!(0.00), dec!(0.00)),
            conversion(),
        )
        .into(),
        TransferOut::new(
            derivation(),
            Quantity::new(dec!(55)),
            money(dec!(15.00), dec!(13.96)),
            conversion(),
        )
        .into(),
        Split::new(Derivation::new(date(), [cite("4100200302")])).into(),
    ];

    for transaction in variants {
        let id = database
            .transactions()
            .insert(&transaction)
            .await
            .expect("insert");

        assert_eq!(
            database.transactions().find(id).await.expect("read back"),
            Some(transaction.clone()),
            "a variant must read back as itself"
        );
    }
}

/// A transaction's citations keep the caller's order, which is the shape a multi-row corporate
/// action had [DOM-016], [TST-004].
#[tokio::test]
async fn citations_keep_their_order() {
    let (_db, database) = open().await;
    let cites = [cite("c"), cite("a"), cite("b")];
    let transaction: Transaction = Split::new(Derivation::new(date(), cites.clone())).into();

    let id = database
        .transactions()
        .insert(&transaction)
        .await
        .expect("insert");
    let stored = database
        .transactions()
        .find(id)
        .await
        .expect("read back")
        .expect("a stored transaction");

    assert_eq!(stored.cites(), cites.as_slice());
}

/// A transaction that was never stored is absent rather than an error [TST-004].
#[tokio::test]
async fn an_absent_transaction_reads_back_as_none() {
    let (_db, database) = open().await;
    let id = database
        .transactions()
        .insert(&Split::new(derivation()).into())
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

    // 214.054 is four cents and a half of a cent; storing it would round a figure the caller
    // never rounded, and the stored gross is what every calculation reads [DOM-085].
    let unscaled: Transaction = Buy::new(
        derivation(),
        Quantity::new(dec!(55)),
        price(dec!(4.18), dec!(3.89)),
        money(dec!(230.00), dec!(214.054)),
        money(dec!(8.00), dec!(7.45)),
        BuyOrigin::Purchase,
        conversion(),
    )
    .into();

    match database.transactions().insert(&unscaled).await {
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

    let transaction: Transaction = Sell::new(
        derivation(),
        Quantity::new(dec!(55)),
        price(dec!(4.1812345), dec!(3.89)),
        money(dec!(229.97), dec!(213.95)),
        money(dec!(8.00), dec!(7.45)),
        conversion(),
    )
    .into();

    match database.transactions().insert(&transaction).await {
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

    let transaction: Transaction = Expiration::new(
        derivation(),
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

    match database.transactions().insert(&transaction).await {
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
    let transaction: Transaction = Expiration::new(
        derivation(),
        money(dec!(1825.5), dec!(1825.5)),
        money(dec!(0), dec!(0)),
        Conversion::native(date()),
    )
    .into();

    database
        .transactions()
        .insert(&transaction)
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
    let transaction = database
        .transactions()
        .insert(&Split::new(derivation()).into())
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
    let id = database
        .transactions()
        .insert(
            &Sell::new(
                derivation(),
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

/// The schema's nullable columns and its one structured column are guarded too: a missing
/// `shares`, a ratio part of zero and a `parsed` blob that is no longer JSON are reported rather
/// than read as a default [TST-004].
#[tokio::test]
async fn a_hand_edited_nullable_or_structured_column_is_reported() {
    let (db, database) = open().await;
    database
        .accounts()
        .insert(&account())
        .await
        .expect("insert");

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
        .insert(&record)
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
