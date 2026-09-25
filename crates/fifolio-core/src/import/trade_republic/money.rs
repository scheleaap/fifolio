//! What a Trade Republic money column is worth: the native figures, the gross and the fees they
//! imply, and the foreign side a row states.
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
//! Three more carry the foreign side, populated where the row was in another currency
//! [IMP-TR-006]: `original_amount`, `original_currency` and `fx_rate`. Each is read on its own:
//! no requirement says what a row carrying some of them and not the others means, so none is
//! refused for it.
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
//! # `fx_rate` values nothing
//!
//! `fx_rate` was foreign units per EUR up to 2024-07-02 and its reciprocal from 2024-10-01 on,
//! so it has no convention a reader could apply (DEC-073). It is read where populated, verbatim
//! and as a bare decimal rather than an [`FxRate`](crate::decimal::FxRate), which would claim the
//! stored convention [DOM-086] for it. Nothing here converts with it: every figure above is the
//! row's own, in its settlement `currency`. Refusing a *stored* row that carries the triple is
//! IMP-TR-017's and needs the row's classification, so it is FIF-029's, not this module's.
//!
//! # A blank cell is an absent figure, not a zero
//!
//! Most money cells of most rows are blank: a `CASH` receipt states no `price`, and the
//! `TAX_EXCHANGE` pair states no price, no amount, no tax and no currency at all [IMP-TR-018].
//! So a blank is read as absent — [`Booked::gross`] and [`Booked::unit_price`] answer `None` —
//! and only a *populated* cell that is not a number refuses the file. A blank `fee` or `tax`
//! contributes nothing to the sum of IMP-TR-007, which is the same figure a zero would give;
//! the distinction is kept out of the sum rather than invented for it.

use rust_decimal::Decimal;

use super::{TradeRepublicError, UNIT_PRICE_COLUMN, field};
use crate::decimal::{Money, QuotedPrice};
use crate::import::reader::SourceRow;
use crate::valuation::Currency;

/// The cash movement, fee excluded [IMP-TR-016].
const AMOUNT: &str = "amount";

/// The broker's charge, negative [IMP-TR-004].
const FEE: &str = "fee";

/// Withholding, negative [IMP-TR-004].
const TAX: &str = "tax";

/// What the row's own figures are denominated in [IMP-TR-005].
const CURRENCY: &str = "currency";

/// The three columns of the foreign side [IMP-TR-006].
const ORIGINAL_AMOUNT: &str = "original_amount";
const ORIGINAL_CURRENCY: &str = "original_currency";
const FX_RATE: &str = "fx_rate";

/// A figure outside the range a decimal can hold [ARC-009].
const OVERFLOW: &str = "the figure is outside the range of a decimal";

/// The money columns of a Trade Republic row, as the file states them [IMP-TR-005],
/// [IMP-TR-006].
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
    original_amount: Option<Decimal>,
    original_currency: Option<Currency>,
    fx_rate: Option<Decimal>,
}

impl Booked {
    /// The money `row` states.
    ///
    /// # Errors
    ///
    /// When the row carries no such column, or a populated money cell is not a number.
    pub fn read(row: &SourceRow) -> Result<Self, TradeRepublicError> {
        Ok(Self {
            currency: text(row, CURRENCY)?.map(Currency::new),
            unit_price: amount(row, UNIT_PRICE_COLUMN)?.map(QuotedPrice::new),
            amount: amount(row, AMOUNT)?,
            fee: amount(row, FEE)?,
            tax: amount(row, TAX)?,
            original_amount: amount(row, ORIGINAL_AMOUNT)?,
            original_currency: text(row, ORIGINAL_CURRENCY)?.map(Currency::new),
            fx_rate: amount(row, FX_RATE)?,
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

    /// The traded value, fee excluded: `|amount|` [IMP-TR-016], in [`currency`](Self::currency).
    /// `None` on a row that states no amount.
    ///
    /// The magnitude, because the sign is the direction of the cash flow [IMP-TR-004] and the
    /// domain stores a gross as a positive figure.
    #[must_use]
    pub fn gross(&self) -> Option<Money> {
        self.amount.map(|amount| Money::new(amount.abs()))
    }

    /// The costs the row carried: `|fee| + |tax|`, in [`currency`](Self::currency)
    /// [IMP-TR-007], [IMP-TR-016].
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

    /// `original_amount` exactly as stated, cash-flow sign included [IMP-TR-004], [IMP-TR-006].
    #[must_use]
    pub fn original_amount(&self) -> Option<Decimal> {
        self.original_amount
    }

    /// `original_currency` as stated [IMP-TR-006].
    #[must_use]
    pub fn original_currency(&self) -> Option<&Currency> {
        self.original_currency.as_ref()
    }

    /// `fx_rate` exactly as stated [IMP-TR-006]. Its convention changed in late 2024, so it
    /// must never value anything (DEC-073); it is kept only as the file's own statement.
    #[must_use]
    pub fn fx_rate(&self) -> Option<Decimal> {
        self.fx_rate
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

    /// The sample buy: 35 at 75.09 for 2628.15, with a fee of 1.00 beside it.
    fn sample() -> Booked {
        Booked::read(&row(&[
            ("category", "TRADING"),
            ("type", "BUY"),
            ("shares", "35.0000000000"),
            (UNIT_PRICE_COLUMN, "75.090000"),
            (AMOUNT, "-2628.150000"),
            (FEE, "-1.00"),
            (CURRENCY, "EUR"),
        ]))
        .expect("the sample row's money reads")
    }

    /// A dividend as the fixtures state it: settled in EUR, with a USD foreign side.
    fn foreign_dividend() -> Booked {
        Booked::read(&row(&[
            ("category", "CASH"),
            ("type", "DIVIDEND"),
            (AMOUNT, "0.032946"),
            (TAX, "-0.02"),
            (CURRENCY, "EUR"),
            (ORIGINAL_AMOUNT, "0.05"),
            (ORIGINAL_CURRENCY, "USD"),
            (FX_RATE, "0.860751"),
        ]))
        .expect("the row's money reads")
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
            ("category", "TRADING"),
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
            ("category", "CORPORATE_ACTION"),
            ("type", "TAX_EXCHANGE"),
            ("shares", "-60.0000000000"),
        ]))
        .expect("the row's money reads");

        assert_eq!(booked.currency(), None);
        assert_eq!(booked.unit_price(), None);
        assert_eq!(booked.gross(), None);
        assert_eq!(booked.fees(), Ok(Money::zero()));
        assert_eq!(booked.original_amount(), None);
        assert_eq!(booked.original_currency(), None);
        assert_eq!(booked.fx_rate(), None);
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

    /// The foreign side is read as the file states it, `fx_rate` verbatim and never inverted
    /// or otherwise normalized: it has no reliable convention [IMP-TR-006], (DEC-073).
    #[test]
    fn the_original_columns_are_read_as_stated() {
        let booked = foreign_dividend();

        assert_eq!(booked.original_amount(), Some(dec!(0.05)));
        assert_eq!(booked.original_currency(), Some(&Currency::new("USD")));
        assert_eq!(booked.fx_rate(), Some(dec!(0.860751)));
    }

    /// A populated foreign side values nothing: the gross and the fees stay the settlement
    /// currency's own figures, and no product or quotient of `fx_rate` reaches them
    /// [IMP-TR-016], (DEC-073).
    #[test]
    fn a_foreign_side_leaves_the_settlement_figures_alone() {
        let booked = foreign_dividend();

        assert_eq!(booked.currency(), Some(&Currency::new("EUR")));
        assert_eq!(booked.gross(), Some(Money::new(dec!(0.032946))));
        assert_eq!(booked.fees(), Ok(Money::new(dec!(0.02))));
    }

    /// A foreign side populated in part is read as far as it goes: no requirement makes that
    /// shape a refusal [IMP-TR-006].
    #[test]
    fn a_partly_populated_foreign_side_reads_as_stated() {
        let booked = Booked::read(&row(&[
            (AMOUNT, "0.032946"),
            (CURRENCY, "EUR"),
            (ORIGINAL_AMOUNT, "0.04"),
        ]))
        .expect("the row's money reads");

        assert_eq!(booked.original_amount(), Some(dec!(0.04)));
        assert_eq!(booked.original_currency(), None);
        assert_eq!(booked.fx_rate(), None);
    }

    /// A populated `fx_rate` that is not a number is a parse failure like any other money cell
    /// [IMP-TR-006], (DEC-074).
    #[test]
    fn an_fx_rate_that_is_not_a_number_refuses_the_file() {
        let refusal = Booked::read(&row(&[(FX_RATE, "1,06"), (CURRENCY, "EUR")]))
            .expect_err("the rate is not a number");

        assert_eq!(
            refusal,
            TradeRepublicError::NotAnAmount {
                header: FX_RATE.to_owned(),
                value: "1,06".to_owned(),
            }
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
