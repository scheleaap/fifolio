//! Decimal kinds, their scales, and the one rounding rule.
//!
//! Every figure in this domain is a decimal: money, quantities and rates are never floating
//! point [ARC-006], which the workspace lint `clippy::float_arithmetic` enforces rather than
//! this paragraph.
//!
//! # Values are exact; rounding is a boundary
//!
//! A value carries whatever precision it was built with. Rounding happens when a figure is
//! stored or shown, and nowhere else [ARC-009], because rounding at every step compounds:
//! `0.004 + 0.004` is `0.01` when the sum is rounded once and `0.00` when each term is rounded
//! first. So construction does not round. [`Scaled::rounded`] does, and storage and
//! presentation are the only callers.
//!
//! Rounding is **half away from zero** [ARC-010]. Half up would send `-0.005` to `-0.00` and
//! `+0.005` to `+0.01`, so a loss and a gain of one size would not round to one magnitude. A
//! realized loss is an ordinary negative figure here, so that asymmetry would be a real one.
//!
//! # Scales
//!
//! [ARC-007] states two of the four as exact and two as upper bounds, and the difference shows:
//!
//! | Kind | Scale | |
//! | --- | --- | --- |
//! | [`Quantity`] | up to 8 | a whole number of shares stays whole |
//! | [`QuotedPrice`], [`EffectivePrice`] | up to 6 | |
//! | [`Money`] | exactly 2 | `1825.5` rounds to `1825.50`, so amounts render alike |
//! | [`FxRate`] | exactly 6 | |
//!
//! # Two kinds of unit price
//!
//! A bond is quoted as a percentage of par, so a trade value is `quantity × price × factor`
//! where the factor is 1 or 0.01. The factor is applied exactly once, when a *quoted* price
//! becomes a value; a price obtained by dividing a value by a quantity has the factor in it
//! already and must not take it again [DOM-087]. Getting that wrong is a factor of a hundred
//! in a cost basis.
//!
//! So the two are different types. [`QuotedPrice`] is what a statement shows and what a factor
//! may be applied to; [`EffectivePrice`] is what [`EffectivePrice::from_value`] produces. That
//! makes applying a factor twice take a deliberate conversion through a bare `Decimal` rather
//! than an ordinary slip — it does not make it impossible, since both types can be built from
//! one. The factor itself, and the arithmetic that must refuse an `EffectivePrice`, belong to
//! FIF-075; this item establishes the distinction the rule needs.

use rust_decimal::{Decimal, RoundingStrategy};

/// Decimals a quantity may keep. Fractional shares are ordinary: savings plans and
/// fractional-share purchases both produce them.
pub const QUANTITY_SCALE: u32 = 8;

/// Decimals a unit price may keep, quoted or effective.
pub const PRICE_SCALE: u32 = 6;

/// Decimals a monetary amount carries, exactly.
pub const MONEY_SCALE: u32 = 2;

/// Decimals an exchange rate carries, exactly.
pub const FX_RATE_SCALE: u32 = 6;

/// Round to `scale`, half away from zero [ARC-010].
///
/// The only rounding of a decimal in the crate. An exact rational, which no decimal can hold,
/// is rounded under the same rule by
/// [`crate::effective_quantity::EffectiveQuantity::at_quantity_scale`].
#[must_use]
pub fn round_to(value: Decimal, scale: u32) -> Decimal {
    value.round_dp_with_strategy(scale, RoundingStrategy::MidpointAwayFromZero)
}

/// What every decimal kind here has in common.
pub trait Scaled: Sized + Copy {
    /// Decimals this kind carries, or at most carries.
    const SCALE: u32;

    /// Whether [`Self::SCALE`] is exact, so a shorter value is padded out to it.
    const PADS: bool;

    /// The underlying decimal, exactly as built.
    fn get(self) -> Decimal;

    /// Build from a decimal without rounding.
    fn from_decimal(value: Decimal) -> Self;

    /// The value at this kind's scale: rounded, and padded when the scale is exact.
    ///
    /// This is the storage and presentation boundary [ARC-009].
    #[must_use]
    fn rounded(self) -> Self {
        let mut value = round_to(self.get(), Self::SCALE);
        if Self::PADS {
            value.rescale(Self::SCALE);
        }
        Self::from_decimal(value)
    }

    /// Whether this is zero.
    fn is_zero(self) -> bool {
        self.get().is_zero()
    }
}

/// Defines a decimal newtype.
///
/// The kinds differ only in scale and in meaning, and meaning is the point: a quantity and a
/// price are both decimals, and mixing them is a mistake the compiler can catch.
macro_rules! scaled_decimal {
    ($(#[$meta:meta])* $name:ident, $scale:expr, $pads:expr) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Decimal);

        impl $name {
            /// Wrap `value` without rounding it.
            #[must_use]
            pub const fn new(value: Decimal) -> Self {
                Self(value)
            }

            /// Zero.
            #[must_use]
            pub const fn zero() -> Self {
                Self(Decimal::ZERO)
            }
        }

        impl Scaled for $name {
            const SCALE: u32 = $scale;
            const PADS: bool = $pads;

            fn get(self) -> Decimal {
                self.0
            }

            fn from_decimal(value: Decimal) -> Self {
                Self(value)
            }
        }
    };
}

scaled_decimal!(
    /// A number of units, to at most 8 decimals.
    Quantity,
    QUANTITY_SCALE,
    false
);

scaled_decimal!(
    /// A unit price exactly as a statement shows it, before any quotation factor.
    ///
    /// For a bond this is a percentage of par: `139.46` means 139.46% and not 139.46 euros.
    QuotedPrice,
    PRICE_SCALE,
    false
);

scaled_decimal!(
    /// A unit price that already includes any quotation factor, because it was derived by
    /// dividing a value by a quantity.
    ///
    /// Produced by [`EffectivePrice::from_value`]. No factor is applied to one [DOM-087].
    EffectivePrice,
    PRICE_SCALE,
    false
);

scaled_decimal!(
    /// A monetary amount: a fee, a total, a cost basis or a gain. Negative amounts are
    /// ordinary — a realized loss is one.
    Money,
    MONEY_SCALE,
    true
);

scaled_decimal!(
    /// An exchange rate, in foreign units per EUR.
    FxRate,
    FX_RATE_SCALE,
    true
);

impl EffectivePrice {
    /// The per-unit price implied by `value` over `quantity`, exact.
    ///
    /// `None` when the division cannot be done — a zero quantity, or an overflow — rather than
    /// a panic: a closing of nothing is a data problem the caller reports, not an arithmetic
    /// one.
    ///
    /// The result is effective by construction: whatever factor `value` was computed with is
    /// already inside it [DOM-087].
    #[must_use]
    pub fn from_value(value: Money, quantity: Quantity) -> Option<Self> {
        value.get().checked_div(quantity.get()).map(Self::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    /// Each kind rounds to its own scale [ARC-007].
    #[test]
    fn each_kind_rounds_to_its_own_scale() {
        assert_eq!(
            Quantity::new(dec!(1.234567894)).rounded().get(),
            dec!(1.23456789)
        );
        assert_eq!(
            QuotedPrice::new(dec!(139.4649994)).rounded().get(),
            dec!(139.464999)
        );
        assert_eq!(
            EffectivePrice::new(dec!(0.3333333333)).rounded().get(),
            dec!(0.333333)
        );
        assert_eq!(Money::new(dec!(1825.494)).rounded().get(), dec!(1825.49));
        assert_eq!(FxRate::new(dec!(0.9114134)).rounded().get(), dec!(0.911413));
    }

    /// The scales are the ones the specification fixes [ARC-007].
    #[test]
    fn the_scales_are_the_specified_ones() {
        assert_eq!(Quantity::SCALE, 8);
        assert_eq!(QuotedPrice::SCALE, 6);
        assert_eq!(EffectivePrice::SCALE, 6);
        assert_eq!(Money::SCALE, 2);
        assert_eq!(FxRate::SCALE, 6);
    }

    /// Construction keeps whatever precision it was given; only `rounded` rounds [ARC-009].
    #[test]
    fn construction_does_not_round() {
        let exact = dec!(1825.4949999);
        assert_eq!(Money::new(exact).get(), exact);
        assert_eq!(Quantity::new(dec!(0.000000004)).get(), dec!(0.000000004));
    }

    /// Rounding once beats rounding each term, which is why construction leaves values exact
    /// [ARC-009].
    #[test]
    fn rounding_once_differs_from_rounding_each_term() {
        let a = dec!(0.004);
        let b = dec!(0.004);

        let rounded_once = Money::new(a + b).rounded().get();
        let rounded_each =
            Money::new(Money::new(a).rounded().get() + Money::new(b).rounded().get())
                .rounded()
                .get();

        assert_eq!(rounded_once, dec!(0.01));
        assert_eq!(rounded_each, dec!(0.00));
    }

    /// An exact-scale kind pads, so equal amounts render alike [ARC-007].
    #[test]
    fn an_exact_scale_kind_pads_to_its_scale() {
        assert_eq!(Money::new(dec!(1825.5)).rounded().get().scale(), 2);
        assert_eq!(
            Money::new(dec!(1825.5)).rounded().get().to_string(),
            "1825.50"
        );
        assert_eq!(Money::zero().rounded().get().to_string(), "0.00");
        assert_eq!(FxRate::new(dec!(0.9114)).rounded().get().scale(), 6);
    }

    /// An upper-bounded kind does not pad: a whole number of shares stays whole [ARC-007].
    #[test]
    fn an_upper_bounded_kind_does_not_pad() {
        assert_eq!(Quantity::new(dec!(40)).rounded().get().to_string(), "40");
        assert!(QuotedPrice::new(dec!(5.75)).rounded().get().scale() <= QuotedPrice::SCALE);
    }

    /// A quantity keeps eight decimals [ARC-007].
    #[test]
    fn a_quantity_keeps_eight_decimals() {
        let smallest = dec!(0.00000001);
        assert_eq!(Quantity::new(smallest).rounded().get(), smallest);
        assert_eq!(
            Quantity::new(dec!(0.000000004)).rounded().get(),
            Decimal::ZERO
        );
    }

    /// A half rounds away from zero in both directions, so a loss and a gain round alike
    /// [ARC-010].
    #[test]
    fn a_half_rounds_away_from_zero_in_both_directions() {
        assert_eq!(Money::new(dec!(0.005)).rounded().get(), dec!(0.01));
        assert_eq!(Money::new(dec!(-0.005)).rounded().get(), dec!(-0.01));
        assert_eq!(Money::new(dec!(2.675)).rounded().get(), dec!(2.68));
        assert_eq!(Money::new(dec!(-2.675)).rounded().get(), dec!(-2.68));
    }

    /// The strategy is load-bearing: on a negative midpoint, rounding toward zero gives a
    /// different figure from the rule this crate uses [ARC-010].
    #[test]
    fn the_rounding_strategy_changes_the_answer_on_a_loss() {
        let loss = dec!(-0.005);
        let away = loss.round_dp_with_strategy(MONEY_SCALE, RoundingStrategy::MidpointAwayFromZero);
        let toward = loss.round_dp_with_strategy(MONEY_SCALE, RoundingStrategy::MidpointTowardZero);

        assert_eq!(away, dec!(-0.01));
        assert_eq!(toward, dec!(0.00));
        assert_eq!(Money::new(loss).rounded().get(), away);
    }

    /// Either side of a midpoint rounds the obvious way.
    #[test]
    fn either_side_of_a_midpoint_rounds_the_obvious_way() {
        assert_eq!(Money::new(dec!(0.0049)).rounded().get(), dec!(0.00));
        assert_eq!(Money::new(dec!(0.0051)).rounded().get(), dec!(0.01));
        assert_eq!(Money::new(dec!(-0.0049)).rounded().get(), dec!(0.00));
        assert_eq!(Money::new(dec!(-0.0051)).rounded().get(), dec!(-0.01));
    }

    /// An effective price is a value over a quantity, exact until rounded [DOM-087].
    #[test]
    fn an_effective_price_is_a_value_over_a_quantity() {
        // 209.63 EUR over 40 units, the worked Saxo buy.
        let price = EffectivePrice::from_value(Money::new(dec!(209.63)), Quantity::new(dec!(40)))
            .expect("40 units is not zero");
        assert_eq!(price.rounded().get(), dec!(5.240750));
        // A price is bounded at 6 decimals, not padded to them: 5.24075 needs only five.
        assert!(price.rounded().get().scale() <= EffectivePrice::SCALE);
    }

    /// A non-terminating quotient is held exactly and rounds at the boundary [ARC-009].
    #[test]
    fn a_repeating_quotient_rounds_only_at_the_boundary() {
        let price = EffectivePrice::from_value(Money::new(dec!(1)), Quantity::new(dec!(3)))
            .expect("3 is not zero");
        assert!(price.get().scale() > EffectivePrice::SCALE);
        assert_eq!(price.rounded().get(), dec!(0.333333));
    }

    /// A fractional quantity at full scale divides.
    #[test]
    fn a_fractional_quantity_at_full_scale_divides() {
        // 10.14 EUR over 0.0827 units, the Trade Republic free share.
        let price =
            EffectivePrice::from_value(Money::new(dec!(10.14)), Quantity::new(dec!(0.0827)))
                .expect("0.0827 is not zero");
        assert_eq!(price.rounded().get(), dec!(122.611850));
    }

    /// A division that cannot be done yields nothing rather than panicking.
    #[test]
    fn an_impossible_division_is_none() {
        assert_eq!(
            EffectivePrice::from_value(Money::new(dec!(10)), Quantity::zero()),
            None
        );
        assert_eq!(
            EffectivePrice::from_value(
                Money::new(Decimal::MAX.trunc()),
                Quantity::new(dec!(0.00000001))
            ),
            None
        );
    }

    /// Zero is zero at every scale.
    #[test]
    fn zero_is_zero() {
        assert!(Quantity::zero().is_zero());
        assert!(Money::zero().is_zero());
        assert!(Money::new(Decimal::ZERO).rounded().is_zero());
    }
}
