//! Perturbing the amounts [TST-012].
//!
//! Every monetary figure in a fixture is moved by up to ten percent, so that a fixture states no
//! real amount. That is what makes the fixtures unusable for arithmetic and why `testing.md` says
//! they test parsing, classification and idempotency only [TST-014]: after perturbation
//! `Aantal`, `Totale kosten` and `Boekingsbedrag` no longer reconcile with each other.
//!
//! Three properties are deliberately kept, because an importer's structure depends on them:
//!
//! * **Zero stays zero.** A `Deponering` row is recognizable by `Boekingsbedrag` and `Aantal`
//!   being zero [IMP-SAXO-014], and an empty column stays empty.
//! * **One stays one.** `Omrekeningskoers` is 1 on a transfer in even for a foreign-currency
//!   instrument, which is exactly why one needs an ECB lookup [IMP-SAXO-017].
//! * **Sign and scale stay.** The cash-flow sign convention is classification, not amount
//!   [IMP-TR-004], and a column's decimal scale is part of the format.
//!
//! The factor is derived from the amount's own magnitude, so equal magnitudes stay equal: a
//! reversal row still cancels the row it reverses, which is what makes `Terugboeking` recognizable
//! as one. That is also what keeps two sheets agreeing where they state the same figure: a Saxo
//! `Omrekeningskoers` or a label price met on `Transacties` and again on `Bookings` moves the same
//! way on both.
//!
//! Where two figures are related rather than equal, the related one is not perturbed on its own
//! magnitude but moved by the factor the figure it belongs to moved by — see [`perturb_like`] and
//! [`perturb_shares`]. A Saxo fixture needs both: `Verhandelde waarde` against `Prijs`, and the
//! `Bookings` components of one booking against the booking [TST-031].

use rust_decimal::Decimal;
use sha2::{Digest, Sha256};

/// The perturbed form of `amount`.
#[must_use]
pub fn perturb(amount: Decimal) -> Decimal {
    if amount.is_zero() || amount.abs() == Decimal::ONE {
        return amount;
    }

    let scale = amount.scale();
    let magnitude = amount.abs();
    let mut moved = (magnitude * factor_for(magnitude)).round_dp(scale);
    // A small amount at a coarse scale can round back onto itself — 0.03 times 1.02 is 0.03 at
    // two decimals — and an amount that came through unchanged is an amount that was not
    // perturbed. One unit at the column's scale is the smallest honest move.
    if moved == magnitude || moved.is_zero() {
        moved += Decimal::new(1, scale);
    }
    moved.rescale(scale);

    if amount.is_sign_negative() {
        -moved
    } else {
        moved
    }
}

/// `amount` moved by the factor that took `parent` to `moved_parent`, rounded at `amount`'s own
/// scale.
///
/// For a figure whose meaning is its relation to another: `Verhandelde waarde` is the quantity
/// times the price, and the quantity is structural and never moves [TST-028], so the traded value
/// has to move exactly as the price did or the fixture states a value no quantity and price
/// produce. A parent of zero has no factor, so the amount falls back on its own magnitude.
#[must_use]
pub fn perturb_like(amount: Decimal, parent: Decimal, moved_parent: Decimal) -> Decimal {
    if parent.is_zero() {
        return perturb(amount);
    }
    let mut moved = (amount * moved_parent / parent).round_dp(amount.scale());
    moved.rescale(amount.scale());
    moved
}

/// The components of `parent` in fixture form, moved by the factor `parent` itself moves by.
///
/// The rounding remainder is carried by the largest component, so that the components still sum
/// **exactly** to the perturbed parent: a Saxo booking is decomposed on `Bookings` and an importer
/// reads the decomposition against the booking [IMP-SAXO-042], so a fixture whose components no
/// longer sum is a fixture of nothing [TST-031]. The largest component carries it because a cent
/// of drift is proportionally smallest there; where the remainder would leave it stating the
/// export's own amount, one more unit passes to the next-largest so that neither does.
#[must_use]
pub fn perturb_shares(components: &[Decimal], parent: Decimal) -> Vec<Decimal> {
    let moved_parent = perturb(parent);
    let mut moved: Vec<Decimal> = components
        .iter()
        .map(|component| {
            let moved = perturb_like(*component, parent, moved_parent);
            // As in `perturb`: a small component against a factor near 1 rounds back onto itself,
            // and a component that came through unchanged is a real amount standing in a fixture
            // [TST-012]. One unit at the component's own scale is the smallest honest move; the
            // residual step below then restores the sum.
            if moved == *component && !component.is_zero() {
                nudged(moved)
            } else {
                moved
            }
        })
        .collect();
    let residual = moved_parent - moved.iter().sum::<Decimal>();
    let Some(absorber) = largest_of(&moved, |_| true) else {
        return moved;
    };
    moved[absorber] += residual;

    // The absorber is the one component the nudge above cannot protect, because the remainder can
    // carry it back onto the export's own amount. Where it did, one more unit leaves it for the
    // largest other component, in the direction that one already deviates in: the sum stands, the
    // partner cannot be carried back onto its own amount by a move away from it, and the absorber
    // is off the export's figure [TST-012].
    let leaked = moved[absorber] == components[absorber] && !components[absorber].is_zero();
    if leaked && let Some(partner) = largest_of(&moved, |index| index != absorber) {
        let unit = Decimal::new(1, moved[partner].scale().min(moved[absorber].scale()));
        let step = if moved[partner] > components[partner] {
            unit
        } else {
            -unit
        };
        moved[partner] += step;
        moved[absorber] -= step;
    }
    moved
}

/// The index of the component of largest magnitude among those `eligible`, if there is one.
///
/// The largest carries a correction because a cent of drift is proportionally smallest there.
fn largest_of(moved: &[Decimal], eligible: impl Fn(usize) -> bool) -> Option<usize> {
    (0..moved.len())
        .filter(|index| eligible(*index))
        .max_by_key(|index| moved[*index].abs())
}

/// `amount` moved one unit at its own scale, away from zero as [`perturb`] moves a magnitude.
fn nudged(amount: Decimal) -> Decimal {
    let unit = Decimal::new(1, amount.scale());
    let mut nudged = if amount.is_sign_negative() {
        amount - unit
    } else {
        amount + unit
    };
    nudged.rescale(amount.scale());
    nudged
}

/// A factor in `[0.90, 1.10]`, taken from the digest of the magnitude so that the run is
/// reproducible and equal magnitudes move together.
fn factor_for(magnitude: Decimal) -> Decimal {
    let digest = Sha256::digest(magnitude.to_string().as_bytes());
    let drawn = u64::from(u16::from_be_bytes([digest[0], digest[1]]));
    // 2001 steps of 0.0001 from 0.9000 to 1.1000.
    let steps = i64::try_from(drawn % 2001).expect("a value below 2001 fits an i64");
    Decimal::new(9000 + steps, 4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    fn decimal(literal: &str) -> Decimal {
        Decimal::from_str_exact(literal).expect("a decimal literal")
    }

    /// Amounts are perturbed [TST-012].
    #[test]
    fn an_amount_moves_by_no_more_than_a_tenth() {
        for literal in ["216.92", "-238.00", "1833.24", "4183.80", "0.030000"] {
            let original = decimal(literal);
            let moved = perturb(original);
            assert_ne!(moved, original, "{literal} came through unchanged");
            assert!(
                (moved - original).abs() <= original.abs() * decimal("0.101"),
                "{literal} moved to {moved}"
            );
        }
    }

    /// A zero column stays zero, which is how a `Deponering` row is recognized [IMP-SAXO-014].
    #[test]
    fn zero_is_left_alone() {
        assert_eq!(perturb(Decimal::ZERO), Decimal::ZERO);
        assert_eq!(perturb(decimal("0.00")), decimal("0.00"));
    }

    /// A conversion rate of exactly 1 is a fact about the row, not an amount [IMP-SAXO-017].
    #[test]
    fn one_is_left_alone() {
        assert_eq!(perturb(Decimal::ONE), Decimal::ONE);
        assert_eq!(perturb(-Decimal::ONE), -Decimal::ONE);
    }

    /// The cash-flow sign is classification [IMP-TR-004], and the scale is format.
    #[test]
    fn sign_and_scale_survive() {
        let costs = decimal("-7.29");
        let moved = perturb(costs);
        assert!(moved.is_sign_negative(), "{moved}");
        assert_eq!(moved.scale(), 2);
        assert_eq!(perturb(decimal("0.911413")).scale(), 6);
        assert_eq!(perturb(decimal("3000")).scale(), 0);
    }

    /// Equal magnitudes move together, so a reversal still cancels what it reverses.
    #[test]
    fn a_reversal_still_cancels() {
        let booked = decimal("17.62");
        let reversed = decimal("-17.62");
        assert_eq!(perturb(booked), -perturb(reversed));
    }

    /// A traded value moves by the factor its price moved by, so that it stays the quantity times
    /// the price once the price has moved [TST-031], the quantity itself never moving [TST-028].
    #[test]
    fn a_traded_value_moves_with_its_price() {
        let quantity = decimal("300");
        let price = decimal("51.40");
        let value = decimal("-15419.46");
        let moved_price = perturb(price);
        let moved_value = perturb_like(value, price, moved_price);
        assert_ne!(moved_value, value);
        // The sample's own error between value and quantity times price is 0.54 on this row; the
        // moved pair may not be further out than that error scaled plus the rounding of one cent.
        let before = (value.abs() - quantity * price).abs();
        let after = (moved_value.abs() - quantity * moved_price).abs();
        assert!(
            after <= before * decimal("1.1") + decimal("0.01"),
            "{moved_value} is not {quantity} at {moved_price}"
        );
        assert_eq!(moved_value.scale(), value.scale());
        assert!(moved_value.is_sign_negative());
    }

    /// A booking's components still sum to the booking after both have moved [TST-031].
    #[test]
    fn components_still_sum_to_their_booking() {
        let booking = decimal("2.03");
        let components = [decimal("2.38"), decimal("-0.35")];
        let moved = perturb_shares(&components, booking);
        assert_eq!(moved.iter().sum::<Decimal>(), perturb(booking));
        assert_ne!(moved[0], components[0]);
    }

    /// The remainder is carried, not dropped, and it is the largest component that carries it: at
    /// this booking the shares round to one cent more than the moved booking [TST-031].
    #[test]
    fn a_rounding_remainder_is_carried_by_the_largest_component() {
        let booking = decimal("1.30");
        let components = [decimal("0.05"), decimal("1.25")];
        let moved_booking = perturb(booking);
        let naive: Vec<Decimal> = components
            .iter()
            .map(|component| perturb_like(*component, booking, moved_booking))
            .collect();
        let residual = moved_booking - naive.iter().sum::<Decimal>();
        assert_eq!(residual, decimal("-0.01"), "the case carries no remainder");

        let moved = perturb_shares(&components, booking);

        assert_eq!(moved.iter().sum::<Decimal>(), moved_booking);
        assert_eq!(moved[0], naive[0], "the smaller component carried it");
        assert_eq!(moved[1], naive[1] + residual);
    }

    /// Where two components are of equal magnitude the remainder goes to the later one, which is
    /// what `max_by_key` settles a tie on. The tie decides which cell of the fixture moves, so it
    /// is pinned rather than left to a refactor [TST-031].
    #[test]
    fn a_tie_between_equal_components_is_carried_by_the_later_one() {
        let booking = decimal("0.36");
        let components = [decimal("0.18"), decimal("0.18")];
        let moved_booking = perturb(booking);
        let naive = perturb_like(components[0], booking, moved_booking);
        let residual = moved_booking - naive - naive;
        assert_eq!(residual, decimal("0.01"), "the case carries no remainder");

        let moved = perturb_shares(&components, booking);

        assert_eq!(moved.iter().sum::<Decimal>(), moved_booking);
        assert_eq!(moved, vec![naive, naive + residual]);
    }

    /// The same tie inside the leak correction: the remainder carries the later component back
    /// onto the export's own figure, and the unit that frees it goes to the equal partner
    /// [TST-012], [TST-031].
    #[test]
    fn a_tie_is_settled_the_same_way_when_the_remainder_leaks() {
        let booking = decimal("0.06");
        let components = [decimal("0.03"), decimal("0.03")];

        let moved = perturb_shares(&components, booking);

        assert_eq!(moved.iter().sum::<Decimal>(), perturb(booking));
        assert_eq!(moved, vec![decimal("0.05"), decimal("0.02")]);
    }

    /// A component whose factor rounds it back onto itself is moved anyway, or the fixture states
    /// a real amount [TST-012]; the sum to the booking survives that move [TST-031].
    #[test]
    fn a_component_that_would_come_through_unchanged_is_moved() {
        let booking = decimal("2.42");
        let components = [decimal("0.07"), decimal("2.35")];
        assert_eq!(
            perturb_like(components[0], booking, perturb(booking)),
            components[0],
            "the case no longer rounds back onto itself"
        );

        let moved = perturb_shares(&components, booking);

        assert_ne!(moved[0], components[0]);
        assert_eq!(moved.iter().sum::<Decimal>(), perturb(booking));
    }

    /// The component the remainder lands on is moved off the export's figure too, and the
    /// components still sum to the moved booking [TST-012], [TST-031]. This group is the shape
    /// that leaked: two sub-cent shares whose remainder carried the larger one back onto itself.
    #[test]
    fn the_component_carrying_the_remainder_is_moved_off_the_export_too() {
        let booking = decimal("0.21");
        let components = [decimal("-0.04"), decimal("0.25")];

        let moved = perturb_shares(&components, booking);

        assert_eq!(moved.iter().sum::<Decimal>(), perturb(booking));
        for (component, moved) in components.iter().zip(&moved) {
            assert_ne!(moved, component, "{component} came through unchanged");
        }
    }

    /// A parent of zero has no factor, so the amount falls back on its own magnitude.
    #[test]
    fn a_linked_amount_without_a_parent_moves_on_its_own_magnitude() {
        let amount = decimal("12.15");
        assert_eq!(
            perturb_like(amount, Decimal::ZERO, Decimal::ZERO),
            perturb(amount)
        );
    }

    /// The degenerate groups: one component is the whole booking, and an empty group has nothing
    /// to carry the remainder.
    #[test]
    fn a_single_component_is_the_whole_booking_and_an_empty_group_stays_empty() {
        let booking = decimal("-5.47");
        let moved = perturb_shares(&[booking], booking);
        assert_eq!(moved, vec![perturb(booking)]);
        assert_eq!(perturb_shares(&[], booking), Vec::<Decimal>::new());
    }

    /// Re-running the script produces the same fixtures.
    #[test]
    fn perturbation_is_reproducible() {
        assert_eq!(perturb(decimal("1839.24")), perturb(decimal("1839.24")));
    }

    /// An amount too small to move by a tenth still moves.
    #[test]
    fn a_tiny_amount_still_moves() {
        let tiny = decimal("0.01");
        assert_ne!(perturb(tiny), tiny);
        assert!(!perturb(tiny).is_zero());
    }

    /// The factor never leaves the stated band.
    #[test]
    fn the_factor_stays_within_a_tenth() {
        for cents in 1..500_i64 {
            let factor = factor_for(Decimal::new(cents, 2));
            assert!(
                factor >= decimal("0.9") && factor <= decimal("1.1"),
                "{factor}"
            );
        }
    }

    /// `Decimal::from_str_exact` is what keeps a column's scale, so the helper above is the same
    /// parse the CSV reader uses.
    #[test]
    fn parsing_keeps_trailing_zeros() {
        assert_eq!(Decimal::from_str("0.030000").unwrap().scale(), 6);
    }
}
