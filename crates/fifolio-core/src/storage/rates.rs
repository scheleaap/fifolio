//! The cached ECB rate table [ARC-015], and the read path that answers FIF-009's port.
//!
//! # Why the read path is a snapshot
//!
//! [`RateTable`] is synchronous — it is the port `fx::resolve` calls per leg, and FIF-009 fixed
//! its one method deliberately — while `sqlx` is asynchronous. So the query does not happen per
//! leg: [`RateRepository::snapshot`] reads the table once and [`CachedRates`] answers every
//! lookup from memory. That is the shape an import wants anyway, a file of several thousand
//! rows otherwise issuing several thousand ordered queries, and the whole series is a few
//! megabytes [ARC-017].
//!
//! The lookup itself is the port's contract and no more: the most recent publication for a
//! currency on or before a date, with no fallback bound and no error of its own. The bound and
//! the three errors are `fx`'s [ARC-019, ARC-027] and are not restated here.
//!
//! Once the table is seeded, that is the entire read path: no lookup here reaches outside the
//! database file [ARC-016].

use std::collections::BTreeMap;

use chrono::NaiveDate;
use sqlx::sqlite::SqlitePool;
use sqlx::{Row, query};

use crate::decimal::FxRate;
use crate::ecb::Observation;
use crate::fx::{PublishedRate, RateTable};
use crate::storage::StorageError;
use crate::storage::codec::{at_scale, rate as stored_rate};
use crate::valuation::Currency;

/// The cache of ECB daily reference rates, keyed by currency and date [ARC-015].
pub struct RateRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> RateRepository<'a> {
    pub(super) fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Stores `observations`, leaving a day already cached as it is, and answers with how many
    /// rows were written.
    ///
    /// Insert-and-keep rather than upsert: a published reference rate for a day is final, and
    /// the 90-day window [ARC-018] restates days the historical series [ARC-017] already
    /// deposited, so a top-up must not rewrite a rate a stored conversion was valued at
    /// [DEC-068].
    ///
    /// One transaction for the whole document: the historical series is some hundreds of
    /// thousands of rows, and a seeding interrupted halfway is a cache with a hole in it that
    /// nothing afterwards would notice.
    pub async fn store(&self, observations: &[Observation]) -> Result<usize, StorageError> {
        let mut transaction = self.pool.begin().await?;
        let mut written = 0;

        for observation in observations {
            let inserted = query(
                "insert into fx_rate (currency, rate_date, rate) values (?, ?, ?)
                 on conflict (currency, rate_date) do nothing",
            )
            .bind(observation.currency.code())
            .bind(observation.date)
            .bind(at_scale("rate", observation.rate)?)
            .execute(&mut *transaction)
            .await?;

            if inserted.rows_affected() > 0 {
                written += 1;
            }
        }

        transaction.commit().await?;
        Ok(written)
    }

    /// The whole table, as the in-memory [`RateTable`] an import resolves against.
    pub async fn snapshot(&self) -> Result<CachedRates, StorageError> {
        let rows = query("select currency, rate_date, rate from fx_rate")
            .fetch_all(self.pool)
            .await?;

        rows.into_iter()
            .map(|row| {
                let rate = stored_rate("rate", &row.get::<String, _>("rate"))?;
                let key = (
                    Currency::new(row.get::<String, _>("currency")),
                    row.get::<NaiveDate, _>("rate_date"),
                );
                Ok((key, rate))
            })
            .collect::<Result<BTreeMap<_, _>, StorageError>>()
            .map(|rates| CachedRates { rates })
    }
}

/// The cached rates, in memory, as [`fx::resolve`](crate::fx::resolve) reads them.
#[derive(Debug, Clone, Default)]
pub struct CachedRates {
    rates: BTreeMap<(Currency, NaiveDate), FxRate>,
}

impl RateTable for CachedRates {
    /// A backward range scan over the composite key, which stops inside the currency asked for:
    /// a USD lookup never reaches a CAD publication, the key ordering currency first.
    fn latest_on_or_before(&self, currency: &Currency, date: NaiveDate) -> Option<PublishedRate> {
        self.rates
            .range(..=(currency.clone(), date))
            .next_back()
            .filter(|((held, _), _)| held == currency)
            .map(|((_, date), rate)| PublishedRate {
                rate: *rate,
                date: *date,
            })
    }

    fn newest_publication(&self) -> Option<NaiveDate> {
        self.rates.keys().map(|(_, date)| *date).max()
    }
}
