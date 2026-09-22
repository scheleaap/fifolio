//! EUR valuation: the native/EUR figure pairs every transaction stores, and the rate that
//! relates them [DOM-028].
//!
//! Types only. Nothing here resolves a rate — the precedence `broker` > `ecb` > `native`, the
//! ECB lookup and its previous-business-day fallback are DOM-030 to DOM-035 and belong to
//! FIF-009. What this module fixes is the *shape* of what a transaction stores, which is what
//! makes an EUR figure reproducible and auditable rather than opaque.
//!
//! # Why a pair and not a conversion
//!
//! Each leg of a trade is valued in EUR at its own date [DOM-025], so the movement of the
//! currency between acquisition and disposal falls inside the securities gain and is never
//! reported separately [DOM-026]. There is therefore no currency-gain field anywhere in this
//! crate, and the unit test `the_model_reports_no_separate_currency_movement` is the mechanical
//! form of that absence.
//!
//! The date of the valuation is the **trade date** — the obligating transaction — and never
//! the settlement date [DOM-027]. A transaction carries exactly one date for this purpose,
//! `Transaction::valuation_date`, and no type here holds a settlement date at all.
//!
//! [`Valued`] holds the native figure and the EUR figure side by side at one scale [DOM-029],
//! so a caller cannot hold a pair whose halves are rounded differently, and allocation shares
//! read the EUR half exactly as they read the native one [DOM-084].
//!
//! # The stored rate runs foreign units per EUR
//!
//! `EUR = native / rate` [DOM-086], the ECB convention, which Trade Republic also states.
//! Saxo states the inverse. An importer meeting the other convention converts **from the
//! figures the file states**, at full precision, and only then stores the quotient as the rate:
//! the stored rate is 6 decimals, and for a currency quoted in the hundreds — JPY — inverting a
//! rounded rate loses four significant figures (DEC-027). [`rate_from_inverse_quote`] performs
//! that inversion without rounding, and [`Valued::converted`] never rounds either, so a caller
//! that keeps the full-precision rate gets a full-precision EUR figure.
//!
//! Nothing here computes with the stored rate when the file already states the EUR figures:
//! [`Valued::new`] takes both halves, which is how broker-booked figures are used verbatim
//! [DOM-031].

use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::decimal::{FxRate, Scaled};

/// The currency a native figure is denominated in.
///
/// Uppercased on construction so that one currency is one key: the ECB rate table is keyed by
/// currency and date [ARC-015], and `usd` and `USD` reaching it as two currencies would be a
/// silent cache miss. Not validated against ISO 4217: a broker file that names a currency is
/// the authority on what it is called, as it is for an [`crate::entities::Isin`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Currency(String);

impl Currency {
    #[must_use]
    pub fn new(code: impl AsRef<str>) -> Self {
        Self(code.as_ref().trim().to_ascii_uppercase())
    }

    /// The currency every stored EUR figure is in.
    #[must_use]
    pub fn eur() -> Self {
        Self("EUR".to_owned())
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.0
    }

    /// Whether the transaction is already denominated in EUR, which is the `native` rate
    /// source's condition [DOM-033]. Resolving the source itself is FIF-009's.
    #[must_use]
    pub fn is_eur(&self) -> bool {
        self.0 == "EUR"
    }
}

/// A figure in both currencies: what the statement booked, and what it is worth in EUR
/// [DOM-028].
///
/// One type parameter, so both halves are the same kind and therefore the same scale
/// [DOM-029]: a pair whose native half is a price and whose EUR half is an amount does not
/// exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Valued<T> {
    native: T,
    eur: T,
}

impl<T: Scaled> Valued<T> {
    /// A pair whose halves are both stated: the file booked the EUR figure, so it is used as
    /// booked rather than recomputed [DOM-031].
    #[must_use]
    pub fn new(native: T, eur: T) -> Self {
        Self { native, eur }
    }

    /// A figure already in EUR: both halves are the same, which is what rate source `native`
    /// means [DOM-033].
    #[must_use]
    pub fn in_eur(amount: T) -> Self {
        Self {
            native: amount,
            eur: amount,
        }
    }

    /// The pair implied by converting `native` at `rate`, `EUR = native / rate` [DOM-086].
    ///
    /// `None` when the division cannot be done — a zero rate, or an overflow — rather than a
    /// panic, matching [`crate::decimal::EffectivePrice::from_value`].
    ///
    /// The quotient is left exact [ARC-009]; rounding to the kind's scale happens at the
    /// storage boundary, in [`Valued::rounded`]. A caller converting at a rate it inverted
    /// itself therefore keeps every digit the file stated.
    #[must_use]
    pub fn converted(native: T, rate: FxRate) -> Option<Self> {
        native
            .get()
            .checked_div(rate.get())
            .map(|eur| Self::new(native, T::from_decimal(eur)))
    }

    /// The figure the statement booked, in the transaction's own currency.
    #[must_use]
    pub fn native(self) -> T {
        self.native
    }

    /// The same figure in EUR, at the same scale [DOM-029].
    #[must_use]
    pub fn eur(self) -> T {
        self.eur
    }

    /// Both halves at the kind's scale: the storage and presentation boundary [ARC-009].
    #[must_use]
    pub fn rounded(self) -> Self {
        Self {
            native: self.native.rounded(),
            eur: self.eur.rounded(),
        }
    }
}

/// Where the rate came from [DOM-030].
///
/// The precedence between them, and what each one requires of a rate, are DOM-031 to DOM-033
/// and belong to FIF-009. This enum is the stored label, which is what makes an EUR figure
/// auditable [DOM-028].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RateSource {
    /// The file stated the EUR figures actually booked; the stored rate is the quotient they
    /// imply and is informational [DOM-031].
    Broker,
    /// The ECB daily euro reference rate [DOM-032].
    Ecb,
    /// The transaction is already in EUR, so the rate is 1 [DOM-033].
    Native,
}

/// The conversion a transaction's EUR figures were obtained under [DOM-028].
///
/// Held whole rather than as three loose fields, so a rate can never be stored without saying
/// what it converts, where it came from and which day it is the rate of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversion {
    currency: Currency,
    rate: FxRate,
    source: RateSource,
    rate_date: NaiveDate,
}

impl Conversion {
    /// `rate_date` is the date the stored rate is the published rate *of*. It equals the trade
    /// date in the ordinary case; when the ECB published nothing for the trade date, the most
    /// recent earlier rate is used and this field is that rate's own date, so the substitution
    /// is visible rather than silent [DOM-034]. Choosing the rate, and bounding how stale a
    /// substitute may be, is FIF-009's.
    #[must_use]
    pub fn new(currency: Currency, rate: FxRate, source: RateSource, rate_date: NaiveDate) -> Self {
        Self {
            currency,
            rate,
            source,
            rate_date,
        }
    }

    /// A transaction already in EUR: rate 1, source `native`, as of its own trade date
    /// [DOM-033].
    #[must_use]
    pub fn native(trade_date: NaiveDate) -> Self {
        Self::new(
            Currency::eur(),
            FxRate::new(Decimal::ONE),
            RateSource::Native,
            trade_date,
        )
    }

    /// The currency the native figures are denominated in.
    #[must_use]
    pub fn currency(&self) -> &Currency {
        &self.currency
    }

    /// Foreign units per EUR: `EUR = native / rate` [DOM-086].
    #[must_use]
    pub fn rate(&self) -> FxRate {
        self.rate
    }

    #[must_use]
    pub fn source(&self) -> RateSource {
        self.source
    }

    /// The date the stored rate is the rate of, which is the trade date unless a fallback was
    /// used [DOM-034].
    #[must_use]
    pub fn rate_date(&self) -> NaiveDate {
        self.rate_date
    }
}

/// A rate stated the other way round — EUR per foreign unit, as Saxo's `Omrekeningskoers` is —
/// as the foreign-units-per-EUR rate this crate stores [DOM-086].
///
/// The reciprocal is taken at full precision and is **not** rounded here: rounding it to the
/// stored 6 decimals and converting with the result is exactly what DEC-027 forbids. Callers
/// convert with what this returns and round only at the storage boundary.
///
/// `None` on a zero quote or an overflow, rather than a panic.
#[must_use]
pub fn rate_from_inverse_quote(quote: Decimal) -> Option<FxRate> {
    Decimal::ONE.checked_div(quote).map(FxRate::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    use rust_decimal_macros::dec;

    use crate::decimal::{FX_RATE_SCALE, MONEY_SCALE, Money, PRICE_SCALE, QuotedPrice};

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 5, 2).expect("a valid date")
    }

    /// A pair holds the booked native figure and the booked EUR figure together [DOM-028].
    #[test]
    fn a_pair_holds_the_native_and_the_eur_figure() {
        // The worked Saxo buy in `importers.md`: 40 @ 5.75 USD is 230.00 USD of gross, booked
        // as 209.63 EUR.
        let gross = Valued::new(Money::new(dec!(230.00)), Money::new(dec!(209.63)));

        assert_eq!(gross.native(), Money::new(dec!(230.00)));
        assert_eq!(gross.eur(), Money::new(dec!(209.63)));
    }

    /// Both halves of a pair are the same kind, so they round to the same scale [DOM-029].
    #[test]
    fn both_halves_of_a_pair_share_one_scale() {
        let fees = Valued::new(Money::new(dec!(8.324)), Money::new(dec!(7.996))).rounded();
        assert_eq!(fees.native().get(), dec!(8.32));
        assert_eq!(fees.eur().get(), dec!(8.00));
        assert_eq!(fees.native().get().scale(), MONEY_SCALE);
        assert_eq!(fees.eur().get().scale(), MONEY_SCALE);

        let price = Valued::new(
            QuotedPrice::new(dec!(5.7500364)),
            QuotedPrice::new(dec!(5.2407499)),
        )
        .rounded();
        assert!(price.native().get().scale() <= PRICE_SCALE);
        assert!(price.eur().get().scale() <= PRICE_SCALE);
        assert_eq!(price.eur().get(), dec!(5.240750));
    }

    /// An EUR-denominated figure is the same on both sides, which is what rate source `native`
    /// means [DOM-033].
    #[test]
    fn an_eur_figure_pairs_with_itself() {
        let gross = Valued::in_eur(Money::new(dec!(1839.24)));

        assert_eq!(gross.native(), gross.eur());

        let conversion = Conversion::native(date());
        assert_eq!(conversion.rate(), FxRate::new(Decimal::ONE));
        assert_eq!(conversion.source(), RateSource::Native);
        assert!(conversion.currency().is_eur());
    }

    /// The stored rate is foreign units per EUR: dividing by it gives the EUR figure
    /// [DOM-086].
    #[test]
    fn the_stored_rate_is_foreign_units_per_eur() {
        // 218.32 USD at 1.0414 USD per EUR is 209.64 EUR, so multiplying instead of dividing
        // would give 227.36 and be wrong by 17.72.
        let rate = FxRate::new(dec!(1.0414));
        let gross =
            Valued::converted(Money::new(dec!(218.32)), rate).expect("a non-zero rate divides");

        assert_eq!(gross.rounded().eur().get(), dec!(209.64));
        assert_ne!(
            gross.rounded().eur().get(),
            (dec!(218.32) * rate.get()).round_dp(MONEY_SCALE)
        );
    }

    /// The conversion is left exact and rounds only at the storage boundary [ARC-009].
    #[test]
    fn a_conversion_is_exact_until_it_is_stored() {
        let gross = Valued::converted(Money::new(dec!(100.00)), FxRate::new(dec!(3)))
            .expect("3 is not zero");

        assert!(gross.eur().get().scale() > MONEY_SCALE);
        assert_eq!(gross.rounded().eur().get(), dec!(33.33));
    }

    /// A conversion that cannot be done yields nothing rather than panicking.
    #[test]
    fn an_impossible_conversion_is_none() {
        assert_eq!(
            Valued::converted(Money::new(dec!(10.00)), FxRate::new(Decimal::ZERO)),
            None
        );
        assert_eq!(rate_from_inverse_quote(Decimal::ZERO), None);
    }

    /// A format quoting the inverse converts at full precision from the figures the file
    /// states, never from a rounded rate [DOM-086].
    ///
    /// JPY is the case DEC-027 names. The file states 0.0057831429 EUR per JPY; the stored
    /// rate is its reciprocal, 172.916356..., and both roundings that could stand in for the
    /// exact conversion move the figure on a 1,000,000,000 JPY nominal:
    ///
    /// | Converted from | EUR |
    /// | --- | --- |
    /// | the stated quote, at full precision | 5_783_142.90 |
    /// | the stored rate rounded to 6 decimals | 5_783_142.89 |
    /// | the quote itself rounded to 6 decimals | 5_783_000.00 |
    ///
    /// The last is the four significant figures DEC-027 names: a quote of that size has almost
    /// nothing left after six decimals, which is why the reciprocal is taken before anything
    /// is rounded and why [`rate_from_inverse_quote`] does not round.
    #[test]
    fn an_inverse_quote_converts_at_full_precision() {
        let quote = dec!(0.0057831429);
        let rate = rate_from_inverse_quote(quote).expect("a non-zero quote inverts");
        assert!(
            rate.get().scale() > FX_RATE_SCALE,
            "the reciprocal is kept whole, not cut to the stored scale"
        );

        let native = Money::new(dec!(1_000_000_000.00));
        let converted = |rate: FxRate| {
            Valued::converted(native, rate)
                .expect("a non-zero rate divides")
                .rounded()
                .eur()
                .get()
        };

        let at_full_precision = converted(rate);
        assert_eq!(at_full_precision, dec!(5_783_142.90));
        // What the file states is reproduced exactly: native x quote.
        assert_eq!(at_full_precision, (native.get() * quote).round_dp(2));

        assert_ne!(converted(rate.rounded()), at_full_precision);

        let from_a_rounded_quote = rate_from_inverse_quote(FxRate::new(quote).rounded().get())
            .expect("a non-zero quote inverts");
        assert_eq!(converted(from_a_rounded_quote), dec!(5_783_000.00));
    }

    /// A conversion records the rate, where it came from, and the day it is the rate of
    /// [DOM-028, DOM-030, DOM-034].
    #[test]
    fn a_conversion_records_the_rate_its_source_and_its_date() {
        let friday = NaiveDate::from_ymd_opt(2024, 4, 26).expect("a valid date");
        let conversion = Conversion::new(
            Currency::new("usd"),
            FxRate::new(dec!(1.0414)),
            RateSource::Ecb,
            friday,
        );

        assert_eq!(conversion.currency().code(), "USD");
        assert_eq!(conversion.rate(), FxRate::new(dec!(1.0414)));
        assert_eq!(conversion.source(), RateSource::Ecb);
        assert_eq!(
            conversion.rate_date(),
            friday,
            "a weekend trade stores the Friday rate's own date [DOM-034]"
        );
    }

    /// The three sources are the whole set [DOM-030]: a fourth makes this match fail to
    /// compile.
    #[test]
    fn the_rate_source_is_a_closed_three_case_set() {
        let sources = [RateSource::Broker, RateSource::Ecb, RateSource::Native];

        for source in sources {
            match source {
                RateSource::Broker | RateSource::Ecb | RateSource::Native => {}
            }
        }

        assert_eq!(sources.len(), 3);
    }

    /// No currency gain is stored anywhere: the movement of the currency falls inside the
    /// securities gain [DOM-026].
    ///
    /// An absence has no runtime form, so this reads the two modules that model a transaction's
    /// money at compile time — `include_str!` is not I/O — and asserts that no field, method or
    /// variant names one. A field named for a currency, fx or exchange gain would fail here.
    #[test]
    fn the_model_reports_no_separate_currency_movement() {
        let modeled_money = [
            include_str!("valuation.rs"),
            include_str!("transaction.rs"),
            include_str!("decimal.rs"),
        ];

        // Assembled rather than written out, so that this test's own text is not the match it
        // is looking for.
        let forbidden: Vec<String> = ["currency", "fx", "exchange"]
            .iter()
            .map(|kind| format!("{kind}_gain"))
            .collect();

        for source in modeled_money {
            for name in &forbidden {
                assert!(
                    !source.contains(name),
                    "{name} would be a separately reported currency gain [DOM-026]"
                );
            }
        }
    }
}
