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
//! as one.

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
