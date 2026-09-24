//! Rows that undo an earlier row, and what a `Corporate action-Id` group's cash sums to.
//!
//! # `Terugboeking` is a suffix
//!
//! It is never an `Acties` value on its own: it is attached to the action it reverses, and the
//! exports carry `Terugkoopaanbod - Terugboeking` and `Dividend - Terugboeking`. A suffixed value
//! classifies as its prefix, reversing [IMP-SAXO-033], which is what [`Reversible`] answers. The
//! match therefore requires both the separator and a non-empty prefix: a bare `Terugboeking`
//! reverses nothing nameable and is left as the action it is, so it reaches the classification
//! table as an unlisted value (OQ-008) rather than as a reversal of the empty action.
//!
//! What the prefix *means* — which variant `Terugkoopaanbod` classifies to — is IMP-SAXO-013's
//! and not this module's, exactly as in [`super::quantity`].
//!
//! # A group's cash, with reversals taken back out
//!
//! [`group_cash`] sums the rows of one `Corporate action-Id`. A reversal's cash **subtracts**: it
//! is a claw-back and not a second payment. The sample is the DeVolksbank tender, `3946.14` paid
//! against `-1998.07` reversed, netting `1948.07`.
//!
//! Its costs subtract too [IMP-SAXO-034], and that half is **chosen, not observed**: both
//! reversal rows in five years of exports carry zero in `Totale kosten`, so no data says either
//! way and the rule is picked to be arithmetically consistent with the cash — whatever the
//! reversal undoes, it undoes whole. No test here can catch it being wrong. **The check to
//! perform if a costed reversal ever arrives** is this: take that group's rows to the Saxo
//! statement or the corporate-action report for the event and read the fees actually charged over
//! the whole event; if they equal the non-reversal rows' costs less the reversal's, this rule
//! holds; if they equal the sum of all of them, the reversal's costs are a second charge and the
//! subtraction here is wrong.
//!
//! # Magnitudes in, magnitude out
//!
//! Each figure enters the sum as its magnitude, signed by whether its row reverses, rather than
//! as the cell's own sign. Two reasons, one per column: `Totale kosten` is always negative
//! [IMP-SAXO-010], so summing it as carried would make a reversal *increase* the group's costs,
//! which is the opposite of the rule; and a direction is never read off the sign of a money cell
//! in this importer, for the reason [`Direction`](super::money::Direction) gives. The result is
//! consequently unsigned — the gross cash the event moved, net of what was clawed back — and
//! which way it moved is the classification's answer [IMP-SAXO-013], not this function's. Where
//! reversals exceed the rows they reverse, the sum goes negative, which is reported as it falls
//! out rather than clamped.
//!
//! # EUR only
//!
//! The summation is over the EUR figures the rows carry, never over the native ones
//! [IMP-SAXO-035]: a group's rows may hold different `Omrekeningskoers` values, so their native
//! figures are amounts in incomparable units and only the booked EUR totals compute.

use rust_decimal::Decimal;

use super::money::{Booked, OVERFLOW};
use super::{SaxoError, field};
use crate::decimal::{Money, Scaled as _};
use crate::import::reader::SourceRow;

/// The `Acties` suffix marking a row that undoes an earlier one [IMP-SAXO-033].
const REVERSAL: &str = "Terugboeking";

/// What separates the suffix from the action it reverses.
const SEPARATOR: char = '-';

/// The column an action is spelled in.
const ACTIES: &str = "Acties";

/// An `Acties` action with its reversal suffix read off it [IMP-SAXO-033].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reversible<'a> {
    action: &'a str,
    reversing: bool,
}

impl<'a> Reversible<'a> {
    /// Reads the suffix off an action: `Dividend - Terugboeking` is `Dividend`, reversing.
    ///
    /// Takes the action an `Acties` label states — [`Label::action`](super::quantity::Label) —
    /// or the raw cell of a label with no trade clause, the two being the same string.
    #[must_use]
    pub fn read(acties: &'a str) -> Self {
        acties
            .trim_end()
            .strip_suffix(REVERSAL)
            .map(str::trim_end)
            // The separator and a non-empty prefix are both required, so `Terugboeking` alone is
            // not a reversal of nothing [IMP-SAXO-033].
            .and_then(|prefix| prefix.strip_suffix(SEPARATOR))
            .map(str::trim_end)
            .filter(|action| !action.is_empty())
            .map_or(
                Self {
                    action: acties,
                    reversing: false,
                },
                |action| Self {
                    action,
                    reversing: true,
                },
            )
    }

    /// The action the row performs, the suffix removed: `Terugkoopaanbod`, `Dividend`.
    #[must_use]
    pub fn action(&self) -> &'a str {
        self.action
    }

    /// Whether the row undoes the action rather than performing it.
    #[must_use]
    pub fn reversing(&self) -> bool {
        self.reversing
    }
}

/// What the rows of one `Corporate action-Id` sum to, in EUR [IMP-SAXO-034], [IMP-SAXO-035].
///
/// Both figures are magnitudes, reversals already taken out of them; see the module's header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupCash {
    cash: Money,
    costs: Money,
}

impl GroupCash {
    /// The cash the event moved, in EUR: the sum of `Aantal` with reversals subtracted.
    #[must_use]
    pub fn cash(&self) -> Money {
        self.cash
    }

    /// The costs the event carried, in EUR: the sum of `Totale kosten` with reversals subtracted.
    #[must_use]
    pub fn costs(&self) -> Money {
        self.costs
    }
}

/// Sums the cash and the costs of one `Corporate action-Id` group [IMP-SAXO-034],
/// [IMP-SAXO-035].
///
/// `rows` are the group's `Transacties` rows; grouping them is the caller's, which is what lets
/// one group be summed without reading the file's other events.
///
/// # Errors
///
/// When a row carries no `Acties` column or no money column, when a money column holds something
/// that is not a number, or when a sum leaves the range of a decimal.
pub fn group_cash<'a>(
    rows: impl IntoIterator<Item = &'a SourceRow>,
) -> Result<GroupCash, SaxoError> {
    rows.into_iter().try_fold(
        GroupCash {
            cash: Money::new(Decimal::ZERO),
            costs: Money::new(Decimal::ZERO),
        },
        |total, row| {
            let acties = field(row, ACTIES).ok_or_else(|| SaxoError::MissingColumn {
                header: ACTIES.to_owned(),
            })?;
            let booked = Booked::read(row)?;
            let reversing = Reversible::read(acties).reversing();
            let contribution = |figure: Decimal| {
                if reversing {
                    -figure.abs()
                } else {
                    figure.abs()
                }
            };

            Ok(GroupCash {
                cash: added(total.cash, contribution(booked.eur_movement()))?,
                costs: added(total.costs, contribution(booked.eur_costs()))?,
            })
        },
    )
}

/// One row's contribution added to a running total, at full precision [ARC-009].
fn added(total: Money, contribution: Decimal) -> Result<Money, SaxoError> {
    total
        .get()
        .checked_add(contribution)
        .map(Money::new)
        .ok_or(SaxoError::UnderivableMoney { reason: OVERFLOW })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::saxo::Sheet;
    use crate::import::saxo::quantity::Label;
    use rust_decimal_macros::dec;

    /// A `Transacties` row spelled as the file spells it, with only the named columns populated.
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

    /// A row of one group: its label, its EUR movement, its EUR costs, and a native side under
    /// its own quote.
    fn group_row(acties: &str, eur: &str, costs: &str, native: &str, quote: &str) -> SourceRow {
        transacties_row(&[
            ("Acties", acties),
            ("Aantal", eur),
            ("Totale kosten", costs),
            ("Boekingsbedrag", native),
            ("_Valuta", "USD"),
            ("Omrekeningskoers", quote),
        ])
    }

    /// The two suffixed values the exports carry classify as their prefix, reversing
    /// [IMP-SAXO-033].
    #[test]
    fn a_suffixed_action_classifies_as_its_prefix_reversing() {
        for (acties, action) in [
            ("Terugkoopaanbod - Terugboeking", "Terugkoopaanbod"),
            ("Dividend - Terugboeking", "Dividend"),
        ] {
            let read = Reversible::read(acties);

            assert_eq!(read.action(), action);
            assert!(read.reversing());
        }
    }

    /// The suffix is read off the action an `Acties` label states, trade clause and all
    /// [IMP-SAXO-033], [IMP-SAXO-038].
    #[test]
    fn the_suffix_is_read_off_a_parsed_label() {
        let label = Label::parse("Terugkoopaanbod - Terugboeking 40 @ 5.75 USD").expect("a label");

        assert_eq!(Reversible::read(label.action()).action(), "Terugkoopaanbod");
        assert!(Reversible::read(label.action()).reversing());
    }

    /// `Terugboeking` is never a value on its own, so a bare one reverses nothing and keeps its
    /// own spelling rather than becoming the empty action [IMP-SAXO-033].
    #[test]
    fn a_bare_reversal_is_not_a_reversal() {
        for acties in ["Terugboeking", "- Terugboeking", "Terugboeking -"] {
            let read = Reversible::read(acties);

            assert_eq!(read.action(), acties);
            assert!(!read.reversing());
        }
    }

    /// An unsuffixed action passes through untouched, including one merely ending in the word
    /// [IMP-SAXO-033].
    #[test]
    fn an_unsuffixed_action_is_not_reversing() {
        for acties in ["Dividend", "Terugkoopaanbod", "XTerugboeking"] {
            let read = Reversible::read(acties);

            assert_eq!(read.action(), acties);
            assert!(!read.reversing());
        }
    }

    /// The DeVolksbank tender, on synthetic figures: paid less reversed is the net
    /// [IMP-SAXO-034]. The fixture's own amounts are perturbed [TST-014], so the specification's
    /// figures are stated here rather than read from it.
    #[test]
    fn a_reversals_cash_subtracts_from_its_groups_cash() {
        let group = [
            group_row("Terugkoopaanbod", "3946.14", "0", "3946.14", "1"),
            group_row(
                "Terugkoopaanbod - Terugboeking",
                "-1998.07",
                "0",
                "-1998.07",
                "1",
            ),
        ];

        let summed = group_cash(&group).expect("a summable group");

        assert_eq!(summed.cash().get(), dec!(1948.07));
    }

    /// A reversal's costs subtract as its cash does [IMP-SAXO-034]. Chosen, not observed: every
    /// reversal in the exports carries zero costs, so this asserts the choice and not the data.
    #[test]
    fn a_reversals_costs_subtract_too() {
        let group = [
            group_row("Terugkoopaanbod", "3946.14", "-49.00", "3946.14", "1"),
            group_row(
                "Terugkoopaanbod - Terugboeking",
                "-1998.07",
                "-20.00",
                "-1998.07",
                "1",
            ),
        ];

        let summed = group_cash(&group).expect("a summable group");

        assert_eq!(summed.costs().get(), dec!(29.00));
    }

    /// Summation is over the EUR figures and never over the native ones, because a group's rows
    /// may hold different quotes [IMP-SAXO-035].
    #[test]
    fn summation_is_over_the_eur_figures() {
        let group = [
            group_row("Terugkoopaanbod", "1000.00", "0", "1200.00", "0.833333"),
            group_row("Terugkoopaanbod", "500.00", "0", "550.00", "0.909091"),
        ];

        let summed = group_cash(&group).expect("a summable group");

        // The native figures sum to 1750.00, which is no amount in any currency: the two rows
        // are quoted differently.
        assert_eq!(summed.cash().get(), dec!(1500.00));
    }

    /// A group of one row is that row's own magnitudes, so nothing is lost by summing a single
    /// booking [IMP-SAXO-034].
    #[test]
    fn a_group_of_one_row_sums_to_that_row() {
        let group = [group_row(
            "Fusie", "-216.92", "-7.29", "-238.00", "0.911413",
        )];

        let summed = group_cash(&group).expect("a summable group");

        assert_eq!(summed.cash().get(), dec!(216.92));
        assert_eq!(summed.costs().get(), dec!(7.29));
    }

    /// A row whose money cannot be read stops the sum rather than contributing zero
    /// [IMP-SAXO-010].
    #[test]
    fn a_row_with_an_unreadable_amount_refuses_the_sum() {
        let group = [group_row("Fusie", "n/a", "0", "0", "1")];

        assert!(matches!(
            group_cash(&group),
            Err(SaxoError::NotAnAmount { .. })
        ));
    }

    /// A row of another sheet carries no `Acties`, and asking one for a group's cash reports
    /// that rather than summing it as unreversed.
    #[test]
    fn a_row_carrying_no_acties_refuses_the_sum() {
        let columns = vec![("Aantal".to_owned(), "1.00".to_owned())];
        let group = [SourceRow::new(columns, "")];

        assert!(matches!(
            group_cash(&group),
            Err(SaxoError::MissingColumn { .. })
        ));
    }
}
