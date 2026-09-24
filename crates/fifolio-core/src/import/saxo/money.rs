//! The money a `Transacties` row books, and the figures a transaction stores derived from it.
//!
//! Four columns carry it [IMP-SAXO-009], [IMP-SAXO-010]:
//!
//! | Column | Meaning |
//! | --- | --- |
//! | `Boekingsbedrag` | the cash movement in the native currency (`_Valuta`), **including** costs |
//! | `Aantal` | the same movement in EUR |
//! | `Totale kosten` | the costs, **in EUR**, always negative |
//! | `Omrekeningskoers` | the native-to-EUR multiplier, whose **reciprocal** is the stored rate |
//!
//! `Aantal` is never a quantity, whatever its name says [IMP-SAXO-009]: it is the euro side of
//! the movement. The quantity is a `_Transacties` column and reaches [`Booked::derive`] as an
//! argument, which is the mechanical form of that rule — nothing here can read a share count off
//! the row.
//!
//! # The derivation is sign-aware
//!
//! `Aantal` is a cash movement, so it is gross *plus* costs when cash goes out and gross *minus*
//! costs when cash comes in:
//!
//! ```text
//! EUR gross = |Aantal| − |Totale kosten|   on an acquisition
//! EUR gross = |Aantal| + |Totale kosten|   on a disposal
//! EUR fees  = |Totale kosten|
//! EUR price = EUR gross / (quantity × factor)
//! ```
//!
//! One formula for both directions understates every disposal's proceeds by twice the fee, which
//! is what DEC-035 corrected [IMP-SAXO-030]. [`Direction`] is therefore an argument and not
//! something inferred from the sign of a cell: a zero movement has no sign to read.
//!
//! The native figures follow the same shape, with the costs converted back from EUR using the
//! file's own `Omrekeningskoers` and **not** the stored rate, which is its reciprocal
//! [IMP-SAXO-030].
//!
//! # Dividing by the factor is what keeps the price the quoted one
//!
//! The domain values a trade as `quantity × unit_price × factor`, so an importer that stored
//! `gross / quantity` would have the factor applied twice — a hundredfold error in a bond's cost
//! basis [IMP-SAXO-023], [DEC-035]. Dividing by `quantity × factor` here is what makes the
//! result a [`QuotedPrice`]: the price the statement shows, `139.46` and not `1.3946`
//! [DOM-039].
//!
//! The factor itself arrives as an argument. Which factor a security takes, and where the
//! domain's trade value invokes it, is ARC-008 and DOM-038 — undecided, OQ-010, and FIF-075's —
//! so nothing here chooses between 1 and 0.01.
//!
//! # Full precision, including the subtraction
//!
//! Nothing here rounds: the quotients and the difference that consumes them keep every digit,
//! and rounding happens at the storage boundary [ARC-009]. The visible consequence is that a
//! stored native unit price need not equal the figure the statement prints — the sample buy
//! derives `5.750036` against a printed `5.75`, exactly as the sample sell derives `30.654`
//! against a printed `30.65` [IMP-SAXO-032]. The printed price is a rounded display; the derived
//! one is what the booked amounts imply, and it is the authoritative one. DEC-059 settled that
//! against DEC-054, which had rounded the native fee first so the printed figure came back.
//!
//! # The conversion
//!
//! Every Saxo row books both halves of its movement, so the rate precedence of [`crate::fx`]
//! can only land on two of its three answers: `native` for a row already in EUR [DOM-033], and
//! `broker` for any other, the file's own EUR figures being what was actually paid [DOM-031].
//! The ECB branch is unreachable for this format, which is why [`Booked::conversion`] builds the
//! [`Conversion`] itself rather than taking a rate table it could never consult.

use chrono::NaiveDate;
use rust_decimal::Decimal;

use super::super::reader::SourceRow;
use super::{SaxoError, field};
use crate::decimal::{Money, Quantity, QuotedPrice, Scaled as _};
use crate::valuation::{Conversion, Currency, RateSource, Valued, rate_from_inverse_quote};

/// The native cash movement, costs included [IMP-SAXO-010].
const NATIVE_MOVEMENT: &str = "Boekingsbedrag";

/// The same movement in EUR. Never a quantity [IMP-SAXO-009].
const EUR_MOVEMENT: &str = "Aantal";

/// The costs, in EUR and negative [IMP-SAXO-010].
const EUR_COSTS: &str = "Totale kosten";

/// The native-to-EUR multiplier [IMP-SAXO-029].
const QUOTE: &str = "Omrekeningskoers";

/// The currency `Boekingsbedrag` is denominated in. `Valuta` is the booking currency, which is
/// EUR on every row of the sample, and is not this.
const NATIVE_CURRENCY: &str = "_Valuta";

/// A row whose `Omrekeningskoers` is zero: neither the stored rate nor the native fee exists.
const ZERO_QUOTE: &str = "Omrekeningskoers is zero, so no rate and no native cost exist";

/// A price asked for over nothing: a zero quantity, or a zero quotation factor.
const NO_DIVISOR: &str = "the quantity times the quotation factor is zero, so no unit price exists";

/// A figure outside the range a decimal can hold [ARC-009].
const OVERFLOW: &str = "the figure is outside the range of a decimal";

/// Which way the cash moved, which decides whether the costs are inside the gross or outside it
/// [IMP-SAXO-030].
///
/// Taken from the row's classification — `Trade Event Type` on `_Transacties`, the `Acties`
/// label as a fallback [IMP-SAXO-038] — and never from the sign of a money cell: a movement of
/// zero, which every `Deponering` books, carries no sign to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Cash out: the movement includes the costs, so the gross is the movement less them.
    Acquisition,
    /// Cash in: the movement is net of the costs, so the gross is the movement plus them.
    Disposal,
}

/// The four money columns of a `Transacties` row, as the file states them [IMP-SAXO-010].
///
/// Held as bare decimals rather than as [`Money`]: they are the file's own figures, and what
/// each one *is* — a gross, a fee, a rate — is the derivation's answer and not the column's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Booked {
    currency: Currency,
    native_movement: Decimal,
    eur_movement: Decimal,
    eur_costs: Decimal,
    quote: Decimal,
}

/// What a row's money derives to: the figures a transaction stores [DOM-028], [DOM-085].
///
/// Unrounded, like every intermediate [ARC-009]. The caller rounds at the storage boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedMoney {
    gross: Valued<Money>,
    fees: Valued<Money>,
    unit_price: Valued<QuotedPrice>,
    conversion: Conversion,
}

impl DerivedMoney {
    /// The traded value, costs excluded, in both currencies [DOM-085].
    #[must_use]
    pub fn gross(&self) -> Valued<Money> {
        self.gross
    }

    /// The costs the movement carried, as a positive amount in both currencies.
    #[must_use]
    pub fn fees(&self) -> Valued<Money> {
        self.fees
    }

    /// The unit price the booked amounts imply, quoted as the statement quotes it
    /// [IMP-SAXO-023], [DOM-039].
    #[must_use]
    pub fn unit_price(&self) -> Valued<QuotedPrice> {
        self.unit_price
    }

    /// The rate the EUR figures were obtained under, its source and its date [DOM-028].
    #[must_use]
    pub fn conversion(&self) -> &Conversion {
        &self.conversion
    }
}

impl Booked {
    /// The money columns of `row`.
    ///
    /// # Errors
    ///
    /// When the row carries no such column, when it names no native currency, or when a money
    /// column holds something that is not a number. A blank money cell is refused rather than
    /// read as zero: every row of the sample populates all four, so a blank one is a shape
    /// neither `design/` nor the export describes, and reading it as zero would book a movement
    /// of nothing rather than report that the file was not understood.
    pub fn read(row: &SourceRow) -> Result<Self, SaxoError> {
        let currency = field(row, NATIVE_CURRENCY).ok_or_else(|| SaxoError::MissingColumn {
            header: NATIVE_CURRENCY.to_owned(),
        })?;
        if currency.is_empty() {
            return Err(SaxoError::NoCurrency);
        }

        Ok(Self {
            currency: Currency::new(currency),
            native_movement: amount(row, NATIVE_MOVEMENT)?,
            eur_movement: amount(row, EUR_MOVEMENT)?,
            eur_costs: amount(row, EUR_COSTS)?,
            quote: amount(row, QUOTE)?,
        })
    }

    /// The currency the native figures are denominated in, from `_Valuta`.
    #[must_use]
    pub fn currency(&self) -> &Currency {
        &self.currency
    }

    /// The conversion the row's EUR figures were booked under [DOM-030], [IMP-SAXO-029].
    ///
    /// The stored rate is the **reciprocal** of `Omrekeningskoers`, because this crate stores
    /// foreign units per EUR and Saxo quotes the inverse [DOM-086]. It is taken at full
    /// precision, never from a rounded quote (DEC-027).
    ///
    /// # Errors
    ///
    /// When the quote is zero, so no reciprocal exists.
    pub fn conversion(&self, trade_date: NaiveDate) -> Result<Conversion, SaxoError> {
        if self.currency.is_eur() {
            return Ok(Conversion::native(trade_date));
        }

        let rate = rate_from_inverse_quote(self.quote)
            .ok_or(SaxoError::UnderivableMoney { reason: ZERO_QUOTE })?;
        Ok(Conversion::new(
            self.currency.clone(),
            rate,
            // The file states what was actually paid in EUR, so the rate is informational
            // [DOM-031].
            RateSource::Broker,
            trade_date,
        ))
    }

    /// The gross, the fees and the unit price this row implies, in both currencies.
    ///
    /// `quantity` and `direction` come from the row's `_Transacties` counterpart
    /// [IMP-SAXO-038]; `factor` is the security's quotation factor, 1 per unit or 0.01 percent
    /// of par, which FIF-075 owns and this function only applies — exactly once, as the divisor
    /// of the price [IMP-SAXO-023].
    ///
    /// `Traded Quantity` is **signed**, so this takes the quantity's magnitude, exactly as it
    /// takes the magnitude of the money columns: [`Direction`] is what says which way the trade
    /// went, and a second sign carrying the same fact could only contradict it. Passing the
    /// column through unchanged is therefore safe — `-60` on the sample sell derives the same
    /// price as `60`. `design/` states no convention for this, so it is chosen here.
    ///
    /// # Errors
    ///
    /// When the quote is zero, when `quantity × factor` is zero so no unit price exists, or
    /// when a figure leaves the range of a decimal.
    pub fn derive(
        &self,
        direction: Direction,
        quantity: Quantity,
        factor: Decimal,
        trade_date: NaiveDate,
    ) -> Result<DerivedMoney, SaxoError> {
        let eur_fees = self.eur_costs.abs();
        // Converted with the file's own quote, not with the stored rate: the two are
        // reciprocals, so using the rate here would multiply where it must divide
        // [IMP-SAXO-030].
        // An EUR row's native side *is* its EUR side [DOM-033], so there is nothing to convert
        // and the quote is not consulted.
        let native_fees = if self.currency.is_eur() {
            eur_fees
        } else {
            eur_fees
                .checked_div(self.quote)
                .ok_or(SaxoError::UnderivableMoney { reason: ZERO_QUOTE })?
        };

        let eur_gross = gross(self.eur_movement, eur_fees, direction)?;
        let native_gross = gross(self.native_movement, native_fees, direction)?;

        let divisor = quantity
            .get()
            .abs()
            .checked_mul(factor)
            .ok_or(SaxoError::UnderivableMoney { reason: OVERFLOW })?;
        if divisor.is_zero() {
            return Err(SaxoError::UnderivableMoney { reason: NO_DIVISOR });
        }
        let price = |gross: Decimal| {
            gross
                .checked_div(divisor)
                .map(QuotedPrice::new)
                // The divisor is non-zero, so the only failure left is range [ARC-009].
                .ok_or(SaxoError::UnderivableMoney { reason: OVERFLOW })
        };

        Ok(DerivedMoney {
            gross: Valued::new(Money::new(native_gross), Money::new(eur_gross)),
            fees: Valued::new(Money::new(native_fees), Money::new(eur_fees)),
            unit_price: Valued::new(price(native_gross)?, price(eur_gross)?),
            conversion: self.conversion(trade_date)?,
        })
    }
}

/// The traded value a cash movement implies: the movement less its costs when cash went out,
/// plus them when cash came in [IMP-SAXO-030].
fn gross(movement: Decimal, fees: Decimal, direction: Direction) -> Result<Decimal, SaxoError> {
    let magnitude = movement.abs();
    match direction {
        Direction::Acquisition => magnitude.checked_sub(fees),
        Direction::Disposal => magnitude.checked_add(fees),
    }
    .ok_or(SaxoError::UnderivableMoney { reason: OVERFLOW })
}

/// One money column of `row` as a decimal.
fn amount(row: &SourceRow, header: &str) -> Result<Decimal, SaxoError> {
    let value = field(row, header).ok_or_else(|| SaxoError::MissingColumn {
        header: header.to_owned(),
    })?;
    value
        .parse::<Decimal>()
        .map_err(|_| SaxoError::NotAnAmount {
            header: header.to_owned(),
            value: value.to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decimal::QUANTITY_SCALE;
    use crate::import::saxo::Sheet;
    use rust_decimal_macros::dec;

    /// A `Transacties` row spelled as the file spells it, with only the named columns
    /// populated.
    fn transacties_row(values: &[(&str, &str)]) -> SourceRow {
        let columns = Sheet::Transacties
            .headers()
            .iter()
            .map(|header| {
                let value = values
                    .iter()
                    .find(|(name, _)| name.split_whitespace().eq(header.split_whitespace()))
                    .map_or("", |(_, value)| *value);
                ((*header).to_owned(), value.to_owned())
            })
            .collect();
        SourceRow::new(columns, "")
    }

    /// The one buy of the worked example in `importers.md`: `Koop 40 @ 5.75 USD`.
    fn sample_buy() -> Booked {
        Booked::read(&transacties_row(&[
            ("_Valuta", "USD"),
            ("Boekingsbedrag", "-238.00"),
            ("Aantal", "-216.92"),
            ("Totale kosten", "-7.29"),
            ("Omrekeningskoers", "0.911413"),
        ]))
        .expect("the sample buy reads")
    }

    /// The one disposal of the worked example: `Verkoop -60 @ 30.65 EUR`, booked in EUR.
    fn sample_sell() -> Booked {
        Booked::read(&transacties_row(&[
            ("_Valuta", "EUR"),
            ("Boekingsbedrag", "1833.24"),
            ("Aantal", "1833.24"),
            ("Totale kosten", "-6.00"),
            ("Omrekeningskoers", "1"),
        ]))
        .expect("the sample sell reads")
    }

    fn trade_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2023, 10, 2).expect("a valid date")
    }

    /// Per unit, which is every worked example but the bond's [IMP-SAXO-023].
    const PER_UNIT: Decimal = Decimal::ONE;

    fn derived(booked: &Booked, direction: Direction, quantity: Decimal) -> DerivedMoney {
        booked
            .derive(direction, Quantity::new(quantity), PER_UNIT, trade_date())
            .expect("the sample derives")
    }

    /// The worked buy: `|Aantal| − |Totale kosten|` is the gross, and the price it implies is
    /// the one `importers.md` states [IMP-SAXO-010], [IMP-SAXO-030], [DOM-039].
    #[test]
    fn the_sample_buy_derives_its_eur_figures() {
        let money = derived(&sample_buy(), Direction::Acquisition, dec!(40));

        assert_eq!(money.gross().eur().rounded().get(), dec!(209.63));
        assert_eq!(money.fees().eur().rounded().get(), dec!(7.29));
        assert_eq!(money.unit_price().eur().rounded().get(), dec!(5.240750));
        // The conversion the derivation embeds is the row's own, at the trade date
        // [IMP-SAXO-029], [DOM-028], [DOM-031].
        assert_eq!(money.conversion().source(), RateSource::Broker);
        assert_eq!(money.conversion().rate_date(), trade_date());
        assert_eq!(money.conversion().currency(), &Currency::new("USD"));
    }

    /// The same buy's native side: the costs converted back with `Omrekeningskoers`, giving the
    /// 8.00 USD and the 230.00 = 40 × 5.75 of the worked example [IMP-SAXO-030].
    #[test]
    fn the_sample_buy_derives_its_native_figures() {
        let money = derived(&sample_buy(), Direction::Acquisition, dec!(40));

        assert_eq!(money.fees().native().rounded().get(), dec!(8.00));
        assert_eq!(money.gross().native().rounded().get(), dec!(230.00));
    }

    /// The subtraction keeps full precision, so the derived native price is what the booked
    /// amounts imply and not the `5.75` the statement prints [IMP-SAXO-032], [ARC-009],
    /// [DEC-059].
    #[test]
    fn the_native_price_is_what_the_booked_amounts_imply() {
        let money = derived(&sample_buy(), Direction::Acquisition, dec!(40));

        // What DEC-054 would have produced, and DEC-059 reversed: rounding the native fee to the
        // money scale first gives (238.00 − 8.00) / 40 = 5.75, the printed figure, and loses the
        // derived one. This assertion is what distinguishes the two.
        assert_eq!(money.unit_price().native().rounded().get(), dec!(5.750036));
    }

    /// The worked disposal: `|Aantal| + |Totale kosten|`, and the price the booked amounts imply
    /// differs from the label's rounded `30.65` [IMP-SAXO-010], [IMP-SAXO-030], [DOM-039].
    #[test]
    fn the_sample_sell_derives_the_price_its_booked_amounts_imply() {
        let money = derived(&sample_sell(), Direction::Disposal, dec!(60));

        assert_eq!(money.gross().eur().rounded().get(), dec!(1839.24));
        assert_eq!(money.fees().eur().rounded().get(), dec!(6.00));
        assert_eq!(money.unit_price().eur().rounded().get(), dec!(30.654));
        assert_ne!(money.unit_price().eur().rounded().get(), dec!(30.65));
    }

    /// `Traded Quantity` is signed, and the sample sell's is `-60`: the magnitude is what divides
    /// the price, so wiring the column straight through cannot produce a negative price and a
    /// negative cost basis [IMP-SAXO-010], [IMP-SAXO-038].
    #[test]
    fn a_signed_quantity_derives_the_same_price_as_its_magnitude() {
        let sell = sample_sell();
        let signed = derived(&sell, Direction::Disposal, dec!(-60));
        let magnitude = derived(&sell, Direction::Disposal, dec!(60));

        assert_eq!(signed.unit_price().eur().rounded().get(), dec!(30.654));
        assert_eq!(signed, magnitude);
    }

    /// One formula for both directions understates a disposal by twice the fee, which is why
    /// the direction is an argument [IMP-SAXO-030], [DEC-035].
    #[test]
    fn the_two_directions_differ_by_twice_the_fee() {
        let sell = sample_sell();
        let as_disposal = derived(&sell, Direction::Disposal, dec!(60));
        let as_acquisition = derived(&sell, Direction::Acquisition, dec!(60));

        let difference = as_disposal.gross().eur().get() - as_acquisition.gross().eur().get();
        assert_eq!(difference, dec!(12.00));
        assert_eq!(difference, as_disposal.fees().eur().get() * dec!(2));
    }

    /// A foreign disposal takes the native fee — converted with `Omrekeningskoers` — on the
    /// same side as the EUR one, so its two grosses differ [IMP-SAXO-030].
    #[test]
    fn a_foreign_disposal_adds_the_converted_fee_to_the_native_movement() {
        let money = derived(&sample_buy(), Direction::Disposal, dec!(40));

        // 7.29 / 0.911413 = 7.99857, so 238.00 + 7.99857 rounds to 246.00, against an EUR
        // gross of 216.92 + 7.29. Passing the EUR fee, or the stored rate, would give 245.29
        // or 246.65.
        assert_eq!(money.fees().native().rounded().get(), dec!(8.00));
        assert_eq!(money.gross().native().rounded().get(), dec!(246.00));
        assert_eq!(money.gross().eur().rounded().get(), dec!(224.21));
        assert_ne!(money.gross().native(), money.gross().eur());
    }

    /// A movement of zero carries no sign to read, which is why the direction is an argument:
    /// the gross is the fee, and the two directions put it on opposite sides [IMP-SAXO-030].
    ///
    /// Synthetic. `Deponering` is the label that books a zero movement, but its gross comes from
    /// `Verhandelde waarde` (FIF-024, IMP-SAXO-039), so neither figure below is a claim about
    /// what a `Deponering` derives to; what is asserted is the direction rule alone.
    #[test]
    fn a_zero_movement_derives_its_gross_from_the_direction_alone() {
        let row = transacties_row(&[
            ("_Valuta", "EUR"),
            ("Boekingsbedrag", "0"),
            ("Aantal", "0"),
            ("Totale kosten", "-3.50"),
            ("Omrekeningskoers", "1"),
        ]);
        let booked = Booked::read(&row).expect("the row reads");

        let acquisition = derived(&booked, Direction::Acquisition, dec!(1));
        let disposal = derived(&booked, Direction::Disposal, dec!(1));

        assert_eq!(disposal.gross().eur().get(), disposal.fees().eur().get());
        assert_eq!(
            disposal.gross().native().get(),
            disposal.fees().native().get()
        );
        assert_eq!(
            acquisition.gross().eur().get(),
            -disposal.gross().eur().get()
        );
        assert_eq!(
            acquisition.gross().native().get(),
            -disposal.gross().native().get()
        );
    }

    /// An EUR row needs no conversion for its native side, so its quote is not consulted —
    /// a zero one included [DOM-033].
    #[test]
    fn an_eur_row_takes_its_native_fee_as_the_eur_fee() {
        let row = transacties_row(&[
            ("_Valuta", "EUR"),
            ("Boekingsbedrag", "1833.24"),
            ("Aantal", "1833.24"),
            ("Totale kosten", "-6.00"),
            ("Omrekeningskoers", "0"),
        ]);
        let booked = Booked::read(&row).expect("the row reads");

        let money = derived(&booked, Direction::Disposal, dec!(60));

        assert_eq!(money.fees().native(), money.fees().eur());
        assert_eq!(money.gross().native().rounded().get(), dec!(1839.24));
    }

    /// A fractional quantity at full scale still meets the factor exactly once, so
    /// `quantity × price × factor` rebuilds the gross [IMP-SAXO-023], [TST-016].
    #[test]
    fn a_full_scale_fractional_quantity_divides_the_price_exactly() {
        let row = transacties_row(&[
            ("_Valuta", "EUR"),
            ("Boekingsbedrag", "4183.80"),
            ("Aantal", "4183.80"),
            ("Totale kosten", "0"),
            ("Omrekeningskoers", "1"),
        ]);
        let booked = Booked::read(&row).expect("the row reads");
        // 8 decimals, the full quantity scale, and a divisor the gross divides by exactly.
        let quantity = Quantity::new(dec!(0.00390625));
        assert_eq!(quantity.get().scale(), QUANTITY_SCALE);

        let money = booked
            .derive(Direction::Acquisition, quantity, dec!(0.01), trade_date())
            .expect("it derives");

        assert_eq!(money.unit_price().eur().get(), dec!(107105280));
        assert_eq!(
            money.unit_price().eur().get() * quantity.get() * dec!(0.01),
            money.gross().eur().get()
        );
    }

    /// A figure past the range a decimal holds is reported as an overflow rather than as some
    /// other refusal [ARC-009].
    #[test]
    fn a_figure_past_the_decimal_range_is_refused_as_an_overflow() {
        // The divisor itself leaves the range, and — with a divisor small enough but non-zero —
        // the quotient does, which is not the zero divisor of `NO_DIVISOR`.
        let cases = [
            (Decimal::MAX, dec!(2)),
            (dec!(0.00000001), dec!(0.00000000000000000001)),
        ];

        for (quantity, factor) in cases {
            let error = sample_buy()
                .derive(
                    Direction::Acquisition,
                    Quantity::new(quantity),
                    factor,
                    trade_date(),
                )
                .expect_err("the figure leaves the range");

            assert_eq!(error, SaxoError::UnderivableMoney { reason: OVERFLOW });
        }
    }

    /// An EUR row books one set of figures, so both halves of every pair are the one figure
    /// [DOM-029], [DOM-033].
    #[test]
    fn an_eur_row_values_at_one_to_one() {
        let money = derived(&sample_sell(), Direction::Disposal, dec!(60));

        assert_eq!(money.gross().native(), money.gross().eur());
        assert_eq!(money.fees().native(), money.fees().eur());
        assert_eq!(money.unit_price().native(), money.unit_price().eur());
    }

    /// The stored rate is the reciprocal of `Omrekeningskoers`, not the quote itself
    /// [IMP-SAXO-029], [DOM-086].
    #[test]
    fn the_stored_rate_is_the_reciprocal_of_the_quote() {
        let conversion = sample_buy()
            .conversion(trade_date())
            .expect("a non-zero quote inverts");

        assert_eq!(conversion.rate().rounded().get(), dec!(1.097197));
        assert_ne!(conversion.rate().rounded().get(), dec!(0.911413));
        // Unrounded before the storage boundary, so converting with it keeps every digit
        // (DEC-027).
        assert!(conversion.rate().get().scale() > 6);
    }

    /// The native cost is the EUR cost divided by the quote. Dividing by the stored rate
    /// instead — multiplying by the quote — gives 6.64 where the statement shows 8.00
    /// [IMP-SAXO-030].
    #[test]
    fn the_native_cost_uses_the_quote_and_not_the_stored_rate() {
        let buy = sample_buy();
        let money = derived(&buy, Direction::Acquisition, dec!(40));
        let rate = buy
            .conversion(trade_date())
            .expect("a non-zero quote inverts")
            .rate();

        let with_the_rate = Money::new(dec!(7.29) / rate.get());
        assert_eq!(money.fees().native().rounded().get(), dec!(8.00));
        assert_eq!(with_the_rate.rounded().get(), dec!(6.64));
    }

    /// A foreign row's EUR figures are the file's own, so the rate is informational and its
    /// source `broker`; a row already in EUR is `native` [DOM-030], [DOM-031], [DOM-033].
    #[test]
    fn the_rate_source_is_broker_on_a_foreign_row_and_native_on_an_eur_one() {
        let foreign = sample_buy().conversion(trade_date()).expect("it inverts");
        let domestic = sample_sell().conversion(trade_date()).expect("it inverts");

        assert_eq!(foreign.source(), RateSource::Broker);
        assert!(foreign.source().is_informational());
        assert_eq!(foreign.currency(), &Currency::new("USD"));
        assert_eq!(foreign.rate_date(), trade_date());

        assert_eq!(domestic.source(), RateSource::Native);
        assert_eq!(domestic.rate().get(), Decimal::ONE);
    }

    /// `Aantal` is the EUR movement and never a share count, so the quantity the price divides
    /// by is the caller's [IMP-SAXO-009].
    #[test]
    fn the_eur_movement_is_never_read_as_a_quantity() {
        let buy = sample_buy();
        let forty = derived(&buy, Direction::Acquisition, dec!(40));
        let twenty = derived(&buy, Direction::Acquisition, dec!(20));

        assert_eq!(forty.unit_price().eur().rounded().get(), dec!(5.240750));
        assert_eq!(twenty.unit_price().eur().rounded().get(), dec!(10.481500));
        // Dividing by `Aantal` would give a price of about one, which is what reading the
        // column as a quantity produces.
        assert_ne!(
            forty.unit_price().eur().get(),
            forty.gross().eur().get() / dec!(216.92)
        );
    }

    /// The factor divides the price exactly once, so `quantity × price × factor` rebuilds the
    /// gross and a percent-of-par quotation is not a hundredfold error [IMP-SAXO-023],
    /// [DEC-035].
    ///
    /// Synthetic, and per-unit against percent-of-par rather than the bond worked example:
    /// which factor a security takes is FIF-075's, and OQ-010 leaves it undecided.
    #[test]
    fn the_quotation_factor_divides_the_price_exactly_once() {
        let row = transacties_row(&[
            ("_Valuta", "EUR"),
            ("Boekingsbedrag", "4183.80"),
            ("Aantal", "4183.80"),
            ("Totale kosten", "0"),
            ("Omrekeningskoers", "1"),
        ]);
        let booked = Booked::read(&row).expect("the row reads");
        let quantity = Quantity::new(dec!(3000));

        let per_unit = booked
            .derive(Direction::Acquisition, quantity, PER_UNIT, trade_date())
            .expect("it derives");
        let percent_of_par = booked
            .derive(Direction::Acquisition, quantity, dec!(0.01), trade_date())
            .expect("it derives");

        assert_eq!(per_unit.unit_price().eur().rounded().get(), dec!(1.3946));
        assert_eq!(
            percent_of_par.unit_price().eur().rounded().get(),
            dec!(139.46)
        );
        // Applying the factor a second time is the hundredfold error the division prevents.
        assert_eq!(
            percent_of_par.unit_price().eur().get() * quantity.get() * dec!(0.01),
            per_unit.gross().eur().get()
        );
    }

    /// A money column holding something that is not a number names itself, and a blank cell is
    /// one of those: it is refused rather than read as a movement of nothing.
    #[test]
    fn a_money_column_that_is_not_a_number_is_refused() {
        for (header, value) in [("Aantal", "n/a"), ("Totale kosten", "")] {
            // The override comes first: the helper answers the first entry naming a column.
            let row = transacties_row(&[
                (header, value),
                ("_Valuta", "EUR"),
                ("Boekingsbedrag", "1"),
                ("Aantal", "1"),
                ("Totale kosten", "0"),
                ("Omrekeningskoers", "1"),
            ]);

            let error = Booked::read(&row).expect_err("a non-numeric money cell is refused");

            assert_eq!(
                error,
                SaxoError::NotAnAmount {
                    header: header.to_owned(),
                    value: value.to_owned(),
                }
            );
        }
    }

    /// A row carrying no `_Valuta` names no currency for its native figures, and is refused
    /// rather than assumed to be in EUR.
    #[test]
    fn a_row_with_no_native_currency_is_refused() {
        let row = transacties_row(&[
            ("Boekingsbedrag", "1"),
            ("Aantal", "1"),
            ("Totale kosten", "0"),
            ("Omrekeningskoers", "1"),
        ]);

        assert_eq!(Booked::read(&row), Err(SaxoError::NoCurrency));
    }

    /// A caller reading the wrong sheet's row is told which column is absent, rather than
    /// getting a figure of zero.
    #[test]
    fn a_row_missing_a_money_column_names_it() {
        let row = SourceRow::new(vec![("_Valuta".to_owned(), "USD".to_owned())], "");

        assert_eq!(
            Booked::read(&row),
            Err(SaxoError::MissingColumn {
                header: NATIVE_MOVEMENT.to_owned(),
            })
        );
    }

    /// A price over nothing is reported rather than panicking: a closing of no units is a data
    /// problem the caller reports.
    #[test]
    fn a_price_over_a_zero_divisor_is_refused() {
        for factor in [PER_UNIT, Decimal::ZERO] {
            let quantity = if factor == PER_UNIT {
                Quantity::zero()
            } else {
                Quantity::new(dec!(40))
            };

            let error = sample_buy()
                .derive(Direction::Acquisition, quantity, factor, trade_date())
                .expect_err("no unit price exists");

            assert_eq!(error, SaxoError::UnderivableMoney { reason: NO_DIVISOR });
        }
    }

    /// A zero `Omrekeningskoers` has no reciprocal, so neither the stored rate nor the native
    /// cost exists [IMP-SAXO-029].
    #[test]
    fn a_zero_quote_is_refused() {
        let row = transacties_row(&[
            ("_Valuta", "USD"),
            ("Boekingsbedrag", "-238.00"),
            ("Aantal", "-216.92"),
            ("Totale kosten", "-7.29"),
            ("Omrekeningskoers", "0"),
        ]);
        let booked = Booked::read(&row).expect("the row reads");

        assert_eq!(
            booked.conversion(trade_date()),
            Err(SaxoError::UnderivableMoney { reason: ZERO_QUOTE })
        );
        assert_eq!(
            booked.derive(
                Direction::Acquisition,
                Quantity::new(dec!(40)),
                PER_UNIT,
                trade_date()
            ),
            Err(SaxoError::UnderivableMoney { reason: ZERO_QUOTE })
        );
    }

    /// The currency is read off `_Valuta`, which is the native side; `Valuta` is the booking
    /// currency and is EUR on every row of the sample [IMP-SAXO-010].
    #[test]
    fn the_native_currency_is_the_underscored_column() {
        let row = transacties_row(&[
            ("Valuta", "EUR"),
            ("_Valuta", "CAD"),
            ("Boekingsbedrag", "18.64"),
            ("Aantal", "12.97"),
            ("Totale kosten", "0"),
            ("Omrekeningskoers", "0.699873"),
        ]);

        let booked = Booked::read(&row).expect("the row reads");

        assert_eq!(booked.currency(), &Currency::new("CAD"));
    }
}
