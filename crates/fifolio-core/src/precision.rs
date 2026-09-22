//! How precision is kept across a chain of operations [ARC-009].
//!
//! Intermediate arithmetic is exact and rounding happens at a storage or presentation boundary.
//! `decimal::Scaled::rounded` is that boundary. This module says what "exact" is worth in
//! practice, because `rust_decimal` is not unbounded.
//!
//! # The one exception
//!
//! ARC-009 names it in the same breath: allocation shares. [DOM-061] requires each share to be
//! rounded to 2 decimals *independently*, with the drift absorbed by the last, and that rounding
//! is part of the result rather than a presentation step — it is what makes the shares sum
//! exactly to the parent figure. So the rule is: rounding happens once, at the boundary, except
//! where the drift rule makes a share's rounding part of the answer.
//!
//! # A division may be left unrounded
//!
//! A quotient of ordinary figures carries far more decimals than the domain's widest scale of 8
//! — `1/3` carries 28 — so leaving one unrounded loses nothing any figure here can represent,
//! and rounding it early would.
//!
//! # The limit is 28 *significant digits*, not 28 decimals
//!
//! Available decimals shrink as the integer part grows:
//!
//! | quotient | decimals kept |
//! | --- | --- |
//! | `1 / 3` | 28 |
//! | `1 000 000 / 3` | 23 |
//! | `10²⁰ / 3` | 9 |
//! | `10²² / 3` | 7 — **below the quantity scale** |
//!
//! Past about twenty integer digits a quotient holds fewer decimals than a quantity can carry.
//! Nothing reports this: the excess is dropped silently, with no error and no `None`. That is a
//! different mechanism from overflow, which is loud. It is unreachable with real money — 10²²
//! euros is not a portfolio — but it is the shape of the limit, not 28 decimals.
//!
//! # The limit rounds to even, which is not this crate's rule
//!
//! Reaching it rounds rather than truncates, and it rounds **half to even**, performed by
//! `rust_decimal` itself: `1 / 536870912` ends `…0312` where [ARC-010]'s half away from zero
//! would give `…0313`. ARC-010 governs `decimal::round_to` and nothing else; the implicit
//! rounding at the representation limit is outside its scope. Worth knowing precisely because
//! the two rules disagree only at an exact midpoint, which is where a test would be written.
//!
//! # A quotient does not reliably rebuild its total
//!
//! Multiplying back sometimes restores the total and sometimes does not, either side of it:
//!
//! | | round trip |
//! | --- | --- |
//! | `10 / 3 × 3` | `10` — restored |
//! | `10.14 / 0.0827 × 0.0827` | `10.14` — restored |
//! | `1 / 3 × 3` | `0.9999999999999999999999999999` — short |
//! | `1 / 7 × 7` | `1.0000000000000000000000000003` — over |
//!
//! Unpredictable is worse than consistently wrong: a test written against one pair passes while
//! the same code fails on another, and `10.14 / 0.0827` is a real position from the sample data
//! that happens to round-trip cleanly. So a total is never reconstructed by multiplying a
//! quotient back; it is taken from the figure the broker booked, which is what [DOM-085] and
//! [DOM-104] already require. This is the arithmetic reason behind that rule, not a second rule.
//!
//! # Overflow is an error, never a wrap
//!
//! `Decimal::MAX` is about 7.9 × 10²⁸. Operations that can leave the range use `checked_*` and
//! yield `None`, so an impossible figure is reported rather than produced.
//!
//! # Where a value must already be at its scale
//!
//! At two call boundaries, and nowhere else:
//!
//! * **persistence** — every value bound into a statement is already at its scale, so two equal
//!   amounts store identically; the repositories refuse one that is not rather than rounding it
//!   themselves, because a rounding the caller never performed must not be attributed to it;
//! * **presentation** — every value handed to a formatter, a serializer or a report is
//!   `.rounded()` first.
//!
//! Everywhere else a value carries the precision it was built with. A function that rounds its
//! own inputs is a defect: it makes the chain round more than once. `Scaled::rounded` returns
//! `Self`, so the type system cannot tell a rounded value from an exact one; the convention is
//! enforced at those two boundaries by the items that build them, not here.

/// Significant digits `rust_decimal` can represent.
///
/// Not a count of decimals: the decimals available to a quotient shrink as its integer part
/// grows.
pub const MAX_SIGNIFICANT_DIGITS: u32 = 28;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decimal::{EffectivePrice, Money, Quantity, Scaled};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::str::FromStr;

    /// A quotient of ordinary figures outruns every scale the domain uses, which is what makes
    /// leaving it unrounded worthwhile [ARC-009].
    #[test]
    fn a_quotient_of_ordinary_figures_outruns_every_domain_scale() {
        let third = dec!(1) / dec!(3);

        assert_eq!(third.scale(), MAX_SIGNIFICANT_DIGITS);
        assert!(third.scale() > Quantity::SCALE);
        assert!(third.scale() > Money::SCALE);
    }

    /// The limit is on significant digits, so a large enough numerator leaves a quotient with
    /// fewer decimals than a quantity can carry — silently [ARC-009].
    #[test]
    fn a_large_magnitude_quotient_keeps_fewer_decimals_than_a_quantity() {
        let ordinary = dec!(1000000) / dec!(3);
        let huge =
            Decimal::from_str("10000000000000000000000.00").expect("a valid decimal") / dec!(3);

        assert_eq!(
            ordinary.scale(),
            23,
            "decimals shrink as the integer part grows"
        );
        assert_eq!(huge.scale(), 7);
        assert!(
            huge.scale() < Quantity::SCALE,
            "past ~20 integer digits a quotient cannot hold a quantity's 8 decimals"
        );
    }

    /// At the limit the rounding is half to even, which is *not* this crate's rule. The two
    /// differ only at an exact midpoint [ARC-009], [ARC-010].
    #[test]
    fn the_limit_rounds_to_even_not_away_from_zero() {
        // 1 / 2^29 is exactly 0.00000000186264514923095703125: 29 decimals, the last a lone 5,
        // and the digit before it even. Half to even keeps the 2; half away from zero gives 3.
        let at_a_midpoint = dec!(1) / dec!(536870912);

        assert!(at_a_midpoint.to_string().ends_with("0312"));
        assert!(!at_a_midpoint.to_string().ends_with("0313"));
    }

    /// Multiplying a quotient back restores the total for some figures and not others, and can
    /// land either side. That unpredictability is why a total comes from the booked figure
    /// rather than being rebuilt [DOM-085].
    #[test]
    fn multiplying_a_quotient_back_is_unreliable_in_both_directions() {
        let short = (dec!(1) / dec!(3)) * dec!(3);
        let over = (dec!(1) / dec!(7)) * dec!(7);
        let restored = (dec!(10) / dec!(3)) * dec!(3);

        assert!(short < dec!(1), "{short} falls short");
        assert!(over > dec!(1), "{over} overshoots");
        assert_eq!(restored, dec!(10), "and this one comes back exactly");
    }

    // The tests above characterize `rust_decimal`, and would pass whatever this crate did. The
    // ones below fail if this crate stops following the policy.

    /// Construction keeps the precision it was given: this crate does not round early
    /// [ARC-009].
    #[test]
    fn this_crate_does_not_round_on_construction() {
        let exact = dec!(1825.4949999);
        let long = dec!(0.123456789012345);

        assert_eq!(Money::new(exact).get(), exact);
        assert_eq!(Quantity::new(long).get(), long);
    }

    /// A derived unit price is left exact, so the chain rounds once rather than here
    /// [ARC-009].
    #[test]
    fn a_derived_price_is_left_exact() {
        let price = EffectivePrice::from_value(Money::new(dec!(1)), Quantity::new(dec!(3)))
            .expect("3 is not zero");

        assert!(
            price.get().scale() > EffectivePrice::SCALE,
            "from_value must not round; rounding belongs to the boundary"
        );
    }

    /// The boundary is idempotent: rounding an already-rounded value changes nothing, so a
    /// value crossing two boundaries is not rounded twice into a different figure [ARC-009].
    #[test]
    fn rounding_at_the_boundary_is_idempotent() {
        let awkward = Money::new(dec!(2.675));

        assert_eq!(awkward.rounded().rounded(), awkward.rounded());
        assert_eq!(awkward.rounded().get(), dec!(2.68));
    }

    /// The same trap as above, in the domain's own terms [DOM-085].
    #[test]
    fn a_derived_unit_price_need_not_rebuild_its_total() {
        let total = Money::new(dec!(1));
        let quantity = Quantity::new(dec!(3));
        let price = EffectivePrice::from_value(total, quantity).expect("3 is not zero");

        assert_ne!(price.get() * quantity.get(), total.get());
        // Rounded for presentation the difference vanishes, which is exactly why the rule is
        // to take the total from the source rather than to round the product and hope.
        assert_eq!(
            Money::new(price.get() * quantity.get()).rounded(),
            total.rounded()
        );
    }

    /// Overflow yields nothing rather than wrapping or panicking, in this crate's own division
    /// as well as in the underlying type [ARC-009].
    #[test]
    fn overflow_is_reported_not_wrapped() {
        assert_eq!(Decimal::MAX.checked_mul(dec!(2)), None);
        assert_eq!(Decimal::MAX.checked_add(Decimal::MAX), None);
        assert_eq!(
            EffectivePrice::from_value(
                Money::new(Decimal::MAX.trunc()),
                Quantity::new(dec!(0.00000001))
            ),
            None,
            "from_value must use checked division"
        );
    }

    /// Ordinary figures sit far inside the limit, so the guard is against a defect and not
    /// against real data.
    #[test]
    fn ordinary_figures_are_nowhere_near_the_limit() {
        // A ten-million-euro position at two decimals.
        let large = dec!(10_000_000.00);

        assert!(large.checked_mul(dec!(1_000_000_000)).is_some());
        assert!(Decimal::MAX / large > dec!(1_000_000_000_000_000_000_000));
    }
}
