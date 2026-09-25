//! What a Trade Republic money column is worth: the native figures, the gross and the fees they
//! imply, and the conversion a row states.
//!
//! Five columns carry the row's own money [IMP-TR-005]:
//!
//! | Column | Meaning |
//! | --- | --- |
//! | `price` | the unit price, as the statement quotes it [IMP-TR-022] |
//! | `amount` | the cash movement in the settlement currency, **excluding** the fee |
//! | `fee` | the broker's charge, negative |
//! | `tax` | withholding, negative |
//! | `currency` | what the four above are denominated in |
//!
//! Three more carry the foreign side, populated together when the row was in another currency
//! [IMP-TR-006]: `original_amount`, `original_currency` and `fx_rate`.
//!
//! # `amount` excludes the fee, the opposite of Saxo
//!
//! Saxo's `Boekingsbedrag` is the movement *including* costs, so its gross is the movement less
//! them on an acquisition and plus them on a disposal. Trade Republic states the two separately
//! [IMP-TR-016]:
//!
//! ```text
//! gross = |amount|
//! fees  = |fee| + |tax|          both in the settlement currency
//! ```
//!
//! Which is why nothing here takes a direction: with the fee outside the movement there is no
//! sign-dependent branch to get wrong. The sample buy is `35 × 75.09 = 2628.15`, which is
//! `|amount|` exactly, with its `fee` of −1.00 carried beside it and never subtracted from it.
//! Subtracting it would understate every buy's cost basis by the fee.
//!
//! # `fx_rate` is already the stored convention
//!
//! This crate stores foreign units per EUR, `EUR = native / rate` [DOM-086], and that is what
//! Trade Republic quotes — 1.06239 USD per EUR on the 2024-01-02 dividend. So the rate is used
//! as stated and [`rate_from_inverse_quote`](crate::valuation::rate_from_inverse_quote), which
//! the Saxo importer needs, is deliberately not called here: inverting an already-correct rate
//! would misvalue every foreign row by the square of the rate.
//!
//! # A foreign-currency trade refuses the file
//!
//! No `TRADING` row with `original_*` populated appears in four years of exports, so what the
//! `price` and `amount` of one would be denominated in is unspecified. Such a row **rejects the
//! import** rather than being read under a guess [IMP-TR-017], consistent with the unknown-type
//! rule: a wrong currency on a trade is a cost basis wrong by the exchange rate, permanently and
//! invisibly.
//!
//! That guard reads the `category` cell verbatim. It is not the row classification of
//! IMP-TR-008, which is FIF-029's and maps `category` and `type` together onto the domain's
//! three outcomes; it is the one literal the requirement names.
//!
//! # A blank cell is an absent figure, not a zero
//!
//! Most money cells of most rows are blank: a `CASH` receipt states no `price`, and the
//! `TAX_EXCHANGE` pair states no price, no amount, no tax and no currency at all [IMP-TR-018].
//! So a blank is read as absent — [`Booked::gross`] and [`Booked::unit_price`] answer `None` —
//! and only a *populated* cell that is not a number refuses the file. A blank `fee` or `tax`
//! contributes nothing to the sum of IMP-TR-007, which is the same figure a zero would give;
//! the distinction is kept out of the sum rather than invented for it.

use chrono::NaiveDate;
use rust_decimal::Decimal;

use super::{TradeRepublicError, UNIT_PRICE_COLUMN, field};
use crate::decimal::{FxRate, Money, QuotedPrice};
use crate::import::reader::SourceRow;
use crate::valuation::{Conversion, Currency, RateSource};

/// The cash movement, fee excluded [IMP-TR-016].
const AMOUNT: &str = "amount";

/// The broker's charge, negative [IMP-TR-004].
const FEE: &str = "fee";

/// Withholding, negative [IMP-TR-004].
const TAX: &str = "tax";

/// What the row's own figures are denominated in [IMP-TR-005].
const CURRENCY: &str = "currency";

/// The three columns of the foreign side, populated together [IMP-TR-006].
const ORIGINAL_AMOUNT: &str = "original_amount";
const ORIGINAL_CURRENCY: &str = "original_currency";
const FX_RATE: &str = "fx_rate";

/// The column whose value IMP-TR-017 names. Classifying a row is FIF-029's; this is the literal
/// the foreign-trade refusal is stated in terms of.
const CATEGORY: &str = "category";

/// The `category` value that makes a row a trade [IMP-TR-017].
const TRADE: &str = "TRADING";

/// A figure outside the range a decimal can hold [ARC-009].
const OVERFLOW: &str = "the figure is outside the range of a decimal";

/// A row denominated in a foreign currency that states no rate, so its EUR side is underivable
/// [IMP-TR-024].
const NO_RATE: &str = "the row is not in EUR and states no fx_rate, so no conversion exists";

/// The foreign side of a row, as the export states it [IMP-TR-006].
///
/// Held as a triple because the export populates it as one: a row carrying some of the three and
/// not the others is refused rather than half-read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Original {
    amount: Decimal,
    currency: Currency,
    rate: FxRate,
}

impl Original {
    /// The amount in the foreign currency, exactly as stated: the sign is the row's cash-flow
    /// sign [IMP-TR-004] and is left on it.
    #[must_use]
    pub fn amount(&self) -> Decimal {
        self.amount
    }

    /// The currency the foreign amount is denominated in.
    #[must_use]
    pub fn currency(&self) -> &Currency {
        &self.currency
    }

    /// `fx_rate`, foreign units per EUR, exactly as the file states it: the stored convention
    /// already [DOM-086], so it is never inverted.
    #[must_use]
    pub fn rate(&self) -> FxRate {
        self.rate
    }
}

/// The money columns of a Trade Republic row, as the file states them [IMP-TR-005].
///
/// Each is optional because the export leaves most of them blank on most rows; what each one
/// *means* is the accessors below, not the column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Booked {
    currency: Option<Currency>,
    unit_price: Option<QuotedPrice>,
    amount: Option<Decimal>,
    fee: Option<Decimal>,
    tax: Option<Decimal>,
    original: Option<Original>,
}

impl Booked {
    /// The money `row` states.
    ///
    /// # Errors
    ///
    /// When the row carries no such column, when a populated money cell is not a number, when
    /// the `original_*` triple is populated in part, or when a `TRADING` row states a foreign
    /// side at all [IMP-TR-017].
    pub fn read(row: &SourceRow) -> Result<Self, TradeRepublicError> {
        let original = original(row)?;
        if let Some(foreign) = &original
            && field(row, CATEGORY)? == TRADE
        {
            return Err(TradeRepublicError::ForeignCurrencyTrade {
                currency: foreign.currency.code().to_owned(),
                transaction_id: field(row, super::IDENTITY_COLUMN)?.to_owned(),
            });
        }

        Ok(Self {
            currency: text(row, CURRENCY)?.map(Currency::new),
            unit_price: amount(row, UNIT_PRICE_COLUMN)?.map(QuotedPrice::new),
            amount: amount(row, AMOUNT)?,
            fee: amount(row, FEE)?,
            tax: amount(row, TAX)?,
            original,
        })
    }

    /// What the row's own figures are denominated in, `None` where the row states no currency
    /// because it moves no money — the `TAX_EXCHANGE` pair [IMP-TR-018].
    #[must_use]
    pub fn currency(&self) -> Option<&Currency> {
        self.currency.as_ref()
    }

    /// The unit price as the statement quotes it [IMP-TR-022], `None` on a row that states
    /// none.
    ///
    /// A [`QuotedPrice`] and not an effective one: no quotation factor is applied here, and none
    /// has been divided out of it [DOM-087].
    #[must_use]
    pub fn unit_price(&self) -> Option<QuotedPrice> {
        self.unit_price
    }

    /// The traded value, fee excluded: `|amount|` [IMP-TR-016]. `None` on a row that states no
    /// amount.
    ///
    /// The magnitude, because the sign is the direction of the cash flow [IMP-TR-004] and the
    /// domain stores a gross as a positive figure.
    #[must_use]
    pub fn gross(&self) -> Option<Money> {
        self.amount.map(|amount| Money::new(amount.abs()))
    }

    /// The costs the row carried: `|fee| + |tax|`, in the settlement currency [IMP-TR-007],
    /// [IMP-TR-016].
    ///
    /// Zero where the row states neither, which is every observed trade's `tax` and every
    /// receipt's `fee`.
    ///
    /// # Errors
    ///
    /// When the sum leaves the range of a decimal [ARC-009].
    pub fn fees(&self) -> Result<Money, TradeRepublicError> {
        let magnitude = |figure: Option<Decimal>| figure.unwrap_or_default().abs();
        magnitude(self.fee)
            .checked_add(magnitude(self.tax))
            .map(Money::new)
            .ok_or(TradeRepublicError::UnderivableMoney { reason: OVERFLOW })
    }

    /// The foreign side of the row, where it has one [IMP-TR-006].
    #[must_use]
    pub fn original(&self) -> Option<&Original> {
        self.original.as_ref()
    }

    /// The conversion the row's figures were booked under [DOM-028].
    ///
    /// `fx_rate` is used verbatim, the file already quoting foreign units per EUR [DOM-086], and
    /// the source is `broker`: the export states both sides of the movement, so the rate records
    /// what was actually paid rather than converting anything [DOM-031]. A row stating no
    /// foreign side is in EUR and converts natively [DOM-033] — as does a row stating no
    /// currency at all, which moves no money to convert.
    ///
    /// # Errors
    ///
    /// When the row states a non-EUR `currency` and no rate [IMP-TR-024], a shape no export
    /// carries and that no requirement gives a rate for.
    pub fn conversion(&self, trade_date: NaiveDate) -> Result<Conversion, TradeRepublicError> {
        match &self.original {
            Some(foreign) => Ok(Conversion::new(
                foreign.currency.clone(),
                foreign.rate,
                RateSource::Broker,
                trade_date,
            )),
            None if self.currency.as_ref().is_none_or(Currency::is_eur) => {
                Ok(Conversion::native(trade_date))
            }
            None => Err(TradeRepublicError::UnderivableMoney { reason: NO_RATE }),
        }
    }
}

/// The `original_*` triple of `row`, `None` where all three are blank.
///
/// All three or none: the export populates them together [IMP-TR-006], so a row carrying two of
/// them is a shape neither `design/` nor the export describes, and reading it would put a rate
/// against an amount that is not the one it converts.
fn original(row: &SourceRow) -> Result<Option<Original>, TradeRepublicError> {
    let foreign = amount(row, ORIGINAL_AMOUNT)?;
    let currency = text(row, ORIGINAL_CURRENCY)?;
    let rate = amount(row, FX_RATE)?;

    match (foreign, currency, rate) {
        (None, None, None) => Ok(None),
        (Some(amount), Some(currency), Some(rate)) => Ok(Some(Original {
            amount,
            currency: Currency::new(currency),
            rate: FxRate::new(rate),
        })),
        (foreign, currency, rate) => Err(TradeRepublicError::PartialConversion {
            absent: [
                (ORIGINAL_AMOUNT, foreign.is_none()),
                (ORIGINAL_CURRENCY, currency.is_none()),
                (FX_RATE, rate.is_none()),
            ]
            .into_iter()
            .filter(|&(_, blank)| blank)
            .map(|(header, _)| header.to_owned())
            .collect(),
        }),
    }
}

/// One column of `row`, `None` where it is carried blank.
fn text<'a>(row: &'a SourceRow, header: &str) -> Result<Option<&'a str>, TradeRepublicError> {
    Ok(Some(field(row, header)?).filter(|value| !value.is_empty()))
}

/// One money column of `row` as a decimal, `None` where it is carried blank.
fn amount(row: &SourceRow, header: &str) -> Result<Option<Decimal>, TradeRepublicError> {
    text(row, header)?
        .map(|value| {
            value
                .parse::<Decimal>()
                .map_err(|_| TradeRepublicError::NotAnAmount {
                    header: header.to_owned(),
                    value: value.to_owned(),
                })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::super::tests::row;
    use super::*;
    use crate::decimal::Scaled as _;
    use rust_decimal_macros::dec;

    fn trade_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 5, 2).expect("a real date")
    }

    /// The sample buy: 35 at 75.09 for 2628.15, with a fee of 1.00 beside it.
    fn sample() -> Booked {
        Booked::read(&row(&[
            (CATEGORY, TRADE),
            ("type", "BUY"),
            ("shares", "35.0000000000"),
            (UNIT_PRICE_COLUMN, "75.090000"),
            (AMOUNT, "-2628.150000"),
            (FEE, "-1.00"),
            (CURRENCY, "EUR"),
        ]))
        .expect("the sample row's money reads")
    }

    /// The five native columns map onto the figures the domain stores [IMP-TR-005],
    /// [IMP-TR-022].
    #[test]
    fn the_native_columns_map_onto_the_domain_figures() {
        let booked = sample();

        assert_eq!(booked.currency(), Some(&Currency::new("EUR")));
        assert_eq!(booked.unit_price(), Some(QuotedPrice::new(dec!(75.09))));
        assert_eq!(booked.gross(), Some(Money::new(dec!(2628.15))));
        assert_eq!(booked.fees(), Ok(Money::new(dec!(1.00))));
    }

    /// `amount` excludes the fee, so the gross is the movement itself and the fee is carried
    /// separately: 35 × 75.09 = 2628.15 [IMP-TR-016].
    #[test]
    fn the_gross_is_the_movement_itself_and_the_fee_is_beside_it() {
        let booked = sample();

        let gross = booked.gross().expect("the sample states an amount");
        assert_eq!(gross.get(), dec!(35) * dec!(75.09));
        // Saxo's rule would give 2627.15 here, understating the cost basis by the fee.
        assert_ne!(gross.get(), dec!(2628.15) - dec!(1.00));
        assert_eq!(booked.fees(), Ok(Money::new(dec!(1.00))));
    }

    /// A sell states a positive `amount`, and the gross is its magnitude either way: `|amount|`,
    /// never `-amount`, which would make every disposal's proceeds negative [IMP-TR-016].
    #[test]
    fn the_gross_of_an_inflow_is_its_magnitude_too() {
        let booked = Booked::read(&row(&[
            (CATEGORY, TRADE),
            ("type", "SELL"),
            ("shares", "-35.0000000000"),
            (UNIT_PRICE_COLUMN, "75.090000"),
            (AMOUNT, "2628.150000"),
            (FEE, "-1.00"),
            (CURRENCY, "EUR"),
        ]))
        .expect("the row's money reads");

        assert_eq!(booked.gross(), Some(Money::new(dec!(2628.15))));
    }

    /// `fee` and `tax` are summed into the domain's single fees figure, as magnitudes
    /// [IMP-TR-007].
    #[test]
    fn the_fee_and_the_tax_sum_into_one_figure() {
        let booked = Booked::read(&row(&[
            (AMOUNT, "5.337365"),
            (FEE, "-0.25"),
            (TAX, "-1.66"),
            (CURRENCY, "EUR"),
        ]))
        .expect("the row's money reads");

        assert_eq!(booked.fees(), Ok(Money::new(dec!(1.91))));
    }

    /// A row stating neither a fee nor a tax carries no costs, not an unreadable file
    /// [IMP-TR-007].
    #[test]
    fn a_row_stating_no_fee_and_no_tax_carries_no_costs() {
        let booked = Booked::read(&row(&[(AMOUNT, "2869.800000"), (CURRENCY, "EUR")]))
            .expect("the row's money reads");

        assert_eq!(booked.fees(), Ok(Money::zero()));
    }

    /// The `TAX_EXCHANGE` pair states no price, no amount, no tax and no currency, and is read
    /// as stating them rather than as a file that cannot be read [IMP-TR-018].
    #[test]
    fn a_row_stating_no_money_at_all_reads_as_absent_figures() {
        let booked = Booked::read(&row(&[
            (CATEGORY, "CORPORATE_ACTION"),
            ("type", "TAX_EXCHANGE"),
            ("shares", "-60.0000000000"),
        ]))
        .expect("the row's money reads");

        assert_eq!(booked.currency(), None);
        assert_eq!(booked.unit_price(), None);
        assert_eq!(booked.gross(), None);
        assert_eq!(booked.fees(), Ok(Money::zero()));
        assert_eq!(booked.original(), None);
        // A row moving no money has nothing to convert, so it converts natively rather than
        // refusing the file for want of a currency [DOM-033].
        assert_eq!(
            booked.conversion(trade_date()),
            Ok(Conversion::native(trade_date()))
        );
    }

    /// A populated money cell that is not a number refuses the file, naming the column and the
    /// value [IMP-TR-005].
    #[test]
    fn a_money_cell_that_is_not_a_number_refuses_the_file() {
        let refusal = Booked::read(&row(&[(AMOUNT, "2.628,15"), (CURRENCY, "EUR")]))
            .expect_err("the amount is not a number");

        assert_eq!(
            refusal,
            TradeRepublicError::NotAnAmount {
                header: AMOUNT.to_owned(),
                value: "2.628,15".to_owned(),
            }
        );
    }

    /// The foreign side arrives as the triple the export populates together [IMP-TR-006].
    #[test]
    fn the_original_columns_arrive_as_one_triple() {
        let booked = Booked::read(&row(&[
            (CATEGORY, "CASH"),
            ("type", "DIVIDEND"),
            (AMOUNT, "0.032946"),
            (CURRENCY, "EUR"),
            (ORIGINAL_AMOUNT, "0.04"),
            (ORIGINAL_CURRENCY, "USD"),
            (FX_RATE, "1.062390"),
        ]))
        .expect("the row's money reads");

        let original = booked.original().expect("the row states a foreign side");
        assert_eq!(original.amount(), dec!(0.04));
        assert_eq!(original.currency(), &Currency::new("USD"));
    }

    /// `fx_rate` is foreign units per EUR already, so it is stored as stated and never inverted
    /// [IMP-TR-006], [DOM-086].
    #[test]
    fn the_fx_rate_is_stored_without_inversion() {
        let booked = Booked::read(&row(&[
            (CATEGORY, "CASH"),
            ("type", "DIVIDEND"),
            (AMOUNT, "0.032946"),
            (CURRENCY, "EUR"),
            (ORIGINAL_AMOUNT, "0.04"),
            (ORIGINAL_CURRENCY, "USD"),
            (FX_RATE, "1.062390"),
        ]))
        .expect("the row's money reads");

        let conversion = booked
            .conversion(trade_date())
            .expect("the row states a rate");
        assert_eq!(conversion.rate(), FxRate::new(dec!(1.062390)));
        // The inverse, 0.941345…, is what a Saxo-style inversion would have stored.
        assert_ne!(conversion.rate().get(), Decimal::ONE / dec!(1.062390));
        assert_eq!(conversion.currency(), &Currency::new("USD"));
        assert_eq!(conversion.source(), RateSource::Broker);
        assert_eq!(conversion.rate_date(), trade_date());
    }

    /// A row in EUR converts natively: rate 1, source `native` [DOM-033].
    #[test]
    fn a_row_in_euros_converts_natively() {
        let conversion = sample()
            .conversion(trade_date())
            .expect("an EUR row converts");

        assert_eq!(conversion, Conversion::native(trade_date()));
    }

    /// A `TRADING` row stating a foreign side rejects the import, naming the currency and the
    /// row [IMP-TR-017].
    #[test]
    fn a_foreign_currency_trade_rejects_the_import() {
        let refusal = Booked::read(&row(&[
            (CATEGORY, TRADE),
            ("type", "BUY"),
            ("shares", "35.0000000000"),
            (UNIT_PRICE_COLUMN, "75.090000"),
            (AMOUNT, "-2628.150000"),
            (FEE, "-1.00"),
            (CURRENCY, "EUR"),
            (ORIGINAL_AMOUNT, "-2800.00"),
            (ORIGINAL_CURRENCY, "USD"),
            (FX_RATE, "1.062390"),
            ("transaction_id", "bf751ce3-33c9-539c-96d7-1428cc7bdde9"),
        ]))
        .expect_err("a foreign-currency trade is unspecified");

        assert_eq!(
            refusal,
            TradeRepublicError::ForeignCurrencyTrade {
                currency: "USD".to_owned(),
                transaction_id: "bf751ce3-33c9-539c-96d7-1428cc7bdde9".to_owned(),
            }
        );
    }

    /// The refusal keys on `category`, not on `type`: a `SELL` is refused as a `BUY` is, while
    /// the same triple on a `CORPORATE_ACTION` row reads [IMP-TR-017].
    #[test]
    fn the_refusal_keys_on_the_category_and_not_on_the_type() {
        let foreign = |category: &str, kind: &str| {
            Booked::read(&row(&[
                (CATEGORY, category),
                ("type", kind),
                (AMOUNT, "2628.150000"),
                (CURRENCY, "EUR"),
                (ORIGINAL_AMOUNT, "2800.00"),
                (ORIGINAL_CURRENCY, "USD"),
                (FX_RATE, "1.062390"),
                ("transaction_id", "bf751ce3-33c9-539c-96d7-1428cc7bdde9"),
            ]))
        };

        assert_eq!(
            foreign(TRADE, "SELL").expect_err("a foreign-currency sell is unspecified"),
            TradeRepublicError::ForeignCurrencyTrade {
                currency: "USD".to_owned(),
                transaction_id: "bf751ce3-33c9-539c-96d7-1428cc7bdde9".to_owned(),
            }
        );
        assert!(
            foreign("CORPORATE_ACTION", "TAX_EXCHANGE")
                .expect("only a trade is refused")
                .original()
                .is_some()
        );
    }

    /// A partly populated `original_*` triple refuses the file, naming what is absent: the
    /// export populates the three together [IMP-TR-006].
    #[test]
    fn a_partly_populated_conversion_refuses_the_file() {
        let refusal = Booked::read(&row(&[
            (CATEGORY, "CASH"),
            ("type", "DIVIDEND"),
            (AMOUNT, "0.032946"),
            (CURRENCY, "EUR"),
            (ORIGINAL_AMOUNT, "0.04"),
        ]))
        .expect_err("two of the three columns are blank");

        assert_eq!(
            refusal,
            TradeRepublicError::PartialConversion {
                absent: vec![ORIGINAL_CURRENCY.to_owned(), FX_RATE.to_owned()],
            }
        );
    }

    /// The refusal names the *blank* columns, so a row truncated after two of the three reports
    /// the one it lacks and not the two it carries [IMP-TR-006].
    #[test]
    fn a_conversion_missing_only_the_rate_names_that_one_column() {
        let refusal = Booked::read(&row(&[
            (CATEGORY, "CASH"),
            ("type", "DIVIDEND"),
            (AMOUNT, "0.032946"),
            (CURRENCY, "EUR"),
            (ORIGINAL_AMOUNT, "0.04"),
            (ORIGINAL_CURRENCY, "USD"),
        ]))
        .expect_err("the rate is blank");

        assert_eq!(
            refusal,
            TradeRepublicError::PartialConversion {
                absent: vec![FX_RATE.to_owned()],
            }
        );
    }

    /// A row denominated in a foreign currency that states no rate has no EUR side, and is
    /// reported rather than converted at a rate of 1. No export carries this shape
    /// [IMP-TR-024].
    #[test]
    fn a_foreign_row_without_a_rate_has_no_conversion() {
        let booked = Booked::read(&row(&[(AMOUNT, "100.00"), (CURRENCY, "USD")]))
            .expect("the row's money reads");

        assert_eq!(
            booked.conversion(trade_date()),
            Err(TradeRepublicError::UnderivableMoney { reason: NO_RATE })
        );
    }

    /// The fees of two figures that cannot be added are reported, not wrapped [ARC-009].
    #[test]
    fn a_fee_sum_outside_the_range_of_a_decimal_is_reported() {
        let booked = Booked::read(&row(&[
            (FEE, &Decimal::MAX.to_string()),
            (TAX, &Decimal::MAX.to_string()),
            (CURRENCY, "EUR"),
        ]))
        .expect("the row's money reads");

        assert_eq!(
            booked.fees(),
            Err(TradeRepublicError::UnderivableMoney { reason: OVERFLOW })
        );
    }

    /// A caller naming a column of another format is told which one, rather than reading a
    /// blank as an absent figure.
    #[test]
    fn a_column_the_row_does_not_carry_is_named() {
        let refusal = amount(&row(&[]), "Boekingsbedrag").expect_err("the column is Saxo's");

        assert_eq!(
            refusal,
            TradeRepublicError::MissingColumn {
                header: "Boekingsbedrag".to_owned(),
            }
        );
    }
}
