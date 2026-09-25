//! The ECB euro reference-rate feed: its document, the injected fetcher, and the two paths that
//! fill the local cache [ARC-015 to ARC-018], which [`rates_for`] calls when an import needs a
//! rate the cache does not hold [ARC-019].
//!
//! # Two documents, one shape
//!
//! The ECB publishes its daily euro reference rates as a `gesmes` envelope holding nested
//! `Cube` elements: an outer one per publication day, carrying `time`, and an inner one per
//! currency, carrying `currency` and `rate`. The complete historical series back to 1999
//! [ARC-017] and the rolling 90-day window [ARC-018] are the same document in the same shape,
//! differing only in how many days they cover, so [`parse_reference_rates`] reads both and
//! [`seed`] and [`top_up`] differ only in which one they ask the feed for.
//!
//! A rate is foreign units per EUR, which is the direction [`crate::fx`] resolves and stores
//! [ARC-027]; nothing here reinterprets it.
//!
//! # Why the fetcher is a port
//!
//! `fifolio-core` carries no HTTP [ARC-002], and no test may open a socket [TST-019]. So the
//! feed is the injected [`RateFeed`] trait, whose whole contract is to hand over the document
//! text; the tests supply a committed recorded fragment of the series and the process that
//! downloads lives outside this crate. The trait is deliberately synchronous: a caller that
//! fetches over the network resolves its own future first and answers with the document, which
//! keeps an executor out of a port whose test double is a string.
//!
//! # Ingestion is additive
//!
//! A published reference rate for a day is final, and the 90-day window overlaps whatever the
//! historical series already deposited, so both paths insert and leave an existing row alone.
//! Seeding twice therefore stores nothing the second time, and the count both functions return
//! is of rows actually written, not of rows read.
//!
//! Rates arrive at the scale the ECB publishes them at — five decimals at most — and reach
//! their column through `codec::at_scale` like every other decimal [ARC-007, ARC-010]. A feed
//! carrying more than the six decimals an [`FxRate`](crate::decimal::FxRate) holds is refused
//! rather than truncated: that is a document this crate does not understand, and guessing at it
//! would put an invented digit into a cost basis.

use chrono::NaiveDate;
use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use rust_decimal::Decimal;
use std::str::FromStr;
use thiserror::Error;

use crate::decimal::FxRate;
use crate::fx::series_start;
use crate::storage::{CachedRates, Database, StorageError};
use crate::valuation::Currency;

/// One rate as published: a currency, the day it was published for, and the rate itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub currency: Currency,
    pub date: NaiveDate,
    pub rate: FxRate,
}

/// Where the ECB documents come from [ARC-017, ARC-018].
///
/// Injected so that seeding is tested against a recorded fragment rather than a live fetch
/// [TST-019, TST-020]. Both methods answer with the document text; parsing is
/// [`parse_reference_rates`]'s and is not the implementer's business.
pub trait RateFeed {
    /// The complete historical series, every currency back to 1999 [ARC-017].
    ///
    /// # Errors
    ///
    /// [`FeedError::Unreadable`] when the implementation cannot obtain the document.
    fn historical_series(&self) -> Result<String, FeedError>;

    /// The rolling 90-day window, which is what keeps a seeded cache current [ARC-018].
    ///
    /// # Errors
    ///
    /// [`FeedError::Unreadable`] when the implementation cannot obtain the document.
    fn recent_window(&self) -> Result<String, FeedError>;
}

/// A document that could not be obtained, or could not be believed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FeedError {
    /// The implementation could not obtain the document at all.
    #[error("the ECB feed could not be read: {reason}")]
    Unreadable { reason: String },

    /// The document is not the envelope this parser understands, or holds a value that is not a
    /// date or a rate. Named rather than skipped: a silently dropped row is a rate that is
    /// missing at import time and cannot be told from a day the ECB did not publish.
    #[error("the ECB document is not a euro reference-rate envelope: {reason}")]
    Malformed { reason: String },
}

/// Seeding or topping up failed, on the feed's side or on storage's.
#[derive(Debug, Error)]
pub enum IngestError {
    #[error(transparent)]
    Feed(#[from] FeedError),
    /// Includes the scale refusal: a rate carrying more decimals than the column holds
    /// [ARC-010].
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Fills an empty cache from the complete historical series [ARC-017].
///
/// Returns how many rows were written. Rows already cached are left as they are, so running
/// this against a populated cache is harmless and returns a smaller number.
///
/// # Errors
///
/// [`IngestError`] when the feed cannot be read, the document is malformed, or storage refuses
/// a rate.
pub async fn seed(database: &Database, feed: &impl RateFeed) -> Result<usize, IngestError> {
    ingest(database, &feed.historical_series()?).await
}

/// Brings a seeded cache up to date from the rolling 90-day window [ARC-018].
///
/// Returns how many rows were written, which on a cache topped up yesterday is the one or two
/// publication days since.
///
/// # Errors
///
/// As [`seed`].
pub async fn top_up(database: &Database, feed: &impl RateFeed) -> Result<usize, IngestError> {
    ingest(database, &feed.recent_window()?).await
}

async fn ingest(database: &Database, document: &str) -> Result<usize, IngestError> {
    store(database, &parse_reference_rates(document)?).await
}

async fn store(database: &Database, observations: &[Observation]) -> Result<usize, IngestError> {
    Ok(database.rates().store(observations).await?)
}

/// The rate cache an import resolves against, after fetching what the cache does not hold
/// [ARC-019].
///
/// `trade_dates` are the days the import's legs need an ECB rate for: the legs
/// [`fx::resolve`](crate::fx::resolve) would send down its ECB path, which excludes EUR legs and
/// legs whose file books its own EUR figures. The currency does not enter into it, because
/// every document states every currency.
///
/// The cache holds a day when the day falls inside its [coverage](CachedRates::coverage). A
/// day outside it is fetched through the two FIF-010 paths and nothing else:
///
/// * an empty cache is [seeded](seed) from the full series [SRV-047];
/// * a day after the cache is [topped up](top_up) from the 90-day window [ARC-018], unless the
///   window begins after the cache ends. It then no longer covers the days in between, and the
///   full series is ingested instead: stored on its own, the window would leave a hole inside
///   the cache's span that every later import reads as days the ECB skipped [DOM-034];
/// * a day before the cache is seeded from the full series.
///
/// Both paths only insert, so no fetch here rewrites a cached day [ARC-028], and a day the
/// cache already covers is never fetched at all, which is what keeps a cached import offline
/// [ARC-016].
///
/// A feed that cannot be read, offline for instance, is not an error of its own: the cache is
/// returned without the day, and resolution then fails with
/// [`RateError::Unavailable`](crate::fx::RateError::Unavailable) naming the currency and the
/// date, the one error DEC-078 names for it. A day past the cache's newest publication is that
/// error too, not a substitute, because resolution refuses it on its own [DEC-085].
///
/// # Errors
///
/// [`StorageError`] when the database cannot be read or written, including a fetched rate
/// beyond the stored scale [ARC-010].
pub async fn rates_for(
    database: &Database,
    feed: &impl RateFeed,
    trade_dates: impl IntoIterator<Item = NaiveDate>,
) -> Result<CachedRates, StorageError> {
    // A date before 1999 has no rate to fetch; resolution refuses it on its own [ARC-027].
    let needed: Vec<NaiveDate> = trade_dates
        .into_iter()
        .filter(|date| *date >= series_start())
        .collect();

    if !needed.is_empty() {
        fill(database, feed, &needed).await?;
    }

    database.rates().snapshot().await
}

/// Fetches into the cache whatever `needed` falls outside of, per [`rates_for`].
async fn fill(
    database: &Database,
    feed: &impl RateFeed,
    needed: &[NaiveDate],
) -> Result<(), StorageError> {
    let Some(coverage) = database.rates().snapshot().await?.coverage() else {
        return fetched(seed(database, feed).await).map(drop);
    };

    // Read before anything is stored, so that a window which does not reach the cache is never
    // stored without the full series behind it.
    let window = if needed.iter().any(|date| date > coverage.end()) {
        fetched(
            feed.recent_window()
                .and_then(|document| parse_reference_rates(&document))
                .map_err(IngestError::from),
        )?
    } else {
        None
    };
    let reaches_cache = |observations: &[Observation]| {
        observations
            .iter()
            .map(|observation| observation.date)
            .min()
            .is_some_and(|start| start <= *coverage.end())
    };

    let before_cache = needed.iter().any(|date| date < coverage.start());
    match window {
        Some(observations) if reaches_cache(&observations) => {
            fetched(store(database, &observations).await)?;
            if before_cache {
                fetched(seed(database, feed).await)?;
            }
        }
        // The full series states every day the window does, so it replaces the window.
        Some(_) => {
            fetched(seed(database, feed).await)?;
        }
        None if before_cache => {
            fetched(seed(database, feed).await)?;
        }
        None => {}
    }
    Ok(())
}

/// A fetch's outcome with the feed's failure set aside: a day the feed could not supply is a
/// day the cache still lacks, which resolution reports [ARC-019]. Storage failing is not that,
/// and propagates.
fn fetched<T>(outcome: Result<T, IngestError>) -> Result<Option<T>, StorageError> {
    match outcome {
        Ok(value) => Ok(Some(value)),
        Err(IngestError::Feed(error)) => {
            // The reason is logged because the resolution error that follows names the rate
            // it lacks, not why the fetch failed.
            tracing::warn!(%error, "fetching ECB rates the cache does not hold failed");
            Ok(None)
        }
        Err(IngestError::Storage(error)) => Err(error),
    }
}

/// Every rate in a `gesmes` euro reference-rate envelope, in the order the document states them.
///
/// # Errors
///
/// [`FeedError::Malformed`] when the XML does not parse, the document states no envelope at all,
/// a `Cube` carries a rate without a day to attach it to or without a currency to attach it to, a
/// `Cube` names a currency without a rate, or a `time`, `currency` or `rate` attribute does not
/// hold what its name says.
pub fn parse_reference_rates(document: &str) -> Result<Vec<Observation>, FeedError> {
    let mut reader = Reader::from_str(document);
    let mut observations = Vec::new();
    // The day of the enclosing `Cube` carrying `time`, with the depth that cube was opened at:
    // the currency cubes nested under it have no date of their own, and the day stops applying
    // when that cube closes, so a currency cube that is its *sibling* is refused rather than
    // filed under the day before it. A rate under the wrong date is a wrong EUR conversion that
    // no later error surfaces, where a missing one fails loudly at import [ARC-015].
    let mut day: Option<(NaiveDate, usize)> = None;
    // Elements opened and not yet closed. A truncated download otherwise parses as whichever
    // days it happens to contain, and a cache seeded from half a document has a hole in it that
    // nothing later notices, so the document must end where it started.
    let mut depth: usize = 0;
    // Whether the document ever stated the envelope. An empty body, an error page or a JSON
    // fault answers every event loop with no `Cube` at all, and reading that as an empty series
    // would report a seeding that stored nothing as a success [ARC-017].
    let mut envelope = false;

    loop {
        // Whether the element can enclose others: an empty `Cube` has no children, so a `time`
        // it states dates nothing and is not remembered past the element itself.
        let (element, encloses) = match reader
            .read_event()
            .map_err(|error| malformed(&error.to_string()))?
        {
            Event::Start(element) => {
                depth += 1;
                (element, true)
            }
            Event::Empty(element) => (element, false),
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                day = day.filter(|(_, stated_at)| depth >= *stated_at);
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };

        envelope = envelope || element.local_name().as_ref() == "Envelope";

        if element.local_name().as_ref() != "Cube" {
            continue;
        }
        let stated = attribute(&element, "time")?
            .map(|time| {
                NaiveDate::parse_from_str(&time, "%Y-%m-%d")
                    .map_err(|_| malformed(&format!("{time:?} is not a publication date")))
            })
            .transpose()?;
        if let (Some(date), true) = (stated, encloses) {
            day = Some((date, depth));
        }

        if let Some(observation) = observation(&element, stated.or(day.map(|(date, _)| date)))? {
            observations.push(observation);
        }
    }

    if depth > 0 {
        return Err(malformed("the document ends inside an element"));
    }
    if !envelope {
        return Err(malformed("the document states no envelope"));
    }

    Ok(observations)
}

/// The observation a currency `Cube` states, or `None` when the element is the envelope's outer
/// cube or a day's cube rather than a rate.
///
/// # Errors
///
/// [`FeedError::Malformed`] when the cube carries one of `currency` and `rate` without the other,
/// when it states no day, or when the rate is not a positive number. Half an attribution is not a
/// day the ECB did not publish for, and reading it as one would seed the cache with a hole that
/// surfaces much later, as an unavailable rate at import time [ARC-017].
fn observation(
    element: &BytesStart<'_>,
    day: Option<NaiveDate>,
) -> Result<Option<Observation>, FeedError> {
    let (currency, rate) = match (attribute(element, "currency")?, attribute(element, "rate")?) {
        (Some(currency), Some(rate)) => (currency, rate),
        // The envelope's outer cube and each day's cube carry neither attribute.
        (None, None) => return Ok(None),
        (Some(currency), None) => {
            return Err(malformed(&format!("the {currency} cube states no rate")));
        }
        (None, Some(rate)) => {
            return Err(malformed(&format!("the rate {rate:?} names no currency")));
        }
    };

    let date = day.ok_or_else(|| malformed(&format!("a {currency} rate precedes any date")))?;
    // Plain decimal notation only. `Decimal::from_str` also reads exponent form, which the ECB
    // never publishes, and reading `1.1e2` as 110 would invent a magnitude two orders out of a
    // document this parser refuses on lesser grounds elsewhere.
    let parsed = Decimal::from_str(&rate)
        .ok()
        .filter(|_| !rate.contains(['e', 'E']))
        .ok_or_else(|| malformed(&format!("{rate:?} is not a {currency} rate")))?;
    // A rate is foreign units per EUR and a conversion divides by it [ARC-027], so zero and
    // negative are not rates. Refused here rather than stored: `fx::resolve` would hand such a
    // figure to a conversion that answers with nothing, which reads as an unvaluable leg with
    // no currency and date named, where a refused document names both [ARC-015].
    if parsed <= Decimal::ZERO {
        return Err(malformed(&format!(
            "the {currency} rate {rate:?} is not positive"
        )));
    }

    Ok(Some(Observation {
        currency: Currency::new(currency),
        date,
        rate: FxRate::new(parsed),
    }))
}

fn attribute(element: &BytesStart<'_>, name: &str) -> Result<Option<String>, FeedError> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|error| malformed(&error.to_string()))?;
        if attribute.key.local_name().as_ref() == name {
            return attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .map(|value| Some(value.into_owned()))
                .map_err(|error| malformed(&error.to_string()));
        }
    }
    Ok(None)
}

fn malformed(reason: &str) -> FeedError {
    FeedError::Malformed {
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rust_decimal_macros::dec;

    fn day(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("a valid date")
    }

    /// The envelope as the ECB publishes it, newest day first, with the namespaces and the
    /// header elements the real document carries. Nothing here opens a socket [TST-019].
    fn envelope(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<gesmes:Envelope xmlns:gesmes="http://www.gesmes.org/xml/2002-08-01" xmlns="http://www.ecb.int/vocabulary/2002-08-01/eurofxref">
    <gesmes:subject>Reference rates</gesmes:subject>
    <gesmes:Sender>
        <gesmes:name>European Central Bank</gesmes:name>
    </gesmes:Sender>
    <Cube>{body}</Cube>
</gesmes:Envelope>"#
        )
    }

    /// Every currency of every day is read, and each rate takes the day of the cube enclosing
    /// it [ARC-015, ARC-017].
    #[test]
    fn a_document_yields_one_observation_per_currency_per_day() {
        let document = envelope(
            r#"
        <Cube time="2024-04-02">
            <Cube currency="USD" rate="1.0749"/>
            <Cube currency="JPY" rate="162.75"/>
        </Cube>
        <Cube time="2024-03-28">
            <Cube currency="USD" rate="1.0811"/>
            <Cube currency="JPY" rate="163.51"/>
        </Cube>"#,
        );

        let observations = parse_reference_rates(&document).expect("a well-formed envelope");

        assert_eq!(
            observations,
            vec![
                Observation {
                    currency: Currency::new("USD"),
                    date: day(2024, 4, 2),
                    rate: FxRate::new(dec!(1.0749)),
                },
                Observation {
                    currency: Currency::new("JPY"),
                    date: day(2024, 4, 2),
                    rate: FxRate::new(dec!(162.75)),
                },
                Observation {
                    currency: Currency::new("USD"),
                    date: day(2024, 3, 28),
                    rate: FxRate::new(dec!(1.0811)),
                },
                Observation {
                    currency: Currency::new("JPY"),
                    date: day(2024, 3, 28),
                    rate: FxRate::new(dec!(163.51)),
                },
            ],
            "the outer cube carries no rate and the day's cube carries no currency"
        );
    }

    /// A day whose currency list is empty is a day with no rates, not an error: the dated cube
    /// is read, contributes nothing, and does not make the days around it unreadable [ARC-018].
    #[test]
    fn a_day_with_no_currency_yields_no_observation() {
        let document = envelope(
            r#"
        <Cube time="2024-04-02"/>
        <Cube time="2024-03-28"></Cube>"#,
        );

        assert_eq!(
            parse_reference_rates(&document).expect("a day with no rates is well formed"),
            vec![]
        );
    }

    /// An envelope holding no dated cube at all is an empty series, not an error [ARC-018].
    #[test]
    fn an_envelope_with_no_day_yields_nothing() {
        assert_eq!(
            parse_reference_rates(&envelope("")).expect("an empty envelope is well formed"),
            vec![]
        );
    }

    /// A rate that is not a number is named rather than skipped: a dropped row would be
    /// indistinguishable from a day the ECB did not publish [ARC-019].
    #[test]
    fn a_rate_that_is_not_a_number_is_refused() {
        let document =
            envelope(r#"<Cube time="2024-04-02"><Cube currency="USD" rate="N/A"/></Cube>"#);

        let error = parse_reference_rates(&document).expect_err("N/A is not a rate");

        let message = error.to_string();
        assert!(message.contains("N/A"), "{message} does not name the value");
        assert!(
            message.contains("USD"),
            "{message} does not name the currency"
        );
    }

    /// A rate in exponent form is refused rather than read: `Decimal::from_str` would take
    /// `1.1e2` for 110, and a magnitude two orders out is a wrong conversion nothing later
    /// catches, where the refusal names the value [ARC-015].
    #[test]
    fn a_rate_in_exponent_form_is_refused() {
        for rate in ["1.1e2", "1.1E2", "11e-1"] {
            let document = envelope(&format!(
                r#"<Cube time="2024-04-02"><Cube currency="USD" rate="{rate}"/></Cube>"#
            ));

            let error = parse_reference_rates(&document)
                .expect_err("the ECB publishes plain decimal notation");

            let message = error.to_string();
            assert!(message.contains(rate), "{message} does not name the value");
        }
    }

    /// A date that is not a date is refused for the same reason, and names the value it could
    /// not read.
    #[test]
    fn a_time_that_is_not_a_date_is_refused() {
        let document =
            envelope(r#"<Cube time="yesterday"><Cube currency="USD" rate="1.0749"/></Cube>"#);

        let error = parse_reference_rates(&document).expect_err("yesterday is not a date");

        assert!(
            matches!(error, FeedError::Malformed { .. }),
            "a date that is not a date is malformed, got {error:?}"
        );
        let message = error.to_string();
        assert!(
            message.contains("yesterday"),
            "{message} does not name the value"
        );
    }

    /// A rate outside any dated cube has no day to belong to, and guessing one would date a
    /// conversion wrongly.
    #[test]
    fn a_rate_with_no_enclosing_day_is_refused() {
        let document = envelope(r#"<Cube currency="USD" rate="1.0749"/>"#);

        let error = parse_reference_rates(&document).expect_err("a rate before any date");

        assert!(
            matches!(error, FeedError::Malformed { .. }),
            "a dateless rate is malformed, got {error:?}"
        );
        let message = error.to_string();
        assert!(
            message.contains("USD"),
            "{message} does not name the currency"
        );
    }

    /// Half an attribution is refused rather than read as a day the ECB did not publish that
    /// currency for: a dropped row leaves a hole in the seeded series that only surfaces at an
    /// import, as an unavailable rate [ARC-017].
    #[test]
    fn a_cube_naming_a_currency_without_a_rate_is_refused() {
        let document = envelope(r#"<Cube time="2024-04-02"><Cube currency="USD"/></Cube>"#);

        let error = parse_reference_rates(&document).expect_err("a currency without a rate");

        let message = error.to_string();
        assert!(
            message.contains("USD"),
            "{message} does not name the currency"
        );
    }

    /// And the mirror case: a rate with nothing to attribute it to.
    #[test]
    fn a_cube_stating_a_rate_without_a_currency_is_refused() {
        let document = envelope(r#"<Cube time="2024-04-02"><Cube rate="1.0749"/></Cube>"#);

        let error = parse_reference_rates(&document).expect_err("a rate without a currency");

        let message = error.to_string();
        assert!(
            message.contains("1.0749"),
            "{message} does not name the rate"
        );
    }

    /// A download that stopped halfway is refused rather than read as the days it happens to
    /// carry: a cache seeded from half the series has a hole nothing later notices [ARC-017].
    #[test]
    fn a_truncated_document_is_refused() {
        let truncated = r#"<gesmes:Envelope><Cube><Cube time="2024-04-02">
            <Cube currency="USD" rate="1.0749"/>"#;

        let error = parse_reference_rates(truncated).expect_err("a half-downloaded document");

        assert!(
            matches!(error, FeedError::Malformed { .. }),
            "a truncated document is malformed, got {error:?}"
        );
        let message = error.to_string();
        assert!(
            message.contains("ends inside"),
            "{message} does not say where the document stops"
        );
    }

    /// A body that is not the envelope is refused rather than read as an empty series [ARC-017].
    /// An empty response, a gateway's plain-text fault and an error page each parse without
    /// raising on their own — no element, no `Cube`, depth back at zero — so without the
    /// envelope check `seed` would answer `Ok(0)` and leave the cache empty while telling the
    /// caller it succeeded. That is the state ARC-017 exists to prevent, and it is what
    /// distinguishes these from `an_envelope_with_no_day_yields_nothing`: envelope absent, not a
    /// series that is genuinely empty.
    #[test]
    fn a_body_that_is_not_the_envelope_is_refused() {
        for body in [
            "",
            "502 Bad Gateway",
            "<html><body>error</body></html>",
            r#"{"error":"nope"}"#,
            "<gesmes:Envelope><Cube time=\"2024-04-02\"></gesmes:Envelope>",
        ] {
            assert!(
                matches!(
                    parse_reference_rates(body),
                    Err(FeedError::Malformed { .. })
                ),
                "{body:?} is not a euro reference-rate envelope"
            );
        }
    }

    /// A currency cube that is a *sibling* of a dated cube rather than a child of one has no day
    /// of its own, and the day before it is not one either: filing it under the previous
    /// publication would be a wrong date rather than a missing one, which nothing later catches
    /// [ARC-015].
    #[test]
    fn a_rate_beside_a_closed_day_is_refused() {
        let document = envelope(
            r#"
        <Cube time="2024-03-28"><Cube currency="USD" rate="1.0811"/></Cube>
        <Cube currency="CAD" rate="1.4645"/>"#,
        );

        let error = parse_reference_rates(&document).expect_err("a rate outside any dated cube");

        let message = error.to_string();
        assert!(
            message.contains("CAD"),
            "{message} does not name the currency"
        );
    }

    /// The mirror of the case above for a self-closing dated cube: it encloses nothing, so the
    /// day it states applies to no element beyond itself and the currency cube that follows is a
    /// sibling. Filing that rate under the date beside it would be a wrong conversion that no
    /// later error surfaces, where the refusal names the currency [ARC-015].
    #[test]
    fn a_rate_beside_a_self_closing_day_is_refused() {
        let document = envelope(r#"<Cube time="2024-03-28"/><Cube currency="USD" rate="1.0811"/>"#);

        let error = parse_reference_rates(&document).expect_err("a rate outside any dated cube");

        let message = error.to_string();
        assert!(
            message.contains("USD"),
            "{message} does not name the currency"
        );
    }

    /// A cube stating `time`, `currency` and `rate` at once takes its own day rather than the
    /// enclosing one: the date nearest the rate is the one the document attributes it to
    /// [ARC-015].
    #[test]
    fn a_cube_stating_its_own_day_takes_it_over_the_enclosing_one() {
        let document = envelope(
            r#"<Cube time="2024-04-02"><Cube time="2024-03-28" currency="USD" rate="1.0811"/></Cube>"#,
        );

        assert_eq!(
            parse_reference_rates(&document).expect("a well-formed envelope"),
            vec![Observation {
                currency: Currency::new("USD"),
                date: day(2024, 3, 28),
                rate: FxRate::new(dec!(1.0811)),
            }]
        );
    }

    /// A rate is foreign units per EUR and a conversion divides by it [ARC-027], so zero and
    /// negative are refused at the document rather than stored: a stored zero makes a leg
    /// unvaluable much later, with neither currency nor date named [ARC-015].
    #[test]
    fn a_rate_that_is_not_positive_is_refused() {
        for rate in ["0", "0.000000", "-1.0811"] {
            let document = envelope(&format!(
                r#"<Cube time="2024-04-02"><Cube currency="USD" rate="{rate}"/></Cube>"#
            ));

            let error =
                parse_reference_rates(&document).expect_err("a non-positive rate is not a rate");

            let message = error.to_string();
            assert!(message.contains(rate), "{message} does not name the value");
            assert!(
                message.contains("USD"),
                "{message} does not name the currency"
            );
        }
    }
}
