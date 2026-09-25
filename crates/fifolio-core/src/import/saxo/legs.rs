//! What a corporate action's `_Transacties` legs sum to, once the reversal among them is gone.
//!
//! This is the **position half** of a reversal: it reads the `- Terugboeking` suffix and removes
//! rows. Its cash half is [`super::reversal`], which reads the same suffix on `Transacties` and
//! subtracts money [IMP-SAXO-034]. A reversal shows up in both places and the two halves are read
//! by different rules, so neither module is the other's caller.
//!
//! # A side is summed, never indexed
//!
//! A group's legs are its `_Transacties` rows under one `Corporate action-Id`
//! [IMP-SAXO-037], and there are not always two of them: the 2023 Philips dividend issues **two**
//! shares as two `Gekocht` legs of one, and a rule reading "the `Gekocht` leg" acquires one share
//! and silently drops the other [IMP-SAXO-044]. Each side's quantity and traded value are
//! therefore the sum of that side's legs. [`Sides`] offers those sums and nothing else: it holds
//! no leg and answers none, so "the `Gekocht` leg" is not a question a caller can ask.
//!
//! There are **three** sides. `Deponering` is its own, neither bought nor sold: all 13 transfers
//! in the sample carry it, and a two-sided reading drops every one of them [IMP-SAXO-048]. A
//! summed quantity keeps the file's sign, so a `Verkocht` side is negative — the side names its
//! own direction, and re-signing it would state the same fact twice.
//!
//! # Cancellation keys on the label, not on the shape
//!
//! A leg whose `Acties` carries the `- Terugboeking` suffix cancels against the leg in the same
//! group with the same absolute quantity, the **same** price and the opposite-signed traded
//! value; both are removed before anything is summed [IMP-SAXO-045]. A group carrying no suffixed
//! leg cancels nothing, however its legs are shaped [IMP-SAXO-046].
//!
//! The shape alone cannot decide it. The DeVolksbank tender's reversal is
//! `Gekocht 2000 @ 999.03 / -19980.65` against `Verkocht -2000 @ 999.03 / 19980.65`, and the
//! sample's one `Omwisseling` is `Gekocht 3 @ 168.63 / -505.89` against
//! `Verkocht -3 @ 168.63 / 505.89`. The two are the same shape; one is a booking being undone and
//! the other a genuine exchange, and cancelling on shape leaves the exchange with no legs at all.
//! `Openen/sluiten` corroborates a reversal and is deliberately not read: the suffix is the rule
//! [IMP-SAXO-046].
//!
//! Four points the specification does not settle, each chosen here so that nothing is invented.
//! None occurs in the five-year sample, the first excepted:
//!
//! * a suffixed leg matching no leg in its group **cancels nothing and survives**, rather than
//!   refusing the file. The sample carries exactly that — the lone `Dividend - Terugboeking` leg
//!   of the 2023 export sits in a `Corporate action-Id` of its own, the dividend it reverses
//!   being in another — so refusing would refuse a real export;
//! * a reversal never cancels another reversal, a reversal being a booking undone rather than a
//!   booking;
//! * a leg naming a **fourth** `Trade Event Type` refuses the group. IMP-SAXO-048 names three
//!   and the sample carries only those three; a fourth is a position moving in a direction this
//!   reader cannot name, and dropping it would lose that position silently;
//! * a leg whose `Instrumentvaluta` is **blank** refuses the group, for the reason IMP-SAXO-047
//!   gives for a disagreement: a traded value in no currency is not summable.
//!
//! # One group, one currency
//!
//! A side's traded values are summable only because a group carries one instrument and so one
//! currency. A group whose legs disagree on `Instrumentvaluta`, or one of whose legs names no
//! currency at all, is refused rather than summed [IMP-SAXO-047]: the sum would otherwise be a
//! figure in no currency, exactly as [`super::reversal`] refuses to sum native figures across
//! quotes.
//!
//! What a group's sides *mean* — a split's ratio, a tender's disposal, a stock election's share
//! count — is not decided here, as in [`super::quantity`]. This module answers what the sides
//! are.

use std::collections::BTreeSet;

use rust_decimal::Decimal;

use super::money::{OVERFLOW, amount};
use super::quantity::{Label, leg_quantity};
use super::reversal::Reversible;
use super::{SaxoError, field};
use crate::decimal::{Money, Quantity, Scaled as _};
use crate::import::reader::SourceRow;
use crate::valuation::Currency;

/// The action a leg states, which is where its reversal suffix is read from [IMP-SAXO-033].
const ACTIES: &str = "Acties";

/// The column stating which side of the event a leg is on [IMP-SAXO-038], [IMP-SAXO-048].
const TRADE_EVENT_TYPE: &str = "Trade Event Type";

/// The price a leg was booked at, as a column and not as the label's display value
/// [IMP-SAXO-012].
const PRICE: &str = "Prijs";

/// The value a leg traded, signed, in the instrument's currency.
const TRADED_VALUE: &str = "Verhandelde waarde";

/// The currency the instrument, and so the traded value, is denominated in [IMP-SAXO-047].
const INSTRUMENT_CURRENCY: &str = "Instrumentvaluta";

/// Which side of a corporate action a leg is on [IMP-SAXO-048].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    /// `Gekocht`: the legs that open a position.
    Acquired,
    /// `Verkocht`: the legs that close one. Its summed quantity is negative, as the file states
    /// it.
    Disposed,
    /// `Deponering`: a transfer in, which is neither of the other two.
    Deposited,
}

impl Side {
    /// The three sides a leg can be on.
    pub const ALL: [Self; 3] = [Self::Acquired, Self::Disposed, Self::Deposited];

    /// The side a leg's `Trade Event Type` names.
    ///
    /// # Errors
    ///
    /// When the leg carries no such column, and when it names a fourth event type: there is no
    /// side to sum it into, and dropping it would lose the position it moves.
    fn read(leg: &SourceRow) -> Result<Self, SaxoError> {
        let event = field(leg, TRADE_EVENT_TYPE).ok_or_else(|| SaxoError::MissingColumn {
            header: TRADE_EVENT_TYPE.to_owned(),
        })?;
        match event {
            "Gekocht" => Ok(Self::Acquired),
            "Verkocht" => Ok(Self::Disposed),
            "Deponering" => Ok(Self::Deposited),
            _ => Err(SaxoError::UnknownSide {
                value: event.to_owned(),
            }),
        }
    }

    /// Where this side's total sits in [`Sides::totals`].
    fn position(self) -> usize {
        match self {
            Self::Acquired => 0,
            Self::Disposed => 1,
            Self::Deposited => 2,
        }
    }
}

/// One leg of a group, read from its columns.
///
/// Private, and it stays private: a caller reaching a leg is a caller indexing a side
/// [IMP-SAXO-044]. `price` exists only to match a reversal against what it reverses and is
/// answered to nobody.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Leg {
    side: Side,
    quantity: Quantity,
    price: Decimal,
    traded_value: Decimal,
    currency: Currency,
    reversing: bool,
}

impl Leg {
    /// One `_Transacties` row.
    fn read(leg: &SourceRow) -> Result<Self, SaxoError> {
        let acties = field(leg, ACTIES).ok_or_else(|| SaxoError::MissingColumn {
            header: ACTIES.to_owned(),
        })?;
        let currency = field(leg, INSTRUMENT_CURRENCY).ok_or_else(|| SaxoError::MissingColumn {
            header: INSTRUMENT_CURRENCY.to_owned(),
        })?;
        if currency.is_empty() {
            return Err(SaxoError::NoInstrumentCurrency);
        }

        Ok(Self {
            side: Side::read(leg)?,
            quantity: leg_quantity(leg)?,
            price: amount(leg, PRICE)?,
            traded_value: amount(leg, TRADED_VALUE)?,
            currency: Currency::new(currency),
            // The suffix is taken off the parsed action, so it is found on a leg whose label
            // carries a trade clause as well as on one whose label is the action alone.
            reversing: Reversible::read(Label::parse(acties)?.action()).reversing(),
        })
    }

    /// Whether this leg is the one `reversal` undoes [IMP-SAXO-045].
    ///
    /// The price is **equal** on a cancelling pair and not negated — both DeVolksbank legs read
    /// 999.03 — while the traded values are opposites.
    fn cancelled_by(&self, reversal: &Self) -> bool {
        !self.reversing
            && self.quantity.get().abs() == reversal.quantity.get().abs()
            && self.price == reversal.price
            && self.traded_value == -reversal.traded_value
    }
}

/// What one side of a group sums to [IMP-SAXO-044].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideTotal {
    quantity: Quantity,
    traded_value: Money,
}

impl SideTotal {
    /// The side's quantity: the sum of its legs, keeping the file's sign, so a disposal is
    /// negative [IMP-SAXO-048].
    #[must_use]
    pub fn quantity(&self) -> Quantity {
        self.quantity
    }

    /// The side's traded value, in the group's instrument currency and not in EUR
    /// [IMP-SAXO-047].
    #[must_use]
    pub fn traded_value(&self) -> Money {
        self.traded_value
    }
}

/// The sides of one `Corporate action-Id` group, summed after cancellation [IMP-SAXO-044],
/// [IMP-SAXO-045], [IMP-SAXO-048].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sides {
    currency: Currency,
    /// Per [`Side::position`], what that side summed to, absent when the group has no leg on it.
    totals: [Option<SideTotal>; 3],
}

impl Sides {
    /// The sides of `legs`, which are one group's `_Transacties` rows; grouping them is the
    /// caller's, exactly as in [`super::reversal::group_cash`].
    ///
    /// `None` for a row joining no leg at all, which is most of the cash ledger.
    ///
    /// # Errors
    ///
    /// When a leg carries no column this reads, states an unreadable quantity, price or traded
    /// value, carries an `Acties` label that does not parse, or names a fourth `Trade Event
    /// Type`; when the group's legs do not agree on one instrument currency [IMP-SAXO-047]; and
    /// when a sum leaves the range of a decimal.
    pub fn of<'a>(
        legs: impl IntoIterator<Item = &'a SourceRow>,
    ) -> Result<Option<Self>, SaxoError> {
        let legs: Vec<Leg> = legs
            .into_iter()
            .map(Leg::read)
            .collect::<Result<_, SaxoError>>()?;
        let Some(first) = legs.first() else {
            return Ok(None);
        };
        let currency = first.currency.clone();
        // Checked over every leg the group has, the cancelled ones included. IMP-SAXO-045 orders
        // the two rules that *read* an event — cancel, then sum — and this is neither: a group
        // naming two instruments is malformed however its legs cancel, and refusing it is safer
        // than cancelling the disagreement away and answering sides for what is left.
        if let Some(other) = legs.iter().find(|leg| leg.currency != currency) {
            return Err(SaxoError::MixedLegCurrency {
                first: currency.code().to_owned(),
                second: other.currency.code().to_owned(),
            });
        }

        let remaining = cancelled(legs);
        let totals: Vec<Option<SideTotal>> = Side::ALL
            .into_iter()
            .map(|side| total(remaining.iter().filter(move |leg| leg.side == side)))
            .collect::<Result<_, SaxoError>>()?;

        Ok(Some(Self {
            currency,
            totals: totals
                .try_into()
                .unwrap_or_else(|_| unreachable!("one total per side, and there are three")),
        }))
    }

    /// The currency the group's instrument, and so every traded value here, is denominated in.
    #[must_use]
    pub fn currency(&self) -> &Currency {
        &self.currency
    }

    /// What one side summed to, `None` when the group has no surviving leg on it.
    #[must_use]
    pub fn side(&self, side: Side) -> Option<SideTotal> {
        self.totals[side.position()]
    }
}

/// `legs` with each reversal and the leg it reverses removed [IMP-SAXO-045].
///
/// Each reversal cancels at most one leg, and a leg is cancelled at most once: two identical
/// bookings reversed once leave one of them standing.
fn cancelled(legs: Vec<Leg>) -> Vec<Leg> {
    let removed =
        legs.iter()
            .enumerate()
            .filter(|(_, leg)| leg.reversing)
            .fold(
                BTreeSet::new(),
                |mut removed: BTreeSet<usize>, (at, reversal)| {
                    let cancelled = legs.iter().enumerate().find(|(other, leg)| {
                        !removed.contains(other) && leg.cancelled_by(reversal)
                    });
                    if let Some((other, _)) = cancelled {
                        removed.extend([at, other]);
                    }
                    removed
                },
            );

    legs.into_iter()
        .enumerate()
        .filter(|(at, _)| !removed.contains(at))
        .map(|(_, leg)| leg)
        .collect()
}

/// What one side's legs sum to, `None` when it has none [IMP-SAXO-044].
fn total<'a>(mut legs: impl Iterator<Item = &'a Leg>) -> Result<Option<SideTotal>, SaxoError> {
    legs.try_fold(None, |summed: Option<SideTotal>, leg| {
        let running = summed.unwrap_or(SideTotal {
            quantity: Quantity::zero(),
            traded_value: Money::new(Decimal::ZERO),
        });
        Ok(Some(SideTotal {
            quantity: Quantity::new(added(running.quantity.get(), leg.quantity.get())?),
            traded_value: Money::new(added(running.traded_value.get(), leg.traded_value)?),
        }))
    })
}

/// One leg's figure added to a running total, at full precision [ARC-009].
fn added(total: Decimal, contribution: Decimal) -> Result<Decimal, SaxoError> {
    total
        .checked_add(contribution)
        .ok_or(SaxoError::UnderivableMoney { reason: OVERFLOW })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::saxo::Sheet;
    use rust_decimal_macros::dec;

    /// One `_Transacties` leg, spelled as the file spells it — non-breaking spaces and all —
    /// with only the named columns populated.
    fn leg(values: &[(&str, &str)]) -> SourceRow {
        let columns = Sheet::Detail
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

    /// A leg of a group: its action, its side, its signed quantity, its price, its traded value
    /// and the `Openen/sluiten` the file states, in EUR.
    fn eur_leg(
        acties: &str,
        event: &str,
        quantity: &str,
        price: &str,
        traded_value: &str,
        opening: &str,
    ) -> SourceRow {
        leg(&[
            ("Acties", acties),
            ("Trade Event Type", event),
            ("Traded Quantity", quantity),
            ("Prijs", price),
            ("Verhandelde waarde", traded_value),
            ("Openen/sluiten", opening),
            ("Instrumentvaluta", "EUR"),
        ])
    }

    /// The side's quantity and traded value, so a test reads as the table does.
    fn summed(sides: &Sides, side: Side) -> Option<(Decimal, Decimal)> {
        sides
            .side(side)
            .map(|total| (total.quantity().get(), total.traded_value().get()))
    }

    /// A row joining no leg has no sides, which is most of the cash ledger [IMP-SAXO-037].
    #[test]
    fn a_group_of_no_legs_has_no_sides() {
        let none: [SourceRow; 0] = [];

        assert_eq!(Sides::of(&none).expect("no legs read"), None);
    }

    /// One `Gekocht`: a stock election issuing one share, four in the sample [IMP-SAXO-044].
    #[test]
    fn one_acquisition_is_its_own_side() {
        let group = [eur_leg(
            "Keuzedividend",
            "Gekocht",
            "1",
            "21.80",
            "-21.80",
            "Te openen",
        )];

        let sides = Sides::of(&group).expect("a summable group").expect("sides");

        assert_eq!(
            summed(&sides, Side::Acquired),
            Some((dec!(1), dec!(-21.80)))
        );
        assert_eq!(summed(&sides, Side::Disposed), None);
        assert_eq!(summed(&sides, Side::Deposited), None);
        assert_eq!(sides.currency(), &Currency::eur());
    }

    /// One `Verkocht` plus one `Gekocht`: both splits, the merger and the exchange. The disposal
    /// keeps the file's negative sign [IMP-SAXO-048].
    #[test]
    fn a_disposal_and_an_acquisition_are_two_sides() {
        let group = [
            eur_leg(
                "Stock split",
                "Verkocht",
                "-4",
                "87.86",
                "351.44",
                "Te sluiten",
            ),
            eur_leg(
                "Stock split",
                "Gekocht",
                "20",
                "17.57",
                "-351.44",
                "Te openen",
            ),
        ];

        let sides = Sides::of(&group).expect("a summable group").expect("sides");

        assert_eq!(
            summed(&sides, Side::Disposed),
            Some((dec!(-4), dec!(351.44)))
        );
        assert_eq!(
            summed(&sides, Side::Acquired),
            Some((dec!(20), dec!(-351.44)))
        );
    }

    /// One `Verkocht`: the expiration, three in the sample [IMP-SAXO-048].
    #[test]
    fn one_disposal_is_its_own_side() {
        let group = [eur_leg(
            "Expiratie",
            "Verkocht",
            "-3000",
            "0",
            "0",
            "Te sluiten",
        )];

        let sides = Sides::of(&group).expect("a summable group").expect("sides");

        assert_eq!(summed(&sides, Side::Disposed), Some((dec!(-3000), dec!(0))));
        assert_eq!(summed(&sides, Side::Acquired), None);
    }

    /// Two `Gekocht`: the 2023 Philips dividend issues **two** shares at 34.74, and a side read
    /// by indexing one leg would acquire one of them [IMP-SAXO-044].
    ///
    /// The fixture's amounts are perturbed [TST-014], so the specification's own figures are
    /// stated here rather than read from it.
    #[test]
    fn two_acquisitions_sum_rather_than_answer_one_of_them() {
        let group = [
            eur_leg("Dividend", "Gekocht", "1", "34.74", "-34.74", "Te openen"),
            eur_leg("Dividend", "Gekocht", "1", "34.74", "-34.74", "Te openen"),
        ];

        let sides = Sides::of(&group).expect("a summable group").expect("sides");

        assert_eq!(
            summed(&sides, Side::Acquired),
            Some((dec!(2), dec!(-69.48)))
        );
    }

    /// The DeVolksbank tender's reversal cancels, and the `Omwisseling` of the same shape does
    /// not: the `- Terugboeking` suffix is the signal and the shape is not [IMP-SAXO-045],
    /// [IMP-SAXO-046].
    ///
    /// Both groups carry `Te openen` against `Te sluiten`, so a rule keying on `Openen/sluiten`
    /// would cancel both, and a rule keying on the shape would cancel both. The tender leaves
    /// one disposal of 2000 at 99.90 for 1998.07, the figure `Bookings` and the cash ledger both
    /// show; the exchange keeps both of its legs.
    #[test]
    fn the_suffix_cancels_and_the_same_shape_without_it_does_not() {
        let tender = [
            eur_leg(
                "Terugkoopaanbod",
                "Verkocht",
                "-2000",
                "999.03",
                "19980.65",
                "Te openen",
            ),
            eur_leg(
                "Terugkoopaanbod",
                "Verkocht",
                "-2000",
                "99.90",
                "1998.07",
                "Te sluiten",
            ),
            eur_leg(
                "Terugkoopaanbod - Terugboeking",
                "Gekocht",
                "2000",
                "999.03",
                "-19980.65",
                "Te sluiten",
            ),
        ];
        let exchange = [
            eur_leg(
                "Omwisseling",
                "Verkocht",
                "-3",
                "168.63",
                "505.89",
                "Te sluiten",
            ),
            eur_leg(
                "Omwisseling",
                "Gekocht",
                "3",
                "168.63",
                "-505.89",
                "Te openen",
            ),
        ];

        let tendered = Sides::of(&tender)
            .expect("a summable group")
            .expect("sides");
        let exchanged = Sides::of(&exchange)
            .expect("a summable group")
            .expect("sides");

        assert_eq!(
            summed(&tendered, Side::Disposed),
            Some((dec!(-2000), dec!(1998.07)))
        );
        assert_eq!(summed(&tendered, Side::Acquired), None);
        assert_eq!(
            summed(&exchanged, Side::Disposed),
            Some((dec!(-3), dec!(505.89)))
        );
        assert_eq!(
            summed(&exchanged, Side::Acquired),
            Some((dec!(3), dec!(-505.89)))
        );
    }

    /// One `Deponering`: a transfer in is a third side, and a two-sided reading would drop all
    /// 13 of the sample's [IMP-SAXO-048].
    #[test]
    fn a_transfer_is_a_third_side() {
        let group = [eur_leg(
            "Deponering 300 @ 51.40 EUR",
            "Deponering",
            "300",
            "51.40",
            "-15420.00",
            "Te openen",
        )];

        let sides = Sides::of(&group).expect("a summable group").expect("sides");

        assert_eq!(
            summed(&sides, Side::Deposited),
            Some((dec!(300), dec!(-15420.00)))
        );
        assert_eq!(summed(&sides, Side::Acquired), None);
        assert_eq!(summed(&sides, Side::Disposed), None);
    }

    /// A suffixed leg matching nothing in its group cancels nothing and survives
    /// [IMP-SAXO-045]. The sample carries exactly that: the 2023 `Dividend - Terugboeking` is
    /// alone under its own `Corporate action-Id`.
    #[test]
    fn a_reversal_matching_no_leg_survives() {
        let group = [eur_leg(
            "Dividend - Terugboeking",
            "Verkocht",
            "-1",
            "34.74",
            "34.74",
            "Te sluiten",
        )];

        let sides = Sides::of(&group).expect("a summable group").expect("sides");

        assert_eq!(
            summed(&sides, Side::Disposed),
            Some((dec!(-1), dec!(34.74)))
        );
    }

    /// One reversal cancels one leg: the second of two identical bookings stands
    /// [IMP-SAXO-045].
    #[test]
    fn a_reversal_cancels_one_leg_and_not_every_match() {
        let group = [
            eur_leg("Dividend", "Gekocht", "1", "34.74", "-34.74", "Te openen"),
            eur_leg("Dividend", "Gekocht", "1", "34.74", "-34.74", "Te openen"),
            eur_leg(
                "Dividend - Terugboeking",
                "Verkocht",
                "-1",
                "34.74",
                "34.74",
                "Te sluiten",
            ),
        ];

        let sides = Sides::of(&group).expect("a summable group").expect("sides");

        assert_eq!(
            summed(&sides, Side::Acquired),
            Some((dec!(1), dec!(-34.74)))
        );
        assert_eq!(summed(&sides, Side::Disposed), None);
    }

    /// A reversal cancels the leg of its own absolute quantity and not one that merely shares
    /// its price and traded value [IMP-SAXO-045].
    ///
    /// The decoy is read first, so a rule blind to the quantity would cancel it and leave a
    /// disposal of 2000 standing instead of the 1000 the group really disposed of.
    #[test]
    fn a_reversal_does_not_cancel_a_leg_of_another_quantity() {
        let group = [
            eur_leg(
                "Terugkoopaanbod",
                "Verkocht",
                "-1000",
                "999.03",
                "19980.65",
                "Te sluiten",
            ),
            eur_leg(
                "Terugkoopaanbod",
                "Verkocht",
                "-2000",
                "999.03",
                "19980.65",
                "Te openen",
            ),
            eur_leg(
                "Terugkoopaanbod - Terugboeking",
                "Gekocht",
                "2000",
                "999.03",
                "-19980.65",
                "Te sluiten",
            ),
        ];

        let sides = Sides::of(&group).expect("a summable group").expect("sides");

        assert_eq!(
            summed(&sides, Side::Disposed),
            Some((dec!(-1000), dec!(19980.65)))
        );
        assert_eq!(summed(&sides, Side::Acquired), None);
    }

    /// A reversal cancels the leg of the **same** price and not one of the negated price
    /// [IMP-SAXO-045].
    ///
    /// The decoy carries -999.03, which is what the specification's earlier "exact negatives"
    /// wording would have matched; it is read first, so a rule blind to the price cancels it.
    #[test]
    fn a_reversal_does_not_cancel_a_leg_of_the_negated_price() {
        let group = [
            eur_leg(
                "Terugkoopaanbod",
                "Gekocht",
                "2000",
                "-999.03",
                "19980.65",
                "Te openen",
            ),
            eur_leg(
                "Terugkoopaanbod",
                "Verkocht",
                "-2000",
                "999.03",
                "19980.65",
                "Te openen",
            ),
            eur_leg(
                "Terugkoopaanbod - Terugboeking",
                "Gekocht",
                "2000",
                "999.03",
                "-19980.65",
                "Te sluiten",
            ),
        ];

        let sides = Sides::of(&group).expect("a summable group").expect("sides");

        assert_eq!(
            summed(&sides, Side::Acquired),
            Some((dec!(2000), dec!(19980.65)))
        );
        assert_eq!(summed(&sides, Side::Disposed), None);
    }

    /// A reversal cancels the leg of the **opposite**-signed traded value and not one signed as
    /// it is itself [IMP-SAXO-045].
    ///
    /// The decoy has the reversal's own shape without its suffix — a real acquisition — and is
    /// read first, so a rule blind to the sign removes a booking that happened.
    #[test]
    fn a_reversal_does_not_cancel_a_leg_of_its_own_traded_sign() {
        let group = [
            eur_leg(
                "Terugkoopaanbod",
                "Gekocht",
                "2000",
                "999.03",
                "-19980.65",
                "Te openen",
            ),
            eur_leg(
                "Terugkoopaanbod",
                "Verkocht",
                "-2000",
                "999.03",
                "19980.65",
                "Te openen",
            ),
            eur_leg(
                "Terugkoopaanbod - Terugboeking",
                "Gekocht",
                "2000",
                "999.03",
                "-19980.65",
                "Te sluiten",
            ),
        ];

        let sides = Sides::of(&group).expect("a summable group").expect("sides");

        assert_eq!(
            summed(&sides, Side::Acquired),
            Some((dec!(2000), dec!(-19980.65)))
        );
        assert_eq!(summed(&sides, Side::Disposed), None);
    }

    /// A reversal never cancels another reversal: a reversal is a booking undone rather than a
    /// booking, so two of them mirroring each other both survive [IMP-SAXO-045].
    #[test]
    fn a_reversal_does_not_cancel_another_reversal() {
        let group = [
            eur_leg(
                "Terugkoopaanbod - Terugboeking",
                "Verkocht",
                "-2000",
                "999.03",
                "19980.65",
                "Te sluiten",
            ),
            eur_leg(
                "Terugkoopaanbod - Terugboeking",
                "Gekocht",
                "2000",
                "999.03",
                "-19980.65",
                "Te sluiten",
            ),
        ];

        let sides = Sides::of(&group).expect("a summable group").expect("sides");

        assert_eq!(
            summed(&sides, Side::Disposed),
            Some((dec!(-2000), dec!(19980.65)))
        );
        assert_eq!(
            summed(&sides, Side::Acquired),
            Some((dec!(2000), dec!(-19980.65)))
        );
    }

    /// A group whose every leg cancels is a group that had legs and has none left, which is not
    /// the same answer as a row joining no leg at all [IMP-SAXO-045], [IMP-SAXO-044]: it is
    /// `Some` with three empty sides, and it still names the instrument's currency.
    #[test]
    fn a_group_whose_legs_all_cancel_has_sides_and_no_side() {
        let group = [
            eur_leg(
                "Terugkoopaanbod",
                "Verkocht",
                "-2000",
                "999.03",
                "19980.65",
                "Te openen",
            ),
            eur_leg(
                "Terugkoopaanbod - Terugboeking",
                "Gekocht",
                "2000",
                "999.03",
                "-19980.65",
                "Te sluiten",
            ),
        ];

        let sides = Sides::of(&group).expect("a summable group").expect("sides");

        assert_eq!(
            Side::ALL.map(|side| summed(&sides, side)),
            [None, None, None]
        );
        assert_eq!(sides.currency(), &Currency::eur());
    }

    /// Two `Verkocht` legs sum, and the sum keeps the file's negative sign rather than being
    /// re-signed by the side it lands on [IMP-SAXO-044], [IMP-SAXO-048].
    #[test]
    fn two_disposals_sum_to_a_negative_side() {
        let group = [
            eur_leg(
                "Terugkoopaanbod",
                "Verkocht",
                "-1200",
                "99.90",
                "1198.80",
                "Te sluiten",
            ),
            eur_leg(
                "Terugkoopaanbod",
                "Verkocht",
                "-800",
                "99.90",
                "799.20",
                "Te sluiten",
            ),
        ];

        let sides = Sides::of(&group).expect("a summable group").expect("sides");

        assert_eq!(
            summed(&sides, Side::Disposed),
            Some((dec!(-2000), dec!(1998.00)))
        );
    }

    /// Fractional quantities sum at the full quantity scale, exactly [TST-016]: a side summed at
    /// a narrower scale would lose the eighth decimal of a fractional share.
    #[test]
    fn fractional_legs_sum_at_the_full_quantity_scale() {
        let group = [
            eur_leg(
                "Keuzedividend",
                "Gekocht",
                "0.00000001",
                "21.80",
                "-0.01",
                "Te openen",
            ),
            eur_leg(
                "Keuzedividend",
                "Gekocht",
                "1.23456789",
                "21.80",
                "-26.91",
                "Te openen",
            ),
        ];

        let sides = Sides::of(&group).expect("a summable group").expect("sides");

        assert_eq!(
            summed(&sides, Side::Acquired),
            Some((dec!(1.23456790), dec!(-26.92)))
        );
    }

    /// A leg whose `Acties` does not parse refuses the group: the suffix cancellation keys on is
    /// read from that label, and a half-read one states nothing about it [IMP-SAXO-045].
    #[test]
    fn a_leg_with_an_unparsable_label_refuses_the_group() {
        let group = [eur_leg(
            "Koop 40 @ 5.75",
            "Gekocht",
            "40",
            "5.75",
            "-230.00",
            "Te openen",
        )];

        assert!(matches!(
            Sides::of(&group),
            Err(SaxoError::UnparsableLabel { .. })
        ));
    }

    /// A side whose legs sum past the range of a decimal refuses the group rather than
    /// saturating, a saturated total being a figure the file does not state [ARC-009].
    #[test]
    fn a_side_summing_out_of_range_refuses_the_group() {
        let huge = Decimal::MAX.to_string();
        let group = [
            eur_leg("Fusie", "Gekocht", "1", "1", &huge, "Te openen"),
            eur_leg("Fusie", "Gekocht", "1", "1", &huge, "Te openen"),
        ];

        assert_eq!(
            Sides::of(&group),
            Err(SaxoError::UnderivableMoney { reason: OVERFLOW })
        );
    }

    /// A group whose legs disagree on instrument currency is refused rather than summed: the sum
    /// would be a figure in no currency [IMP-SAXO-047].
    #[test]
    fn legs_in_two_currencies_refuse_the_group() {
        let group = [
            eur_leg(
                "Fusie",
                "Verkocht",
                "-300",
                "14.25",
                "4275.00",
                "Te sluiten",
            ),
            leg(&[
                ("Acties", "Fusie"),
                ("Trade Event Type", "Gekocht"),
                ("Traded Quantity", "199"),
                ("Prijs", "21.48"),
                ("Verhandelde waarde", "-4275.00"),
                ("Instrumentvaluta", "CAD"),
            ]),
        ];

        assert!(matches!(
            Sides::of(&group),
            Err(SaxoError::MixedLegCurrency { .. })
        ));
    }

    /// A leg naming no instrument currency is refused for the same reason [IMP-SAXO-047].
    #[test]
    fn a_leg_naming_no_currency_refuses_the_group() {
        let group = [leg(&[
            ("Acties", "Fusie"),
            ("Trade Event Type", "Gekocht"),
            ("Traded Quantity", "199"),
            ("Prijs", "21.48"),
            ("Verhandelde waarde", "-4275.00"),
        ])];

        assert_eq!(Sides::of(&group), Err(SaxoError::NoInstrumentCurrency));
    }

    /// A fourth `Trade Event Type` has no side to sum into, so it refuses the group rather than
    /// being dropped, which would lose the position it moves [IMP-SAXO-048].
    #[test]
    fn a_fourth_event_type_refuses_the_group() {
        let group = [eur_leg(
            "Fusie",
            "Geruild",
            "1",
            "34.74",
            "-34.74",
            "Te openen",
        )];

        assert!(matches!(
            Sides::of(&group),
            Err(SaxoError::UnknownSide { .. })
        ));
    }

    /// A leg stating an unreadable quantity stops the sum rather than contributing zero
    /// [IMP-SAXO-038].
    #[test]
    fn a_leg_with_an_unreadable_quantity_refuses_the_group() {
        let group = [eur_leg(
            "Fusie",
            "Gekocht",
            "n/a",
            "34.74",
            "-34.74",
            "Te openen",
        )];

        assert!(matches!(
            Sides::of(&group),
            Err(SaxoError::NotAQuantity { .. })
        ));
    }

    /// A leg stating an unreadable traded value does the same [IMP-SAXO-010].
    #[test]
    fn a_leg_with_an_unreadable_traded_value_refuses_the_group() {
        let group = [eur_leg(
            "Fusie",
            "Gekocht",
            "1",
            "34.74",
            "n/a",
            "Te openen",
        )];

        assert!(matches!(
            Sides::of(&group),
            Err(SaxoError::NotAnAmount { .. })
        ));
    }

    /// A row of another sheet carries none of the columns a leg is read from, and asking one for
    /// a group's sides names the column it lacks rather than reading it as a side.
    #[test]
    fn a_leg_missing_a_column_refuses_the_group() {
        // `Traded Quantity` is spelled out rather than named: its constant belongs to
        // [`super::quantity`], which reads that column.
        for header in [
            ACTIES,
            TRADE_EVENT_TYPE,
            INSTRUMENT_CURRENCY,
            "Traded Quantity",
            PRICE,
            TRADED_VALUE,
        ] {
            let complete = eur_leg("Fusie", "Gekocht", "1", "34.74", "-34.74", "Te openen");
            let columns = complete
                .columns()
                .iter()
                .filter(|(name, _)| name.split_whitespace().ne(header.split_whitespace()))
                .cloned()
                .collect();
            let group = [SourceRow::new(columns, "")];

            assert_eq!(
                Sides::of(&group),
                Err(SaxoError::MissingColumn {
                    header: header.to_owned()
                })
            );
        }
    }
}
