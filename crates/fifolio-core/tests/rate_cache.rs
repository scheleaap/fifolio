//! Integration layer [TST-003]: the ECB rate cache against a real temporary SQLite database,
//! seeded from the committed recorded fragments of the series [ARC-015 to ARC-018], [TST-004].
//!
//! The feed is the injected [`RateFeed`] port and every test supplies it from a file on disk
//! [TST-020], so nothing here opens a socket [TST-019]. There is no code path in this crate
//! that could: the fetch happens outside it [ARC-002].

use std::cell::Cell;
use std::fs;
use std::path::PathBuf;

use chrono::NaiveDate;
use fifolio_core::decimal::FxRate;
use fifolio_core::ecb::{FeedError, IngestError, Observation, RateFeed, rates_for, seed, top_up};
use fifolio_core::fx::{RateError, RateTable, Stated, resolve};
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

/// A second connection to the same file, for the assertions the port cannot make.
async fn raw(db: &TempDb) -> SqlitePool {
    SqlitePool::connect(&format!("sqlite://{}", db.path().display()))
        .await
        .expect("connect to the temporary database")
}

/// How many rows the cache holds, counted on the table. A row count is not part of the port
/// `fx::resolve` reads [ARC-016], so it is asked of the database rather than of the snapshot.
async fn cached_rows(db: &TempDb) -> i64 {
    let pool = raw(db).await;
    let count: i64 = query("select count(*) from fx_rate")
        .fetch_one(&pool)
        .await
        .expect("count the cached rows")
        .get(0);
    pool.close().await;
    count
}

/// Seeding ingests the historical series into a table keyed by currency and date [ARC-015],
/// [ARC-017], [TST-004].
#[tokio::test]
async fn seeding_ingests_the_recorded_historical_series() {
    let (db, database) = open().await;

    let written = seed(&database, &RecordedFeed::new())
        .await
        .expect("seed from the recorded fragment");

    assert_eq!(written, 4, "two currencies on each of the fragment's days");

    let cached = database.rates().snapshot().await.expect("read back");
    assert_eq!(cached_rows(&db).await, 4);

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
    let (db, database) = open().await;
    let feed = RecordedFeed::new();

    seed(&database, &feed).await.expect("first seeding");
    let again = seed(&database, &feed).await.expect("second seeding");

    assert_eq!(again, 0, "a published rate for a day is final");
    assert_eq!(cached_rows(&db).await, 4);
}

/// The 90-day window adds the days published since and leaves the seeded ones alone [ARC-018].
#[tokio::test]
async fn topping_up_adds_only_the_days_the_cache_does_not_hold() {
    let (db, database) = open().await;
    let feed = RecordedFeed::new();

    seed(&database, &feed).await.expect("seeding");
    let added = top_up(&database, &feed).await.expect("top up");

    assert_eq!(
        added, 1,
        "the window restates 2024-03-28, which is already cached, and adds 2024-04-02 alone"
    );

    let cached = database.rates().snapshot().await.expect("read back");
    assert_eq!(cached_rows(&db).await, 5);
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
    let (db, database) = open().await;
    let feed = RecordedFeed::new();

    let added = top_up(&database, &feed).await.expect("top up");

    assert_eq!(added, 3, "one rate on 2024-04-02 and two on 2024-03-28");

    let cached = database.rates().snapshot().await.expect("read back");
    assert_eq!(cached_rows(&db).await, 3);
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
    let (db, database) = open().await;
    let feed = RecordedFeed::new();

    top_up(&database, &feed).await.expect("top up first");
    let seeded = seed(&database, &feed).await.expect("seed afterwards");

    assert_eq!(
        seeded, 2,
        "the series adds its two 1999 rows and restates 2024-03-28, which the window deposited"
    );

    let cached = database.rates().snapshot().await.expect("read back");
    assert_eq!(cached_rows(&db).await, 5);
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
/// Easter publication gap the fragments reproduce [DOM-034]. Nothing in the resolution path
/// takes a feed, so nothing in it can reach a socket [TST-019].
#[tokio::test]
async fn once_seeded_a_resolution_reads_the_cache_and_nothing_else() {
    let (_db, database) = open().await;
    let feed = RecordedFeed::new();
    seed(&database, &feed).await.expect("seeding");
    // The historical fragment ends on the Thursday; the window's Tuesday is the later
    // publication that proves the Saturday was skipped rather than not yet published [DEC-085].
    top_up(&database, &feed).await.expect("top up");

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
    let (db, database) = open().await;
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

    assert_eq!(
        cached_rows(&db).await,
        0,
        "the refused document leaves the cache empty, the row it stated first included"
    );
}

/// And the boundary itself: six decimals is exactly what the column holds, so it stores and reads
/// back unchanged. Asserted beside the refusal above so that an off-by-one bound — `>=` where `>`
/// was meant — cannot pass from the ECB path's side [ARC-007], [ARC-010].
///
/// `0.000001` is the other boundary, the smallest positive figure the column holds: it sits just
/// above the parser's positivity refusal, and a rate that reached the column as zero would be a
/// divisor of zero at valuation time [ARC-027], one unvaluable leg much later.
#[tokio::test]
async fn a_rate_at_the_stored_scale_is_kept_unchanged() {
    for rate in [dec!(1.081111), dec!(0.000001)] {
        let (_db, database) = open().await;
        let feed = RecordedFeed::of(
            &format!(
                r#"<gesmes:Envelope xmlns:gesmes="http://www.gesmes.org/xml/2002-08-01">
                     <Cube><Cube time="2024-03-28">
                       <Cube currency="USD" rate="{rate}"/>
                     </Cube></Cube>
                   </gesmes:Envelope>"#
            ),
            "",
        );

        assert_eq!(seed(&database, &feed).await.expect("a six-decimal rate"), 1);

        let cached = database.rates().snapshot().await.expect("read back");
        assert_eq!(
            cached
                .latest_on_or_before(&Currency::new("USD"), day(2024, 3, 28))
                .expect("the six-decimal rate is cached")
                .rate,
            FxRate::new(rate)
        );
    }
}

/// A `rate` column a hand edit left holding something that is not a decimal is reported, not
/// panicked on and not dropped from the snapshot: a silently missing row reads as a day the ECB
/// did not publish, which is the state ARC-015 keeps the cache out of.
#[tokio::test]
async fn a_stored_rate_that_is_not_a_decimal_is_reported() {
    let (db, database) = open().await;
    let pool = raw(&db).await;
    query("insert into fx_rate (currency, rate_date, rate) values (?, ?, ?)")
        .bind("USD")
        .bind("2024-03-28")
        .bind("not a rate")
        .execute(&pool)
        .await
        .expect("write a corrupt row");
    pool.close().await;

    match database.rates().snapshot().await {
        Err(StorageError::CorruptValue { field, .. }) => assert_eq!(field, "rate"),
        other => panic!("a corrupt rate column must be reported, got {other:?}"),
    }
}

/// A document that is not the envelope leaves the cache as it was, rather than half filled — and
/// "as it was" is a seeded cache here, not an empty one: a top-up that fails over a seeded cache
/// must leave the series intact, which an implementation that cleared the table would also pass
/// were the cache empty [ARC-017].
#[tokio::test]
async fn a_malformed_document_writes_nothing() {
    let (db, database) = open().await;
    // An empty body on the seeding side and a truncated document on the top-up side: an empty
    // response is the one a failed download most often leaves behind, and reading it as a series
    // holding no day would report a cache that stayed empty as a seeding that succeeded.
    let malformed = RecordedFeed::of("", "<gesmes:Envelope><Cube time=\"2024-03-28\">");

    assert!(matches!(
        seed(&database, &malformed).await,
        Err(IngestError::Feed(FeedError::Malformed { .. }))
    ));
    assert_eq!(cached_rows(&db).await, 0);

    seed(&database, &RecordedFeed::new())
        .await
        .expect("seeding from the recorded fragment");

    assert!(matches!(
        top_up(&database, &malformed).await,
        Err(IngestError::Feed(FeedError::Malformed { .. }))
    ));

    let cached = database.rates().snapshot().await.expect("read back");
    assert_eq!(
        cached_rows(&db).await,
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

// ---------------------------------------------------------------------------------------------
// Fetching a rate an import does not have [ARC-019], through the same recorded feed [TST-019 to
// TST-021].
// ---------------------------------------------------------------------------------------------

/// Which documents a fetch asked the feed for, so a test can tell "fetched" from "already held".
#[derive(Default)]
struct Requests {
    historical: Cell<usize>,
    recent: Cell<usize>,
}

/// A feed that records every request before answering it as `inner` does.
struct CountingFeed<F> {
    inner: F,
    requests: Requests,
}

impl<F: RateFeed> CountingFeed<F> {
    fn new(inner: F) -> Self {
        Self {
            inner,
            requests: Requests::default(),
        }
    }

    fn requested(&self) -> (usize, usize) {
        (self.requests.historical.get(), self.requests.recent.get())
    }
}

impl<F: RateFeed> RateFeed for CountingFeed<F> {
    fn historical_series(&self) -> Result<String, FeedError> {
        self.requests
            .historical
            .set(self.requests.historical.get() + 1);
        self.inner.historical_series()
    }

    fn recent_window(&self) -> Result<String, FeedError> {
        self.requests.recent.set(self.requests.recent.get() + 1);
        self.inner.recent_window()
    }
}

/// A USD-only envelope, for the windows and series the committed fragments cannot state.
fn envelope(publications: &[(NaiveDate, &str)]) -> String {
    let days: String = publications
        .iter()
        .map(|(date, usd)| {
            format!(r#"<Cube time="{date}"><Cube currency="USD" rate="{usd}"/></Cube>"#)
        })
        .collect();
    format!(
        r#"<gesmes:Envelope xmlns:gesmes="http://www.gesmes.org/xml/2002-08-01"><Cube>{days}</Cube></gesmes:Envelope>"#
    )
}

fn usd_on(table: &impl RateTable, date: NaiveDate) -> Result<(FxRate, NaiveDate), RateError> {
    resolve(table, &Currency::new("USD"), date, Stated::NativeOnly)
        .map(|conversion| (conversion.rate(), conversion.rate_date()))
}

/// An empty cache is seeded from the full series by the import that first needs a rate, and the
/// 90-day window is not asked for [ARC-019], [SRV-047].
#[tokio::test]
async fn an_import_seeds_an_empty_cache_from_the_full_series() {
    let (db, database) = open().await;
    let feed = CountingFeed::new(RecordedFeed::new());

    let rates = rates_for(&database, &feed, [day(2024, 3, 28)])
        .await
        .expect("fetch");

    assert_eq!(feed.requested(), (1, 0));
    assert_eq!(cached_rows(&db).await, 4, "the whole recorded series");
    assert_eq!(
        usd_on(&rates, day(2024, 3, 28)),
        Ok((FxRate::new(dec!(1.0811)), day(2024, 3, 28)))
    );
}

/// A day after the cache is topped up from the 90-day window; the window reaches back into the
/// cache, so the full series is not asked for again [ARC-019], [ARC-018].
#[tokio::test]
async fn an_import_tops_up_a_cache_that_ends_before_its_date() {
    let (_db, database) = open().await;
    seed(&database, &RecordedFeed::new())
        .await
        .expect("seeding");
    let feed = CountingFeed::new(RecordedFeed::new());

    let rates = rates_for(&database, &feed, [day(2024, 4, 2)])
        .await
        .expect("fetch");

    assert_eq!(feed.requested(), (0, 1));
    assert_eq!(
        usd_on(&rates, day(2024, 4, 2)),
        Ok((FxRate::new(dec!(1.0749)), day(2024, 4, 2)))
    );
}

/// A day the cache covers is not fetched, even one it holds no usable publication for: inside the
/// span a missing day is one the ECB did not publish, and resolution decides what stands in
/// [ARC-016], [ARC-027].
#[tokio::test]
async fn an_import_fetches_nothing_for_a_day_the_cache_covers() {
    let (_db, database) = open().await;
    seed(&database, &RecordedFeed::new())
        .await
        .expect("seeding");
    let feed = CountingFeed::new(UnreachableFeed);

    // 1999-01-20 lies inside the cached span, between its two publications and more
    // than a week after the first: covered, and therefore a stale substitute, not a fetch.
    let rates = rates_for(&database, &feed, [day(2024, 3, 28), day(1999, 1, 20)])
        .await
        .expect("no fetch, so nothing to fail");

    assert_eq!(feed.requested(), (0, 0));
    assert_eq!(
        usd_on(&rates, day(2024, 3, 28)),
        Ok((FxRate::new(dec!(1.0811)), day(2024, 3, 28)))
    );
    assert!(matches!(
        usd_on(&rates, day(1999, 1, 20)),
        Err(RateError::StaleSubstitute { .. })
    ));
}

/// A date the window no longer covers is filled from the full series: here the cache ends in
/// 1999 and the window begins after the trade date [ARC-019].
#[tokio::test]
async fn an_import_reaches_the_full_series_for_a_date_older_than_the_window() {
    let (_db, database) = open().await;
    database
        .rates()
        .store(&[Observation {
            currency: Currency::new("USD"),
            date: day(1999, 1, 4),
            rate: FxRate::new(dec!(1.1789)),
        }])
        .await
        .expect("a cache ending in 1999");
    let feed = CountingFeed::new(RecordedFeed::of(
        &fragment("eurofxref-hist-fragment.xml"),
        &envelope(&[(day(2024, 4, 2), "1.0749")]),
    ));

    let rates = rates_for(&database, &feed, [day(2024, 3, 28)])
        .await
        .expect("fetch");

    assert_eq!(feed.requested(), (1, 1));
    assert_eq!(
        usd_on(&rates, day(2024, 3, 28)),
        Ok((FxRate::new(dec!(1.0811)), day(2024, 3, 28)))
    );
}

/// The window starting after the cache ends leaves a hole even when the trade date is inside the
/// window, and the full series fills it: otherwise a later trade in the hole would resolve to a
/// cached rate from before it as though the ECB had not published [ARC-019], [DOM-034].
#[tokio::test]
async fn a_window_that_does_not_reach_the_cache_brings_the_full_series_with_it() {
    let (_db, database) = open().await;
    database
        .rates()
        .store(&[Observation {
            currency: Currency::new("USD"),
            date: day(2024, 3, 26),
            rate: FxRate::new(dec!(1.0833)),
        }])
        .await
        .expect("a cache ending two days before the recorded Thursday");
    let feed = CountingFeed::new(RecordedFeed::of(
        &fragment("eurofxref-hist-fragment.xml"),
        &envelope(&[(day(2024, 4, 2), "1.0749")]),
    ));

    rates_for(&database, &feed, [day(2024, 4, 2)])
        .await
        .expect("fetch");
    let rates = rates_for(&database, &UnreachableFeed, [day(2024, 3, 28)])
        .await
        .expect("covered now, so no fetch");

    assert_eq!(feed.requested(), (1, 1));
    assert_eq!(
        usd_on(&rates, day(2024, 3, 28)),
        Ok((FxRate::new(dec!(1.0811)), day(2024, 3, 28))),
        "the hole's publication, not the 2024-03-26 rate standing in for it"
    );
}

/// A date before the cache begins, in a cache filled from the window alone, is filled from the
/// full series [ARC-019].
#[tokio::test]
async fn an_import_reaches_the_full_series_for_a_date_before_the_cache() {
    let (_db, database) = open().await;
    top_up(&database, &RecordedFeed::new())
        .await
        .expect("a cache holding the window alone");
    let feed = CountingFeed::new(RecordedFeed::new());

    let rates = rates_for(&database, &feed, [day(1999, 1, 4)])
        .await
        .expect("fetch");

    assert_eq!(feed.requested(), (1, 0));
    assert_eq!(
        usd_on(&rates, day(1999, 1, 4)),
        Ok((FxRate::new(dec!(1.1789)), day(1999, 1, 4)))
    );
}

/// The fetch fills the cache and never rewrites a cached day, even when the window restates it
/// at another figure [ARC-028].
#[tokio::test]
async fn an_import_s_fetch_never_rewrites_a_cached_day() {
    let (_db, database) = open().await;
    seed(&database, &RecordedFeed::new())
        .await
        .expect("seeding");
    let feed = RecordedFeed::of(
        &fragment("eurofxref-hist-fragment.xml"),
        &envelope(&[(day(2024, 4, 2), "1.0749"), (day(2024, 3, 28), "9.9999")]),
    );

    let rates = rates_for(&database, &feed, [day(2024, 4, 2)])
        .await
        .expect("fetch");

    assert_eq!(
        usd_on(&rates, day(2024, 3, 28)),
        Ok((FxRate::new(dec!(1.0811)), day(2024, 3, 28))),
        "the seeded rate stands, not the restated one"
    );
    assert_eq!(
        usd_on(&rates, day(2024, 4, 2)),
        Ok((FxRate::new(dec!(1.0749)), day(2024, 4, 2)))
    );
}

/// Offline with an empty cache, the import fails with FIF-009's own error naming the currency and
/// the date; the feed's failure is not a second error [ARC-019], [DEC-078].
#[tokio::test]
async fn offline_an_empty_cache_fails_with_the_resolver_s_own_error() {
    let (_db, database) = open().await;

    let rates = rates_for(&database, &UnreachableFeed, [day(2024, 3, 28)])
        .await
        .expect("a feed failure is not an error of its own");

    let error = usd_on(&rates, day(2024, 3, 28)).expect_err("nothing cached, nothing fetched");
    assert_eq!(
        error,
        RateError::Unavailable {
            currency: Currency::new("USD"),
            date: day(2024, 3, 28),
        }
    );
    let message = error.to_string();
    assert!(message.contains("USD"), "{message} names no currency");
    assert!(message.contains("2024-03-28"), "{message} names no date");
}

/// Offline with a cache that ends five days before the trade, the last cached rate does not stand
/// in: whether the ECB published in between is unknown, so it is not a previous-publication
/// substitute but a guess [ARC-019], [DOM-034]. A malformed document is the same "still no
/// rate".
#[tokio::test]
async fn offline_a_date_past_the_cache_is_unavailable_rather_than_substituted() {
    let (_db, database) = open().await;
    seed(&database, &RecordedFeed::new())
        .await
        .expect("seeding");
    let malformed = RecordedFeed::of("", "<gesmes:Envelope><Cube time=\"2024-04-02\">");

    for (label, rates) in [
        (
            "unreachable",
            rates_for(&database, &UnreachableFeed, [day(2024, 4, 2)]).await,
        ),
        (
            "malformed",
            rates_for(&database, &malformed, [day(2024, 4, 2)]).await,
        ),
    ] {
        let rates = rates.expect("a feed failure is not an error of its own");
        assert_eq!(
            usd_on(&rates, day(2024, 4, 2)),
            Err(RateError::Unavailable {
                currency: Currency::new("USD"),
                date: day(2024, 4, 2),
            }),
            "{label}"
        );
        assert_eq!(
            usd_on(&rates, day(2024, 3, 28)),
            Ok((FxRate::new(dec!(1.0811)), day(2024, 3, 28))),
            "{label}: the cached days still answer"
        );
    }
}

/// A date before the series has nothing to fetch, and resolution refuses it itself [ARC-027].
#[tokio::test]
async fn a_date_before_the_series_fetches_nothing() {
    let (_db, database) = open().await;
    let feed = CountingFeed::new(RecordedFeed::new());

    let rates = rates_for(&database, &feed, [day(1998, 12, 31)])
        .await
        .expect("no fetch");

    assert_eq!(feed.requested(), (0, 0));
    assert!(matches!(
        usd_on(&rates, day(1998, 12, 31)),
        Err(RateError::BeforeSeries { .. })
    ));

    // The series' first day is inside it, so an empty cache is seeded for it.
    let (_db, database) = open().await;
    let feed = CountingFeed::new(RecordedFeed::new());
    rates_for(&database, &feed, [day(1999, 1, 1)])
        .await
        .expect("fetch");
    assert_eq!(feed.requested(), (1, 0));
}

/// One import with a date after the cache and one before it: the window tops up the later end
/// and the full series still fills the earlier one [ARC-019].
#[tokio::test]
async fn an_import_with_dates_on_both_sides_of_the_cache_fills_both() {
    let (_db, database) = open().await;
    // A window-only cache ending before 2024-04-02, whose start the recorded window reaches.
    top_up(
        &database,
        &RecordedFeed::of("", &envelope(&[(day(2024, 3, 28), "1.0811")])),
    )
    .await
    .expect("a cache holding one window day");
    let feed = CountingFeed::new(RecordedFeed::new());

    let rates = rates_for(&database, &feed, [day(2024, 4, 2), day(1999, 1, 4)])
        .await
        .expect("fetch");

    assert_eq!(feed.requested(), (1, 1));
    assert_eq!(
        usd_on(&rates, day(2024, 4, 2)),
        Ok((FxRate::new(dec!(1.0749)), day(2024, 4, 2)))
    );
    assert_eq!(
        usd_on(&rates, day(1999, 1, 4)),
        Ok((FxRate::new(dec!(1.1789)), day(1999, 1, 4)))
    );
}

/// A fetched rate beyond the stored scale is a storage failure and propagates, rather than being
/// set aside like a feed failure and surfacing later as a missing rate [ARC-019], [ARC-010].
#[tokio::test]
async fn a_fetched_rate_beyond_the_stored_scale_is_a_storage_error() {
    let (_db, database) = open().await;
    seed(&database, &RecordedFeed::new())
        .await
        .expect("seeding");
    let feed = RecordedFeed::of(
        &fragment("eurofxref-hist-fragment.xml"),
        &envelope(&[(day(2024, 4, 2), "1.0749115"), (day(2024, 3, 28), "1.0811")]),
    );

    let refused = rates_for(&database, &feed, [day(2024, 4, 2)]).await;

    assert!(
        matches!(refused, Err(StorageError::UnscaledValue { .. })),
        "got {refused:?}"
    );
}

/// A window that does not reach the cache is not stored when the full series then cannot be
/// read: stored alone, it would leave a hole inside the cache's span that a later import reads as
/// days the ECB skipped, and a trade in it would resolve to a rate from before the hole
/// [ARC-019], [DOM-034], [DEC-085].
#[tokio::test]
async fn a_window_that_does_not_reach_the_cache_is_not_stored_without_the_full_series() {
    let (_db, database) = open().await;
    database
        .rates()
        .store(&[Observation {
            currency: Currency::new("USD"),
            date: day(2024, 3, 26),
            rate: FxRate::new(dec!(1.0833)),
        }])
        .await
        .expect("a cache ending two days before the recorded Thursday");
    let series_unreadable = RecordedFeed::of("", &envelope(&[(day(2024, 4, 2), "1.0749")]));

    rates_for(&database, &series_unreadable, [day(2024, 4, 2)])
        .await
        .expect("a feed failure is not an error of its own");
    let rates = rates_for(&database, &UnreachableFeed, [day(2024, 3, 28)])
        .await
        .expect("offline");

    assert_eq!(
        usd_on(&rates, day(2024, 3, 28)),
        Err(RateError::Unavailable {
            currency: Currency::new("USD"),
            date: day(2024, 3, 28),
        }),
        "not the 2024-03-26 rate standing in for a day the ECB published"
    );
    assert_eq!(
        usd_on(&rates, day(2024, 4, 2)),
        Err(RateError::Unavailable {
            currency: Currency::new("USD"),
            date: day(2024, 4, 2),
        })
    );
}
