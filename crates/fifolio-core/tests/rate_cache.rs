//! Integration layer [TST-003]: the ECB rate cache against a real temporary SQLite database,
//! seeded from the committed recorded fragments of the series [ARC-015 to ARC-018], [TST-004].
//!
//! The feed is the injected [`RateFeed`] port and every test supplies it from a file on disk
//! [TST-020], so nothing here opens a socket [TST-019]. There is no code path in this crate
//! that could: the fetch happens outside it [ARC-002].

use std::fs;
use std::path::PathBuf;

use chrono::NaiveDate;
use fifolio_core::decimal::FxRate;
use fifolio_core::ecb::{FeedError, IngestError, Observation, RateFeed, seed, top_up};
use fifolio_core::fx::{RateTable, Stated, resolve};
use fifolio_core::storage::{Database, StorageError};
use fifolio_core::valuation::{Currency, RateSource};
use fifolio_test_support::TempDb;
use rust_decimal_macros::dec;
use sqlx::sqlite::SqlitePool;
use sqlx::{Row, query};

/// The committed fragments, read from disk rather than fetched [TST-019].
struct RecordedFeed {
    historical: String,
    recent: String,
}

impl RecordedFeed {
    fn new() -> Self {
        Self {
            historical: fragment("eurofxref-hist-fragment.xml"),
            recent: fragment("eurofxref-90d-fragment.xml"),
        }
    }

    /// A feed whose documents are replaced, for the cases the fragments cannot state.
    fn of(historical: &str, recent: &str) -> Self {
        Self {
            historical: historical.to_owned(),
            recent: recent.to_owned(),
        }
    }
}

impl RateFeed for RecordedFeed {
    fn historical_series(&self) -> Result<String, FeedError> {
        Ok(self.historical.clone())
    }

    fn recent_window(&self) -> Result<String, FeedError> {
        Ok(self.recent.clone())
    }
}

/// A feed that cannot be reached, which is the state an offline machine is in.
struct UnreachableFeed;

impl RateFeed for UnreachableFeed {
    fn historical_series(&self) -> Result<String, FeedError> {
        Err(FeedError::Unreadable {
            reason: "no network".to_owned(),
        })
    }

    fn recent_window(&self) -> Result<String, FeedError> {
        self.historical_series()
    }
}

fn fragment(name: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "fixtures",
        "ecb",
        name,
    ]
    .iter()
    .collect();
    fs::read_to_string(&path).unwrap_or_else(|_| panic!("read the fixture {}", path.display()))
}

fn day(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("a valid date")
}

async fn open() -> (TempDb, Database) {
    let db = TempDb::new();
    let database = Database::open(db.path())
        .await
        .expect("open the temporary database");
    (db, database)
}

/// Seeding ingests the historical series into a table keyed by currency and date [ARC-015],
/// [ARC-017], [TST-004].
#[tokio::test]
async fn seeding_ingests_the_recorded_historical_series() {
    let (_db, database) = open().await;

    let written = seed(&database, &RecordedFeed::new())
        .await
        .expect("seed from the recorded fragment");

    assert_eq!(written, 4, "two currencies on each of the fragment's days");

    let cached = database.rates().snapshot().await.expect("read back");
    assert_eq!(cached.len(), 4);

    // The series' first publication, and a currency the other day does not carry: the key is
    // the pair, so neither shadows the other.
    assert_eq!(
        cached
            .latest_on_or_before(&Currency::new("JPY"), day(1999, 1, 4))
            .expect("the first publication is cached")
            .rate,
        FxRate::new(dec!(133.73))
    );
    assert_eq!(
        cached
            .latest_on_or_before(&Currency::new("CAD"), day(2024, 3, 28))
            .expect("the Thursday publication is cached")
            .rate,
        FxRate::new(dec!(1.4645))
    );
    assert!(
        cached
            .latest_on_or_before(&Currency::new("CAD"), day(1999, 1, 4))
            .is_none(),
        "a currency's lookup never reaches a day it was not published on"
    );
}

/// Seeding is done once; running it again writes nothing and disturbs nothing [ARC-017].
#[tokio::test]
async fn seeding_a_cache_that_is_already_seeded_writes_nothing() {
    let (_db, database) = open().await;
    let feed = RecordedFeed::new();

    seed(&database, &feed).await.expect("first seeding");
    let again = seed(&database, &feed).await.expect("second seeding");

    assert_eq!(again, 0, "a published rate for a day is final");
    assert_eq!(
        database.rates().snapshot().await.expect("read back").len(),
        4
    );
}

/// The 90-day window adds the days published since and leaves the seeded ones alone [ARC-018].
#[tokio::test]
async fn topping_up_adds_only_the_days_the_cache_does_not_hold() {
    let (_db, database) = open().await;
    let feed = RecordedFeed::new();

    seed(&database, &feed).await.expect("seeding");
    let added = top_up(&database, &feed).await.expect("top up");

    assert_eq!(
        added, 1,
        "the window restates 2024-03-28, which is already cached, and adds 2024-04-02 alone"
    );

    let cached = database.rates().snapshot().await.expect("read back");
    assert_eq!(cached.len(), 5);
    assert_eq!(
        cached
            .latest_on_or_before(&Currency::new("USD"), day(2024, 4, 2))
            .expect("the Tuesday publication arrived with the top-up")
            .rate,
        FxRate::new(dec!(1.0749))
    );
}

/// A window restating a cached day at a different rate does not rewrite it [ARC-018]. The
/// committed fragment restates 2024-03-28 at the very rate the series deposited, so only a
/// hand-built window can tell "keep" from "overwrite": a stored conversion was valued at the
/// cached rate and must stay reconcilable against it.
#[tokio::test]
async fn topping_up_leaves_a_cached_rate_as_it_was() {
    let (_db, database) = open().await;
    let restated = r#"<?xml version="1.0" encoding="UTF-8"?>
<gesmes:Envelope xmlns:gesmes="http://www.gesmes.org/xml/2002-08-01">
    <Cube><Cube time="2024-03-28"><Cube currency="USD" rate="9.9999"/></Cube></Cube>
</gesmes:Envelope>"#;
    let feed = RecordedFeed::of(&fragment("eurofxref-hist-fragment.xml"), restated);

    seed(&database, &feed).await.expect("seeding");
    let added = top_up(&database, &feed).await.expect("top up");

    assert_eq!(added, 0, "the window states no day the cache does not hold");
    assert_eq!(
        database
            .rates()
            .snapshot()
            .await
            .expect("read back")
            .latest_on_or_before(&Currency::new("USD"), day(2024, 3, 28))
            .expect("the seeded publication is still cached")
            .rate,
        FxRate::new(dec!(1.0811)),
        "the seeded rate stands, not the restated one"
    );
}

/// Topping up a cache that was never seeded writes the whole window, which is the ordinary state
/// of a database created before seeding completes; a second top-up adds nothing [ARC-018].
#[tokio::test]
async fn topping_up_an_unseeded_cache_writes_the_whole_window() {
    let (_db, database) = open().await;
    let feed = RecordedFeed::new();

    let added = top_up(&database, &feed).await.expect("top up");

    assert_eq!(added, 3, "one rate on 2024-04-02 and two on 2024-03-28");

    let cached = database.rates().snapshot().await.expect("read back");
    assert_eq!(cached.len(), 3);
    assert_eq!(
        cached
            .latest_on_or_before(&Currency::new("CAD"), day(2024, 3, 28))
            .expect("the window's older day is cached too")
            .rate,
        FxRate::new(dec!(1.4645))
    );

    assert_eq!(
        top_up(&database, &feed).await.expect("second top up"),
        0,
        "the window states no day the cache does not already hold"
    );
}

/// The reverse ordering: a cache filled from the 90-day window first and seeded afterwards, which
/// is the state of a machine that imported before seeding completed [ARC-017], [ARC-018]. It is
/// the one ordering in which seeding must add rows *older* than what is cached while leaving the
/// day the two documents share alone. The count carries that claim: both fragments state
/// 2024-03-28 at the same rates, so the rate read back cannot tell "kept" from "rewritten".
#[tokio::test]
async fn seeding_after_a_top_up_adds_the_older_days_alone() {
    let (_db, database) = open().await;
    let feed = RecordedFeed::new();

    top_up(&database, &feed).await.expect("top up first");
    let seeded = seed(&database, &feed).await.expect("seed afterwards");

    assert_eq!(
        seeded, 2,
        "the series adds its two 1999 rows and restates 2024-03-28, which the window deposited"
    );

    let cached = database.rates().snapshot().await.expect("read back");
    assert_eq!(cached.len(), 5);
    assert_eq!(
        cached
            .latest_on_or_before(&Currency::new("USD"), day(2024, 3, 28))
            .expect("the day the two documents share")
            .rate,
        FxRate::new(dec!(1.0811))
    );
    assert_eq!(
        cached
            .latest_on_or_before(&Currency::new("JPY"), day(1999, 1, 4))
            .expect("the series reaches back before the window")
            .rate,
        FxRate::new(dec!(133.73))
    );
}

/// Once seeded, an import resolves rates from the cache alone [ARC-016], including across the
/// Easter publication gap the fragment reproduces [DOM-034]. Nothing in the resolution path
/// takes a feed, so nothing in it can reach a socket [TST-019].
#[tokio::test]
async fn once_seeded_a_resolution_reads_the_cache_and_nothing_else() {
    let (_db, database) = open().await;
    seed(&database, &RecordedFeed::new())
        .await
        .expect("seeding");

    let cached = database.rates().snapshot().await.expect("read back");
    database.close().await;

    // Saturday 30 March 2024: the ECB published nothing, so the Thursday rate stands in and
    // carries its own date.
    let conversion = resolve(
        &cached,
        &Currency::new("USD"),
        day(2024, 3, 30),
        Stated::NativeOnly,
    )
    .expect("the cache answers offline");

    assert_eq!(conversion.source(), RateSource::Ecb);
    assert_eq!(conversion.rate(), FxRate::new(dec!(1.0811)));
    assert_eq!(conversion.rate_date(), day(2024, 3, 28));
}

/// A currency the cache does not hold fails with FIF-009's error, unchanged: this item adds no
/// second one [ARC-019].
#[tokio::test]
async fn a_currency_the_cache_does_not_hold_fails_with_the_resolver_s_own_error() {
    let (_db, database) = open().await;
    seed(&database, &RecordedFeed::new())
        .await
        .expect("seeding");
    let cached = database.rates().snapshot().await.expect("read back");

    let error = resolve(
        &cached,
        &Currency::new("CHF"),
        day(2024, 3, 28),
        Stated::NativeOnly,
    )
    .expect_err("the fragment holds no CHF");

    let message = error.to_string();
    assert!(message.contains("CHF"), "{message} names no currency");
    assert!(message.contains("2024-03-28"), "{message} names no date");
}

/// A rate carrying more decimals than the column holds is refused rather than truncated
/// [ARC-007], [ARC-010].
#[tokio::test]
async fn a_rate_beyond_the_stored_scale_is_refused() {
    let (_db, database) = open().await;
    // The good row precedes the refused one deliberately: ingestion is one transaction, so a
    // store that committed per row would leave 2024-03-28 CAD behind when the next row fails.
    let feed = RecordedFeed::of(
        r#"<gesmes:Envelope xmlns:gesmes="http://www.gesmes.org/xml/2002-08-01">
             <Cube><Cube time="2024-03-28">
               <Cube currency="CAD" rate="1.4645"/>
               <Cube currency="USD" rate="1.0811115"/>
             </Cube></Cube>
           </gesmes:Envelope>"#,
        "",
    );

    let refused = seed(&database, &feed).await;

    match refused {
        Err(IngestError::Storage(StorageError::UnscaledValue { field, scale, .. })) => {
            assert_eq!(field, "rate");
            assert_eq!(scale, 6, "an FX rate is stored at six decimals");
        }
        other => panic!("a seven-decimal rate must be refused, got {other:?}"),
    }

    assert!(
        database
            .rates()
            .snapshot()
            .await
            .expect("read back")
            .is_empty(),
        "the refused document leaves the cache empty, the row it stated first included"
    );
}

/// A document that is not the envelope leaves the cache as it was, rather than half filled — and
/// "as it was" is a seeded cache here, not an empty one: a top-up that fails over a seeded cache
/// must leave the series intact, which an implementation that cleared the table would also pass
/// were the cache empty [ARC-017].
#[tokio::test]
async fn a_malformed_document_writes_nothing() {
    let (_db, database) = open().await;
    let malformed = RecordedFeed::of(
        "<gesmes:Envelope><Cube time=\"2024-03-28\">",
        "<gesmes:Envelope><Cube time=\"2024-03-28\">",
    );

    assert!(matches!(
        seed(&database, &malformed).await,
        Err(IngestError::Feed(FeedError::Malformed { .. }))
    ));
    assert!(
        database
            .rates()
            .snapshot()
            .await
            .expect("read back")
            .is_empty()
    );

    seed(&database, &RecordedFeed::new())
        .await
        .expect("seeding from the recorded fragment");

    assert!(matches!(
        top_up(&database, &malformed).await,
        Err(IngestError::Feed(FeedError::Malformed { .. }))
    ));

    let cached = database.rates().snapshot().await.expect("read back");
    assert_eq!(
        cached.len(),
        4,
        "the seeded series survives a failed top-up"
    );
    assert_eq!(
        cached
            .latest_on_or_before(&Currency::new("JPY"), day(1999, 1, 4))
            .expect("the seeded rows are still readable")
            .rate,
        FxRate::new(dec!(133.73))
    );
}

/// An empty response body is refused rather than read as a series holding no day: an `Ok(0)`
/// there would report a cache that stayed empty as a seeding that succeeded, which is the state
/// ARC-017 exists to prevent.
#[tokio::test]
async fn an_empty_document_is_refused_rather_than_seeding_nothing() {
    let (_db, database) = open().await;

    assert!(matches!(
        seed(&database, &RecordedFeed::of("", "")).await,
        Err(IngestError::Feed(FeedError::Malformed { .. }))
    ));
    assert!(
        database
            .rates()
            .snapshot()
            .await
            .expect("read back")
            .is_empty()
    );
}

/// A feed that cannot be reached is reported as such, not as an empty series: a cache that
/// silently stayed empty would fail later, at an import, naming a currency instead [ARC-019].
#[tokio::test]
async fn an_unreachable_feed_is_reported_rather_than_read_as_empty() {
    let (_db, database) = open().await;

    assert!(matches!(
        seed(&database, &UnreachableFeed).await,
        Err(IngestError::Feed(FeedError::Unreadable { .. }))
    ));
    assert!(matches!(
        top_up(&database, &UnreachableFeed).await,
        Err(IngestError::Feed(FeedError::Unreadable { .. }))
    ));
}

/// The rows the repository stores are the observations the document states, whichever path put
/// them there [ARC-015].
#[tokio::test]
async fn the_repository_stores_the_observations_it_is_given() {
    let (db, database) = open().await;

    assert_eq!(
        database.rates().store(&[]).await.expect("store nothing"),
        0,
        "an empty batch opens and commits a transaction and writes no row"
    );

    let written = database
        .rates()
        .store(&[Observation {
            currency: Currency::new("usd"),
            date: day(2024, 4, 2),
            rate: FxRate::new(dec!(1.0749)),
        }])
        .await
        .expect("store one observation");

    assert_eq!(written, 1);

    // A document restating a day within itself: the conflict clause reads a row its own
    // transaction wrote, so the second of the pair is kept out and the first stands.
    let colliding = database
        .rates()
        .store(&[
            Observation {
                currency: Currency::new("CAD"),
                date: day(2024, 3, 28),
                rate: FxRate::new(dec!(1.4645)),
            },
            Observation {
                currency: Currency::new("CAD"),
                date: day(2024, 3, 28),
                rate: FxRate::new(dec!(9.9999)),
            },
        ])
        .await
        .expect("store a colliding pair");

    assert_eq!(colliding, 1, "the pair shares a currency and a date");

    assert_eq!(
        database
            .rates()
            .snapshot()
            .await
            .expect("read back")
            .latest_on_or_before(&Currency::new("USD"), day(2024, 4, 2))
            .expect("the currency is stored as the upper-case code it is")
            .rate,
        FxRate::new(dec!(1.0749))
    );

    // On the column rather than on the decimal read back: `rust_decimal` compares across scales,
    // so `dec!(1.0749) == dec!(1.074900)` and a decimal assertion cannot see the padding
    // `codec::at_scale` applies [ARC-007].
    let pool = SqlitePool::connect(&format!("sqlite://{}", db.path().display()))
        .await
        .expect("connect to the temporary database");
    let stored: String = query("select rate from fx_rate where currency = 'USD'")
        .fetch_one(&pool)
        .await
        .expect("read the column")
        .get("rate");
    pool.close().await;

    assert_eq!(
        stored, "1.074900",
        "stored at the column's scale, padded as every other decimal is"
    );
    assert_eq!(
        database
            .rates()
            .snapshot()
            .await
            .expect("read back")
            .latest_on_or_before(&Currency::new("CAD"), day(2024, 3, 28))
            .expect("the first of the colliding pair is stored")
            .rate,
        FxRate::new(dec!(1.464500))
    );
}
