//! Resolving which rate values a leg in EUR, and on what day [DOM-030].
//!
//! The rate *table* is not here: persistence, seeding from the ECB historical series and the
//! 90-day top-up are FIF-010 [ARC-015 to ARC-018]. What this module owns is resolution — the
//! precedence, the previous-publication fallback and its bound, and the error — behind the
//! injected [`RateTable`] port, so it is unit testable against a fake with weekend and holiday
//! gaps and reaches no socket [TST-019, TST-020].
//!
//! # Precedence
//!
//! `broker` > `ecb` > `native` [DOM-030]. [`resolve`] answers all three, and the order it
//! tests them in is not the order they are listed in: an EUR-denominated leg takes rate 1 and
//! source `native` [DOM-033] even though the file "states EUR figures", because those figures
//! are the native ones and no conversion took place. The precedence orders how a *foreign*
//! figure is valued, and there the file's own booked EUR total wins over a looked-up rate.
//!
//! A broker rate is **informational**: the EUR figures are used verbatim because they are what
//! was actually paid, and the rate is only the quotient they imply [DOM-031]. How it is derived
//! is each format's business — Saxo states the reciprocal of one, Trade Republic states it
//! outright — so [`Stated::Eur`] takes the rate the importer derived, and
//! [`implied_rate`] is offered for a format that states no rate column at all. Nothing in this
//! crate converts with a broker rate; [`RateSource::is_informational`] is the mechanical form of
//! that, and [`fee_in_eur`] refuses one.
//!
//! # The fallback and its bound
//!
//! The ECB publishes on business days, so a trade on a Saturday or a holiday has no rate of its
//! own. The most recent publication *before* the trade date stands in, and the stored
//! [`Conversion::rate_date`] is that publication's own date, which is what makes the
//! substitution visible [DOM-034].
//!
//! It is bounded on both sides [ARC-027]: nothing before the series begins in 1999, and a
//! substitution more than [`MAX_SUBSTITUTION_DAYS`] days stale is an error rather than a silent
//! approximation. A rate that is missing outright is an error too, naming the currency and the
//! date rather than guessing [ARC-019].
//!
//! # Rounding
//!
//! Nothing here rounds. A resolved rate is stored as the table or the format gave it, and the
//! stored scale is applied at the persistence boundary [ARC-009]; rounding a rate before
//! converting with it is what DEC-027 forbids.

use chrono::NaiveDate;
use thiserror::Error;

use crate::decimal::{FxRate, Money, Scaled};
use crate::valuation::{Conversion, Currency, RateSource, Valued};

/// The first year the ECB euro reference series covers; nothing before it exists [ARC-027].
pub const SERIES_START_YEAR: i32 = 1999;

/// How stale a substituted rate may be, in days [ARC-027].
///
/// Seven covers the longest gap the series actually has — the Christmas and New Year holidays
/// straddling two weekends — and refuses anything that would be an approximation rather than a
/// publication.
pub const MAX_SUBSTITUTION_DAYS: i64 = 7;

/// The first date a rate can exist for [ARC-027].
///
/// The series' first publication is a few days into 1999; `architecture.md` bounds it by the
/// year, so this is 1 January and a lookup inside the first days of 1999 fails through the
/// ordinary "nothing published" path rather than through a hardcoded first business day.
#[must_use]
pub fn series_start() -> NaiveDate {
    NaiveDate::from_ymd_opt(SERIES_START_YEAR, 1, 1).expect("1 January 1999 is a valid date")
}

/// A rate as published, with the day it was published for.
///
/// The date travels with the rate because a fallback stores it [DOM-034]: a lookup that
/// returned the rate alone could not say which day it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishedRate {
    pub rate: FxRate,
    pub date: NaiveDate,
}

/// The injected source of ECB daily reference rates [TST-020].
///
/// One method rather than an exact-date lookup plus a backward walk: the fallback is "the most
/// recent published rate before the trade date" [DOM-034], which is one ordered query for any
/// real table and a range lookup for the test fake, and a day-by-day walk in the resolver would
/// make the bound of [`MAX_SUBSTITUTION_DAYS`] the *search* limit rather than a rule about the
/// answer — so a rate eight days stale would be indistinguishable from no rate at all, and the
/// two are different errors.
pub trait RateTable {
    /// The most recent rate published for `currency` on or before `date`, if any.
    ///
    /// Returns the rate *of* `date` when one was published, which is the ordinary case
    /// [DOM-032].
    fn latest_on_or_before(&self, currency: &Currency, date: NaiveDate) -> Option<PublishedRate>;
}

/// What the source file says about a leg's EUR value, which is what decides the rate source
/// [DOM-030].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stated {
    /// The file booked the EUR figures itself. They are used verbatim; `rate` is the quotient
    /// they imply, derived as the format specifies, and is informational [DOM-031].
    Eur { rate: FxRate },
    /// The file states native figures only, so the ECB rate for the trade date values them
    /// [DOM-032].
    NativeOnly,
}

/// Why a leg could not be valued in EUR.
///
/// Every variant names the currency and the date, because an import that fails for want of a
/// rate must say which one [ARC-019].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RateError {
    /// Before the series exists, so no fallback can reach a rate [ARC-027].
    #[error(
        "no {} rate for {date}: the ECB euro reference series begins in {SERIES_START_YEAR}",
        .currency.code()
    )]
    BeforeSeries { currency: Currency, date: NaiveDate },

    /// Nothing published on or before the date: neither cached nor fetchable [ARC-019].
    #[error("no {} rate available for {date}", .currency.code())]
    Unavailable { currency: Currency, date: NaiveDate },

    /// A publication exists but is too old to stand in for the date [ARC-027].
    #[error(
        "the most recent {} rate before {date} is from {rate_date}, more than \
         {MAX_SUBSTITUTION_DAYS} days stale",
        .currency.code()
    )]
    StaleSubstitute {
        currency: Currency,
        date: NaiveDate,
        rate_date: NaiveDate,
    },
}

/// The conversion a leg's EUR figures are valued under, at the trade date [DOM-025, DOM-027].
///
/// `trade_date` is the valuation date throughout: settlement never values anything here.
///
/// # Errors
///
/// [`RateError`] when the ECB path finds no rate it may use. The `broker` and `native` paths
/// cannot fail: both carry their rate with them.
pub fn resolve(
    table: &impl RateTable,
    currency: &Currency,
    trade_date: NaiveDate,
    stated: Stated,
) -> Result<Conversion, RateError> {
    if currency.is_eur() {
        // Ahead of `broker` deliberately: see the module documentation [DOM-033].
        return Ok(Conversion::native(trade_date));
    }

    match stated {
        Stated::Eur { rate } => Ok(Conversion::new(
            currency.clone(),
            rate,
            RateSource::Broker,
            trade_date,
        )),
        Stated::NativeOnly => resolve_ecb(table, currency, trade_date),
    }
}

/// The ECB rate for the trade date, or the bounded substitute for it [DOM-032, DOM-034].
fn resolve_ecb(
    table: &impl RateTable,
    currency: &Currency,
    trade_date: NaiveDate,
) -> Result<Conversion, RateError> {
    if trade_date < series_start() {
        return Err(RateError::BeforeSeries {
            currency: currency.clone(),
            date: trade_date,
        });
    }

    let published = table
        .latest_on_or_before(currency, trade_date)
        .ok_or_else(|| RateError::Unavailable {
            currency: currency.clone(),
            date: trade_date,
        })?;

    if (trade_date - published.date).num_days() > MAX_SUBSTITUTION_DAYS {
        return Err(RateError::StaleSubstitute {
            currency: currency.clone(),
            date: trade_date,
            rate_date: published.date,
        });
    }

    Ok(Conversion::new(
        currency.clone(),
        published.rate,
        RateSource::Ecb,
        // The substitution is visible because the rate's own date is stored [DOM-034].
        published.date,
    ))
}

/// The rate a pair of booked figures implies, `rate = native / eur` [DOM-086].
///
/// For a format that books both halves but states no rate of its own: the quotient is what
/// [`Stated::Eur`] stores, and it is informational [DOM-031]. A format that states a rate
/// column passes that instead — which is what "derived as each format specifies" means.
///
/// `None` when the quotient does not exist: an EUR half of zero, where a leg of nothing values
/// at any rate, and an overflow. The caller decides what to do with a leg whose rate is
/// undetermined, because that is a per-format question about the row, not about arithmetic.
#[must_use]
pub fn implied_rate(native: Money, eur: Money) -> Option<FxRate> {
    native.get().checked_div(eur.get()).map(FxRate::new)
}

/// A fee in EUR, converted at the rate of the leg it belongs to [DOM-035].
///
/// `None` when the leg's rate is informational — a broker-sourced leg books its EUR fee in the
/// file, and that figure is used verbatim as the gross is [DOM-031] — or when the rate does not
/// divide.
#[must_use]
pub fn fee_in_eur(conversion: &Conversion, native_fee: Money) -> Option<Valued<Money>> {
    if conversion.source().is_informational() {
        return None;
    }
    Valued::converted(native_fee, conversion.rate())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    /// A known rate table with gaps [TST-020, TST-021]. Nothing in these tests opens a socket
    /// [TST-019].
    ///
    /// The dates are real ECB publication days around the 2024 Easter weekend: Thursday 28
    /// March published, Good Friday 29 March and Easter Monday 1 April did not, and 2 April
    /// resumed. The surrounding weekend gaps are the ordinary ones.
    struct FakeTable {
        rates: BTreeMap<(Currency, NaiveDate), FxRate>,
    }

    impl FakeTable {
        fn with(entries: &[(&str, NaiveDate, Decimal)]) -> Self {
            Self {
                rates: entries
                    .iter()
                    .map(|(code, date, rate)| ((Currency::new(code), *date), FxRate::new(*rate)))
                    .collect(),
            }
        }
    }

    impl RateTable for FakeTable {
        fn latest_on_or_before(
            &self,
            currency: &Currency,
            date: NaiveDate,
        ) -> Option<PublishedRate> {
            self.rates
                .range(..=(currency.clone(), date))
                .next_back()
                .filter(|((held, _), _)| held == currency)
                .map(|((_, date), rate)| PublishedRate {
                    rate: *rate,
                    date: *date,
                })
        }
    }

    fn day(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("a valid date")
    }

    /// Thursday 28 March 2024 and Tuesday 2 April 2024, with Good Friday, the weekend and
    /// Easter Monday absent between them.
    fn easter_2024() -> FakeTable {
        FakeTable::with(&[
            ("USD", day(2024, 3, 28), dec!(1.0811)),
            ("USD", day(2024, 4, 2), dec!(1.0749)),
            ("CAD", day(2024, 3, 28), dec!(1.4645)),
        ])
    }

    /// An EUR leg is valued at rate 1 from source `native`, whatever the file states and
    /// whatever the table holds [DOM-030, DOM-033].
    #[test]
    fn an_eur_leg_takes_rate_one_and_source_native() {
        let trade_date = day(2024, 3, 28);

        for stated in [
            Stated::NativeOnly,
            Stated::Eur {
                rate: FxRate::new(dec!(1.0811)),
            },
        ] {
            let conversion = resolve(&easter_2024(), &Currency::eur(), trade_date, stated)
                .expect("an EUR leg needs no lookup");

            assert_eq!(conversion.source(), RateSource::Native);
            assert_eq!(conversion.rate(), FxRate::new(Decimal::ONE));
            assert_eq!(conversion.rate_date(), trade_date);
            assert!(conversion.currency().is_eur());
        }
    }

    /// A file that books the EUR figures wins over the ECB table [DOM-030, DOM-031].
    #[test]
    fn a_booked_eur_figure_outranks_the_ecb_rate() {
        let trade_date = day(2024, 3, 28);
        let broker_rate = FxRate::new(dec!(1.097171));

        let conversion = resolve(
            &easter_2024(),
            &Currency::new("USD"),
            trade_date,
            Stated::Eur { rate: broker_rate },
        )
        .expect("a stated rate needs no lookup");

        assert_eq!(conversion.source(), RateSource::Broker);
        assert_eq!(
            conversion.rate(),
            broker_rate,
            "the ECB rate of 1.0811 published that day is not consulted"
        );
        assert_eq!(conversion.rate_date(), trade_date);
    }

    /// The broker rate is the quotient the booked figures imply, and nothing converts with it
    /// [DOM-031, DOM-086].
    #[test]
    fn a_broker_rate_is_the_informational_quotient_of_the_booked_figures() {
        // The worked Saxo buy in `importers.md`: 230.00 USD of gross booked as 209.63 EUR.
        let rate = implied_rate(Money::new(dec!(230.00)), Money::new(dec!(209.63)))
            .expect("a non-zero EUR figure divides");

        // Exact, not cut to the stored scale: dividing the native gross by it reproduces the
        // booked EUR figure to the cent.
        assert_eq!(
            Valued::converted(Money::new(dec!(230.00)), rate)
                .expect("a non-zero rate divides")
                .rounded()
                .eur(),
            Money::new(dec!(209.63))
        );
        assert_eq!(rate.rounded(), FxRate::new(dec!(1.097171)));

        assert!(RateSource::Broker.is_informational());
        assert!(!RateSource::Ecb.is_informational());
        assert!(!RateSource::Native.is_informational());
    }

    /// A leg of nothing has no implied rate rather than a panic [DOM-031].
    #[test]
    fn an_implied_rate_over_a_zero_eur_figure_is_none() {
        assert_eq!(implied_rate(Money::new(dec!(12.50)), Money::zero()), None);
    }

    /// With no EUR figure in the file, the ECB rate for the trade date is used [DOM-032].
    #[test]
    fn a_foreign_leg_with_no_eur_figure_takes_the_ecb_rate_of_the_trade_date() {
        let trade_date = day(2024, 3, 28);

        let conversion = resolve(
            &easter_2024(),
            &Currency::new("USD"),
            trade_date,
            Stated::NativeOnly,
        )
        .expect("that Thursday published");

        assert_eq!(conversion.source(), RateSource::Ecb);
        assert_eq!(conversion.rate(), FxRate::new(dec!(1.0811)));
        assert_eq!(conversion.rate_date(), trade_date);
        assert_eq!(conversion.currency(), &Currency::new("USD"));
    }

    /// A date the ECB did not publish for falls back to the most recent publication before it,
    /// and stores that rate's own date [DOM-034, TST-021].
    #[test]
    fn a_gap_falls_back_to_the_last_publication_and_stores_its_date() {
        let thursday = day(2024, 3, 28);
        let table = easter_2024();

        // Good Friday, Saturday, Sunday and Easter Monday all resolve to the Thursday.
        for trade_date in [
            day(2024, 3, 29),
            day(2024, 3, 30),
            day(2024, 3, 31),
            day(2024, 4, 1),
        ] {
            let conversion = resolve(
                &table,
                &Currency::new("USD"),
                trade_date,
                Stated::NativeOnly,
            )
            .expect("within the bound of the Thursday rate");

            assert_eq!(conversion.rate(), FxRate::new(dec!(1.0811)));
            assert_eq!(
                conversion.rate_date(),
                thursday,
                "the substitution is visible because the rate's own date is stored"
            );
        }
    }

    /// A substitution is bounded at seven days, inclusive [ARC-027].
    #[test]
    fn a_substitution_is_bounded_at_seven_days() {
        let published = day(2024, 3, 28);
        let table = easter_2024();
        let resolve_cad = |trade_date| {
            resolve(
                &table,
                &Currency::new("CAD"),
                trade_date,
                Stated::NativeOnly,
            )
        };

        let at_the_bound = resolve_cad(published + chrono::Duration::days(MAX_SUBSTITUTION_DAYS))
            .expect("exactly seven days stale is still a publication");
        assert_eq!(at_the_bound.rate_date(), published);

        assert_eq!(
            resolve_cad(published + chrono::Duration::days(MAX_SUBSTITUTION_DAYS + 1)),
            Err(RateError::StaleSubstitute {
                currency: Currency::new("CAD"),
                date: day(2024, 4, 5),
                rate_date: published,
            }),
            "eight days is an approximation, not a publication"
        );
    }

    /// Nothing exists before the series begins in 1999 [ARC-027].
    #[test]
    fn a_date_before_the_series_is_an_error() {
        let table = FakeTable::with(&[("USD", day(1998, 12, 31), dec!(1.1789))]);

        assert_eq!(
            resolve(
                &table,
                &Currency::new("USD"),
                day(1998, 12, 31),
                Stated::NativeOnly
            ),
            Err(RateError::BeforeSeries {
                currency: Currency::new("USD"),
                date: day(1998, 12, 31),
            }),
            "a table holding a pre-1999 rate does not make one exist"
        );
    }

    /// A rate that is neither cached nor fetchable fails naming the currency and the date
    /// [ARC-019].
    #[test]
    fn a_missing_rate_names_the_currency_and_the_date() {
        let missing = resolve(
            &easter_2024(),
            &Currency::new("chf"),
            day(2024, 3, 28),
            Stated::NativeOnly,
        )
        .expect_err("the table holds no CHF");

        assert_eq!(
            missing,
            RateError::Unavailable {
                currency: Currency::new("CHF"),
                date: day(2024, 3, 28),
            }
        );

        for error in [
            missing,
            RateError::BeforeSeries {
                currency: Currency::new("CHF"),
                date: day(2024, 3, 28),
            },
            RateError::StaleSubstitute {
                currency: Currency::new("CHF"),
                date: day(2024, 3, 28),
                rate_date: day(2024, 3, 1),
            },
        ] {
            let message = error.to_string();
            assert!(message.contains("CHF"), "{message} names no currency");
            assert!(message.contains("2024-03-28"), "{message} names no date");
        }
    }

    /// A currency's fallback never reaches another currency's publication [ARC-015].
    #[test]
    fn a_lookup_stays_inside_its_own_currency() {
        let table = FakeTable::with(&[("CAD", day(2024, 3, 28), dec!(1.4645))]);

        assert_eq!(
            resolve(
                &table,
                &Currency::new("USD"),
                day(2024, 3, 28),
                Stated::NativeOnly
            ),
            Err(RateError::Unavailable {
                currency: Currency::new("USD"),
                date: day(2024, 3, 28),
            })
        );
    }

    /// A fee is converted at the rate of the leg it belongs to [DOM-035].
    #[test]
    fn a_fee_converts_at_the_rate_of_its_leg() {
        // A Saturday, so the leg resolves to the Thursday rate; the fee must take that same
        // rate and not the Tuesday one the file's own date is nearer to.
        let leg = resolve(
            &easter_2024(),
            &Currency::new("USD"),
            day(2024, 3, 30),
            Stated::NativeOnly,
        )
        .expect("within the bound");

        let fee = fee_in_eur(&leg, Money::new(dec!(7.29))).expect("an ECB rate divides");

        assert_eq!(fee.native(), Money::new(dec!(7.29)));
        assert_eq!(fee.rounded().eur(), Money::new(dec!(6.74)));
        assert_ne!(
            fee.rounded().eur(),
            Valued::converted(Money::new(dec!(7.29)), FxRate::new(dec!(1.0749)))
                .expect("a non-zero rate divides")
                .rounded()
                .eur(),
            "the following Tuesday's rate would give 6.78"
        );
    }

    /// A broker leg's fee is booked in the file, so nothing converts it at the informational
    /// rate [DOM-031, DOM-035].
    #[test]
    fn a_broker_leg_does_not_convert_its_fee() {
        let leg = resolve(
            &easter_2024(),
            &Currency::new("USD"),
            day(2024, 3, 28),
            Stated::Eur {
                rate: FxRate::new(dec!(1.097171)),
            },
        )
        .expect("a stated rate needs no lookup");

        assert_eq!(fee_in_eur(&leg, Money::new(dec!(7.29))), None);
    }
}
