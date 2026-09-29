//! An opening's quantity and unit price as of a position in the canonical order, through the
//! splits between [DOM-089], [DOM-103], [DOM-113].
//!
//! # Stated figures stay as booked
//!
//! A split rewrites nothing. An opening keeps the quantity and total its statement booked, so it
//! still reconciles against that statement, and what a split changes is derived here on demand
//! [DOM-089]. Total cost is unchanged by a split, so the unit price moves inversely.
//!
//! # Why a position and not "today"
//!
//! A disposal states its quantity in the units current when it happened. Buy 10 at 100, sell 5,
//! then split 2:1: the 5 sold cost 500, and dividing the 1000 by the post-split 20 would put them
//! at 250 [DOM-103]. So the effective quantity is asked for **as of** an [`OrderKey`], and only the
//! splits strictly between the opening and that key apply (DEC-097, provisional).
//!
//! # Exact, then one rounding
//!
//! The quantity is an exact rational [DOM-113]. A one-for-three has no finite decimal expansion,
//! so rounding at each split would leave a residue that grows with every further split and could
//! stop a parcel exhausting. As a rational, successive ratios multiply out: one-for-three then
//! three-for-one gives back the stated quantity exactly, whatever it was.
//!
//! Wherever an effective quantity is compared — whether a parcel is exhausted, whether
//! allocations sum to a closing — it is taken at the 8-decimal quantity scale, half away from
//! zero, through [`EffectiveQuantity::at_quantity_scale`] and nothing else, so every comparison
//! rounds alike (DEC-091, provisional). The residue below 1e-8 of a share is dropped there.
//! Display uses the same view.
//!
//! # Which splits
//!
//! A [`Split`] does not hold its security: that is a relation storage keeps [DOM-013]. The
//! caller passes the splits of the opening's own account and security, in any order — a product
//! does not depend on it.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};
use rust_decimal::Decimal;

use crate::decimal::{EffectivePrice, Money, QUANTITY_SCALE, Quantity, Scaled};
use crate::manual_entry::Ratio;
use crate::ordering::OrderKey;
use crate::transaction::{Opening, Split};

/// An opening's quantity as of a position, exact [DOM-113].
///
/// Never stored and never rounded in place: it is derived from the stated quantity each time it
/// is asked for, and rounded only by [`Self::at_quantity_scale`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EffectiveQuantity(BigRational);

impl EffectiveQuantity {
    /// The exact value, for arithmetic that must stay exact before it is compared.
    #[must_use]
    pub fn exact(&self) -> &BigRational {
        &self.0
    }

    /// The quantity at the 8-decimal quantity scale, half away from zero [ARC-010]: the one view
    /// an effective quantity is compared and shown in (DEC-091, provisional).
    ///
    /// The residue below 1e-8 of a share is dropped, so a parcel whose remainder rounds to zero
    /// here is exhausted [DOM-064]. A whole number of shares comes back whole, as a stated
    /// quantity would [ARC-007].
    ///
    /// `None` only when the value lies beyond what a decimal can hold at that scale, which no
    /// holding reaches; reporting it beats producing a wrapped figure.
    #[must_use]
    pub fn at_quantity_scale(&self) -> Option<Quantity> {
        at_quantity_scale(&self.0)
    }

    /// The unit price at this position: `total_cost` over this quantity [DOM-089]. The total is
    /// the opening's own, unchanged by any split, so the price moves inversely to the quantity.
    ///
    /// Divided from the exact quantity, not from its rounded view, and left unrounded as
    /// [`EffectivePrice::from_value`] leaves it: the quotient keeps the full precision of a
    /// decimal and is rounded at display [ARC-009].
    ///
    /// The quotient is taken from the exact rational, not by dividing its numerator by its
    /// denominator as decimals: after a chain of co-prime splits either can outgrow a decimal
    /// while the price itself fits. It is kept at the most decimals, up to 28, that a decimal
    /// can hold for its magnitude, the last one rounded half away from zero [ARC-010].
    ///
    /// `None` for a zero quantity, or for a price too large for a decimal at any scale.
    #[must_use]
    pub fn unit_price(&self, total_cost: Money) -> Option<EffectivePrice> {
        if self.0.is_zero() {
            return None;
        }
        let price = exact(total_cost.get()) / &self.0;
        (0..=Decimal::MAX_SCALE).rev().find_map(|scale| {
            let units = (&price * BigRational::from_integer(BigInt::from(10).pow(scale)))
                .round()
                .to_integer()
                .to_i128()?;
            Decimal::try_from_i128_with_scale(units, scale)
                .ok()
                .map(|value| EffectivePrice::new(value.normalize()))
        })
    }
}

/// `opening`'s effective quantity as of `at`: its stated quantity times the ratio of every split
/// in `splits` that sorts strictly after the opening and strictly before `at` [DOM-089].
///
/// `splits` are those of the opening's account and security, in any order. Bounds are strict
/// (DEC-097, provisional): the opening is not rescaled by a split at its own position, nor is a
/// position rescaled by a split at that very position.
///
/// `None` when `at` precedes the opening: the parcel does not exist there, and answering with
/// its stated quantity would be a plausible figure for a question with no answer (DEC-097).
#[must_use]
pub fn effective_quantity<'a>(
    opening: &Opening,
    splits: impl IntoIterator<Item = &'a Split>,
    at: OrderKey,
) -> Option<EffectiveQuantity> {
    let from = opening.derivation().order_key();
    (from <= at).then(|| {
        EffectiveQuantity(
            splits
                .into_iter()
                .filter(|split| {
                    let key = split.derivation().order_key();
                    from < key && key < at
                })
                .map(|split| as_rational(split.ratio()))
                .fold(exact(opening.quantity().get()), |quantity, ratio| {
                    quantity * ratio
                }),
        )
    })
}

/// What remains unattributed of `opening` as of `at`, at the quantity scale: its effective
/// quantity there less every quantity already allocated against it [DOM-064].
///
/// Each of `allocated` is a quantity together with the position of the closing it was allocated
/// to. An allocation states its quantity in the units current at its own closing, so subtracting
/// it as stated would compare different unit scales; it is rescaled to `at` first, by the ratio
/// of the opening's effective quantities at the two positions. That ratio is exactly the splits
/// strictly between the two (DEC-097), whichever way round they lie.
///
/// The effective quantity and the rescaled allocation sum are each exact, and each is taken
/// through [`EffectiveQuantity::at_quantity_scale`] before one is subtracted from the other
/// (DEC-099, provisional). Rounding the exact difference instead disagrees at a halfway tie:
/// 12.34567891 through a one-for-two is 6.172839455, which views as 6.17283946, yet once
/// 6.17283946 is allocated the exact difference of -0.000000005 rounds to -0.00000001, and the
/// parcel would read as over-allocated by the very figure the proposal offered.
///
/// `None` when `at` or any allocation's position precedes the opening, the parcel not existing
/// there; when the opening states no quantity, leaving nothing to rescale by; or when either side
/// lies beyond what a decimal holds.
#[must_use]
pub fn unattributed_quantity<'a>(
    opening: &Opening,
    splits: impl IntoIterator<Item = &'a Split> + Clone,
    at: OrderKey,
    allocated: impl IntoIterator<Item = (Quantity, OrderKey)>,
) -> Option<Quantity> {
    let now = effective_quantity(opening, splits.clone(), at)?;
    let consumed = allocated
        .into_iter()
        .try_fold(BigRational::zero(), |sum, (quantity, closed_at)| {
            let then = effective_quantity(opening, splits.clone(), closed_at)?;
            (!then.0.is_zero()).then(|| sum + exact(quantity.get()) * &now.0 / &then.0)
        })
        .map(EffectiveQuantity)?;
    now.at_quantity_scale()?
        .get()
        .checked_sub(consumed.at_quantity_scale()?.get())
        .map(|remaining| Quantity::new(remaining.normalize()))
}

/// An exact rational at the 8-decimal quantity scale, half away from zero [ARC-010]; `None` beyond
/// what a decimal holds there. Shared with the quantities a transfer emits [DOM-115], so every
/// rational quantity in the crate is rounded by one rule.
pub(crate) fn at_quantity_scale(value: &BigRational) -> Option<Quantity> {
    // `Ratio::round` rounds half away from zero, which is ARC-010's rule; `decimal::round_to`
    // cannot be used because no decimal holds the exact value to round from.
    let units = (value * BigRational::from_integer(BigInt::from(10).pow(QUANTITY_SCALE)))
        .round()
        .to_integer()
        .to_i128()?;
    Decimal::try_from_i128_with_scale(units, QUANTITY_SCALE)
        .ok()
        .map(|value| Quantity::new(value.normalize()))
}

/// A decimal as the rational it denotes, exactly: mantissa over ten to the scale.
pub(crate) fn exact(value: Decimal) -> BigRational {
    BigRational::new(
        BigInt::from(value.mantissa()),
        BigInt::from(10).pow(value.scale()),
    )
}

pub(crate) fn as_rational(ratio: Ratio) -> BigRational {
    BigRational::new(
        BigInt::from(ratio.numerator().get()),
        BigInt::from(ratio.denominator().get()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::num::NonZeroU32;

    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use vec1::vec1;

    use crate::decimal::QuotedPrice;
    use crate::entities::{Account, Order};
    use crate::identity::{IdentitySource, identify};
    use crate::ordering::{BatchAge, Leg, RecordPosition};
    use crate::storage::RecordHandle;
    use crate::transaction::{
        Buy, BuyOrigin, DateProvenance, Derivation, TransferIn, TransferInSource,
    };
    use crate::valuation::Valued;

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 3, day).expect("a valid date")
    }

    /// A derivation on `day` from a record at `order`, so two derivations on one day can be
    /// placed either way round.
    fn on(day: u32, order: u32) -> Derivation {
        let account = Account::new("Saxo", "69900/1000000");
        let reference = format!("row-{day}-{order}");
        let record = RecordHandle::for_test(
            identify(&account, &IdentitySource::BrokerReference(&reference)),
            RecordPosition::new(Order::new(order), BatchAge::new(1)),
        );
        Derivation::new(date(day), vec1![record])
    }

    fn key(day: u32, order: u32) -> OrderKey {
        OrderKey::new(
            date(day),
            RecordPosition::new(Order::new(order), BatchAge::new(1)),
            Leg::Lead,
        )
    }

    /// `quantity` bought on `day` for `gross` EUR, no fees.
    fn buy(day: u32, quantity: Decimal, gross: Decimal) -> Opening {
        buy_with_fees(on(day, 0), quantity, gross, Decimal::ZERO)
    }

    /// `quantity` bought at `derivation`'s position for `gross` EUR plus `fees`.
    fn buy_with_fees(
        derivation: Derivation,
        quantity: Decimal,
        gross: Decimal,
        fees: Decimal,
    ) -> Opening {
        let day = derivation.trade_date();
        Opening::Buy(Buy::new(
            derivation,
            Quantity::new(quantity),
            Valued::in_eur(QuotedPrice::new(gross / quantity)),
            Valued::in_eur(Money::new(gross)),
            Valued::in_eur(Money::new(fees)),
            BuyOrigin::Purchase,
            crate::valuation::Conversion::native(day),
        ))
    }

    /// A `numerator`-for-`denominator` split on `day`.
    fn split(day: u32, numerator: u32, denominator: u32) -> Split {
        Split::new(
            on(day, 0),
            Ratio::new(
                NonZeroU32::new(numerator).expect("non-zero"),
                NonZeroU32::new(denominator).expect("non-zero"),
            ),
        )
    }

    fn scaled(quantity: &EffectiveQuantity) -> Decimal {
        quantity
            .at_quantity_scale()
            .expect("a holding fits a decimal")
            .get()
    }

    /// The worked example: buy 10 at 100, sell 5, split 2:1. As of the sale the parcel is 10 at
    /// 100, so the 5 sold cost 500; only after the split is it 20 at 50 [DOM-103], [DOM-089].
    #[test]
    fn a_disposal_before_a_split_is_measured_in_the_units_it_stated() {
        let opening = buy(1, dec!(10), dec!(1000.00));
        let splits = [split(10, 2, 1)];
        let sale = key(5, 0);

        let at_sale = effective_quantity(&opening, &splits, sale).expect("after the buy");
        assert_eq!(scaled(&at_sale), dec!(10));
        let price = at_sale.unit_price(opening.gross().eur()).expect("not zero");
        assert_eq!(price.rounded().get(), dec!(100));

        let later = effective_quantity(&opening, &splits, key(20, 0)).expect("after the buy");
        assert_eq!(scaled(&later), dec!(20));
        let price = later.unit_price(opening.gross().eur()).expect("not zero");
        assert_eq!(price.rounded().get(), dec!(50));
    }

    /// One-for-three then three-for-one gives back the stated quantity exactly, with no residue
    /// [DOM-113]. Rounding to the quantity scale at each split would not.
    #[test]
    fn successive_splits_compose_with_no_residue() {
        let opening = buy(1, dec!(1), dec!(100.00));
        let splits = [split(2, 1, 3), split(3, 3, 1)];

        let effective = effective_quantity(&opening, &splits, key(4, 0)).expect("after the buy");
        assert_eq!(
            effective.exact(),
            &BigRational::from_integer(BigInt::from(1))
        );
        // Rounded at each application instead, the chain would give 0.33333333 × 3 =
        // 0.99999999, which is what the rational avoids.
        assert_eq!(scaled(&effective), dec!(1));
    }

    /// A chain of reverse splits is held exactly and rounded once, at the quantity scale
    /// (DEC-091) [DOM-113].
    #[test]
    fn a_chain_of_reverse_splits_rounds_once() {
        let opening = buy(1, dec!(10), dec!(1000.00));
        let splits = [split(2, 1, 3), split(3, 1, 3), split(4, 1, 3)];

        let effective = effective_quantity(&opening, &splits, key(5, 0)).expect("after the buy");
        assert_eq!(
            effective.exact(),
            &BigRational::new(BigInt::from(10), BigInt::from(27))
        );
        assert_eq!(scaled(&effective), dec!(0.37037037));
    }

    /// Splits multiply, so the order the caller passes them in does not matter [DOM-089].
    #[test]
    fn the_order_splits_are_passed_in_does_not_matter() {
        let opening = buy(1, dec!(7), dec!(70.00));
        let forward = [split(2, 1, 3), split(3, 5, 2), split(4, 3, 7)];
        let backward = [split(4, 3, 7), split(3, 5, 2), split(2, 1, 3)];

        let effective = effective_quantity(&opening, &forward, key(5, 0)).expect("after the buy");
        assert_eq!(
            Some(&effective),
            effective_quantity(&opening, &backward, key(5, 0)).as_ref(),
        );
        // 7 × 1/3 × 5/2 × 3/7, so a product both orders get wrong alike still fails.
        assert_eq!(
            effective.exact(),
            &BigRational::new(BigInt::from(5), BigInt::from(2))
        );
    }

    /// The view rounds half away from zero at 8 decimals: an exact half of the last unit rounds
    /// up, where half to even would round it to zero (DEC-091) [ARC-010].
    #[test]
    fn the_quantity_scale_view_rounds_half_away_from_zero() {
        let smallest = buy(1, dec!(0.00000001), dec!(1.00));
        let halved =
            effective_quantity(&smallest, &[split(2, 1, 2)], key(3, 0)).expect("after the buy");
        assert_eq!(scaled(&halved), dec!(0.00000001));

        let three = buy(1, dec!(0.00000003), dec!(1.00));
        let halved =
            effective_quantity(&three, &[split(2, 1, 2)], key(3, 0)).expect("after the buy");
        assert_eq!(scaled(&halved), dec!(0.00000002));
    }

    /// The residue below 1e-8 of a share is dropped, so such a remainder compares as exhausted
    /// (DEC-091) [DOM-064].
    #[test]
    fn a_residue_below_the_quantity_scale_is_dropped() {
        let opening = buy(1, dec!(0.00000001), dec!(1.00));
        let effective =
            effective_quantity(&opening, &[split(2, 1, 3)], key(3, 0)).expect("after the buy");

        assert!(!effective.exact().is_zero());
        assert!(effective.at_quantity_scale().expect("fits").is_zero());
    }

    /// A whole number of shares stays whole in the view, as a stated quantity does [ARC-007].
    #[test]
    fn a_whole_quantity_stays_whole() {
        let opening = buy(1, dec!(10), dec!(1000.00));
        let effective =
            effective_quantity(&opening, &[split(2, 2, 1)], key(3, 0)).expect("after the buy");

        assert_eq!(scaled(&effective).to_string(), "20");
    }

    /// Only splits strictly between the opening and the position apply: not one before the
    /// opening, not one at or after the position (DEC-097) [DOM-089].
    #[test]
    fn only_splits_strictly_between_apply() {
        let opening = buy(5, dec!(10), dec!(1000.00));
        let splits = [
            split(1, 2, 1),  // before the buy
            split(8, 3, 1),  // between
            split(9, 5, 1),  // at the position asked about
            split(12, 7, 1), // after it
        ];
        let at = OrderKey::new(
            date(9),
            RecordPosition::new(Order::new(0), BatchAge::new(1)),
            Leg::Lead,
        );

        let effective = effective_quantity(&opening, &splits, at).expect("after the buy");
        assert_eq!(scaled(&effective), dec!(30));
    }

    /// Within one date the canonical order decides, not the date alone [DOM-089], [DOM-111].
    #[test]
    fn within_one_date_the_canonical_order_decides() {
        let opening = buy(1, dec!(10), dec!(1000.00));
        let splits = [Split::new(
            on(4, 5),
            Ratio::new(NonZeroU32::new(2).expect("non-zero"), NonZeroU32::MIN),
        )];

        let before = effective_quantity(&opening, &splits, key(4, 3)).expect("after the buy");
        let after = effective_quantity(&opening, &splits, key(4, 7)).expect("after the buy");
        assert_eq!(scaled(&before), dec!(10));
        assert_eq!(scaled(&after), dec!(20));
    }

    /// On the opening's own date the canonical order decides the lower bound too, and a split at
    /// the opening's very position does not rescale it (DEC-097) [DOM-089], [DOM-111].
    #[test]
    fn a_split_on_the_opening_date_applies_only_after_it() {
        let opening = buy_with_fees(on(5, 4), dec!(10), dec!(1000.00), Decimal::ZERO);
        let two_for_one = |order| {
            Split::new(
                on(5, order),
                Ratio::new(NonZeroU32::new(2).expect("non-zero"), NonZeroU32::MIN),
            )
        };
        let at = key(9, 0);
        let through = |split: Split| {
            scaled(&effective_quantity(&opening, &[split], at).expect("after the buy"))
        };

        assert_eq!(through(two_for_one(3)), dec!(10), "earlier in the file");
        assert_eq!(through(two_for_one(4)), dec!(10), "at the buy's own key");
        assert_eq!(through(two_for_one(5)), dec!(20), "later in the file");
    }

    /// At its own position an opening has its stated quantity; before it there is no answer
    /// (DEC-097).
    #[test]
    fn before_the_opening_there_is_no_effective_quantity() {
        let opening = buy(5, dec!(10), dec!(1000.00));

        let own = effective_quantity(&opening, &[], opening.derivation().order_key())
            .expect("at its own position");
        assert_eq!(scaled(&own), dec!(10));
        assert_eq!(effective_quantity(&opening, &[], key(4, 0)), None);
    }

    /// The stated figures are never rewritten: after deriving through a split the opening still
    /// carries its booked quantity and total [DOM-089].
    #[test]
    fn the_stated_figures_are_never_rewritten() {
        let opening = buy(1, dec!(10), dec!(1000.00));
        let before = opening.clone();

        let effective =
            effective_quantity(&opening, &[split(2, 1, 3)], key(3, 0)).expect("after the buy");
        assert_eq!(scaled(&effective), dec!(3.33333333));

        assert_eq!(opening, before);
        assert_eq!(opening.quantity().get(), dec!(10));
        assert_eq!(opening.gross().eur().get(), dec!(1000.00));
    }

    /// The unit price divides by the exact quantity, not by its rounded view: 1000 over 10/3 is
    /// exactly 300, where over 3.33333333 it is 300.0000003… [DOM-089], [DOM-113].
    #[test]
    fn the_unit_price_divides_by_the_exact_quantity() {
        let opening = buy(1, dec!(10), dec!(1000.00));
        let effective =
            effective_quantity(&opening, &[split(2, 1, 3)], key(3, 0)).expect("after the buy");

        let price = effective
            .unit_price(opening.gross().eur())
            .expect("not zero");
        assert_eq!(price.get(), dec!(300));

        let from_rounded = EffectivePrice::from_value(
            opening.gross().eur(),
            effective.at_quantity_scale().expect("fits"),
        )
        .expect("not zero");
        assert_ne!(from_rounded.get(), dec!(300));
    }

    /// A `transfer_in`'s total cost is its carried cost basis [DOM-085], and its price moves with
    /// a split as a buy's does [DOM-089].
    #[test]
    fn a_transfer_in_is_priced_from_its_cost_basis() {
        let opening = Opening::TransferIn(TransferIn::new(
            on(1, 0),
            Quantity::new(dec!(10)),
            Valued::in_eur(Money::new(dec!(1000.00))),
            Valued::in_eur(Money::new(dec!(15.00))),
            date(1),
            DateProvenance::TransferDate,
            TransferInSource::Broker,
            crate::valuation::Conversion::native(date(1)),
        ));
        let effective =
            effective_quantity(&opening, &[split(2, 2, 1)], key(3, 0)).expect("after it");

        let price = effective
            .unit_price(opening.gross().eur())
            .expect("not zero");
        assert_eq!(price.get(), dec!(50));
    }

    /// Fees are not in the total cost the price divides [DOM-059]: 1000 plus 8 in fees over 20
    /// is 50, not 50.40 [DOM-089].
    #[test]
    fn fees_are_not_in_the_unit_price() {
        let opening = buy_with_fees(on(1, 0), dec!(10), dec!(1000.00), dec!(8.00));
        let effective =
            effective_quantity(&opening, &[split(2, 2, 1)], key(3, 0)).expect("after the buy");

        let price = effective
            .unit_price(opening.gross().eur())
            .expect("not zero");
        assert_eq!(price.get(), dec!(50));
    }

    /// A price with no finite expansion keeps more precision than the price scale and rounds to
    /// it only when asked (ARC-009): 1000 over 30 [DOM-089].
    #[test]
    fn a_non_terminating_unit_price_rounds_at_the_price_scale() {
        let opening = buy(1, dec!(10), dec!(1000.00));
        let effective =
            effective_quantity(&opening, &[split(2, 3, 1)], key(3, 0)).expect("after the buy");

        let price = effective
            .unit_price(opening.gross().eur())
            .expect("not zero");
        assert!(price.get().scale() > EffectivePrice::SCALE);
        assert_eq!(price.rounded().get(), dec!(33.333333));
    }

    /// After a chain of co-prime splits the reduced price's numerator and denominator are both
    /// past what a decimal holds, yet the price is near 100 and must still come back [DOM-089].
    #[test]
    fn a_price_fits_even_when_its_reduced_fraction_does_not() {
        let near_one = || split(2, u32::MAX, u32::MAX - 1);
        let opening = buy(1, dec!(10), dec!(1000.00));
        let effective =
            effective_quantity(&opening, &[near_one(), near_one(), near_one()], key(3, 0))
                .expect("after the buy");

        let price = effective
            .unit_price(opening.gross().eur())
            .expect("a price near 100 fits a decimal");
        let ratio = Decimal::from(u32::MAX - 1) / Decimal::from(u32::MAX);
        let expected = dec!(100) * ratio * ratio * ratio;
        assert!((price.get() - expected).abs() < dec!(0.00000000000000000001));
    }

    /// Beyond what a decimal holds there is no figure, and `None` says so rather than a wrapped
    /// one [ARC-009], [DOM-113].
    #[test]
    fn a_value_beyond_a_decimal_has_no_view() {
        let opening = buy(1, dec!(1), dec!(1000.00));
        let forward = || split(2, u32::MAX, 1);
        let reverse = || split(2, 1, u32::MAX);

        // About 7.9e28 shares: an integer, but not one a decimal holds at 8 decimals.
        let three = effective_quantity(&opening, &[forward(), forward(), forward()], key(3, 0))
            .expect("after the buy");
        assert_eq!(three.at_quantity_scale(), None);
        // About 3.4e38 shares: past an i128 as well.
        let four = effective_quantity(
            &opening,
            &[forward(), forward(), forward(), forward()],
            key(3, 0),
        )
        .expect("after the buy");
        assert_eq!(four.at_quantity_scale(), None);

        // A price of 1000 × (2^32 − 1)^3, about 7.9e31.
        let tiny = effective_quantity(&opening, &[reverse(), reverse(), reverse()], key(3, 0))
            .expect("after the buy");
        assert_eq!(tiny.unit_price(opening.gross().eur()), None);
    }

    /// A zero quantity has no unit price rather than a panic.
    #[test]
    fn a_zero_quantity_has_no_unit_price() {
        let zero = EffectiveQuantity(BigRational::zero());

        assert_eq!(zero.unit_price(Money::new(dec!(1.00))), None);
    }

    /// Buy 10, sell 4, split 2:1: 4 of the pre-split units are 8 after it, so 12 remain, not 16
    /// [DOM-064]. Before the split the same allocation leaves 6.
    #[test]
    fn an_allocation_is_rescaled_to_the_position_asked_about() {
        let opening = buy(1, dec!(10), dec!(1000.00));
        let splits = [split(10, 2, 1)];
        let allocated = [(Quantity::new(dec!(4)), key(5, 0))];

        let after =
            unattributed_quantity(&opening, &splits, key(20, 0), allocated).expect("after the buy");
        assert_eq!(after, Quantity::new(dec!(12)));
        let before =
            unattributed_quantity(&opening, &splits, key(6, 0), allocated).expect("after the buy");
        assert_eq!(before, Quantity::new(dec!(6)));
    }

    /// 10 through a one-for-three is 10/3; after 3.33333333 is allocated the 1/3 × 1e-8 left
    /// is below the quantity scale, so the parcel is exhausted (DEC-091) [DOM-064], [DOM-113].
    #[test]
    fn a_remainder_below_the_quantity_scale_views_as_zero() {
        let opening = buy(1, dec!(10), dec!(1000.00));
        let splits = [split(2, 1, 3)];
        let allocated = [(Quantity::new(dec!(3.33333333)), key(5, 0))];

        let remaining =
            unattributed_quantity(&opening, &splits, key(6, 0), allocated).expect("after the buy");
        assert_eq!(remaining, Quantity::zero());
    }

    /// 12.34567891 through a one-for-two is 6.172839455 exactly, a halfway tie that views as
    /// 6.17283946. Allocating that view exhausts the parcel rather than over-allocating it by the
    /// rounded -0.000000005 (DEC-091, DEC-099) [DOM-064].
    #[test]
    fn allocating_a_halfway_view_exhausts_the_parcel() {
        let opening = buy(1, dec!(12.34567891), dec!(1234.57));
        let splits = [split(2, 1, 2)];
        let allocated = [(Quantity::new(dec!(6.17283946)), key(5, 0))];

        assert_eq!(
            unattributed_quantity(&opening, &splits, key(5, 0), []),
            Some(Quantity::new(dec!(6.17283946)))
        );
        assert_eq!(
            unattributed_quantity(&opening, &splits, key(6, 0), allocated),
            Some(Quantity::zero())
        );
    }

    /// An allocation to a closing before the opening, or any position before it, has no answer
    /// (DEC-097).
    #[test]
    fn a_position_before_the_opening_has_no_remainder() {
        let opening = buy(5, dec!(10), dec!(1000.00));

        assert_eq!(unattributed_quantity(&opening, &[], key(1, 0), []), None);
        assert_eq!(
            unattributed_quantity(
                &opening,
                &[],
                key(9, 0),
                [(Quantity::new(dec!(1)), key(2, 0))]
            ),
            None
        );
    }

    /// Split properties over generated quantities and ratios [TST-010].
    mod properties {
        use super::*;

        use proptest::prelude::*;

        /// A positive quantity of up to 8 decimals, as a statement may state one [ARC-007].
        fn quantity() -> impl Strategy<Value = Decimal> {
            (1_i64..=1_000_000_000_000_000, 0..=QUANTITY_SCALE)
                .prop_map(|(mantissa, scale)| Decimal::new(mantissa, scale))
        }

        fn ratio() -> impl Strategy<Value = NonZeroU32> {
            any::<NonZeroU32>()
        }

        proptest! {
            /// Before any split the effective quantity is the stated one; after it, the stated
            /// quantity times the ratio, with the total cost untouched [DOM-089], [TST-010].
            #[test]
            fn a_split_scales_the_quantity_and_keeps_the_total_cost(
                stated in quantity(),
                numerator in ratio(),
                denominator in ratio(),
            ) {
                let opening = buy(1, stated, dec!(1000.00));
                let before = opening.clone();
                let splits = [Split::new(on(5, 0), Ratio::new(numerator, denominator))];

                let prior = effective_quantity(&opening, &splits, key(3, 0)).expect("after it");
                prop_assert_eq!(prior.exact(), &exact(stated));

                let later = effective_quantity(&opening, &splits, key(7, 0)).expect("after it");
                prop_assert_eq!(
                    later.exact(),
                    &(exact(stated)
                        * BigRational::new(
                            BigInt::from(numerator.get()),
                            BigInt::from(denominator.get()),
                        ))
                );
                prop_assert_eq!(&opening, &before);
                prop_assert_eq!(opening.gross().eur().get(), dec!(1000.00));
            }

            /// One-for-three then three-for-one returns the stated quantity exactly, whatever it
            /// is [DOM-113], [TST-010].
            #[test]
            fn one_for_three_then_three_for_one_leaves_no_residue(stated in quantity()) {
                let opening = buy(1, stated, dec!(1000.00));
                let splits = [split(2, 1, 3), split(3, 3, 1)];

                let effective =
                    effective_quantity(&opening, &splits, key(4, 0)).expect("after the buy");
                prop_assert_eq!(effective.exact(), &exact(stated));
                prop_assert_eq!(scaled(&effective), stated);
            }

            /// Under no splits, and under a split followed by its inverse, the effective
            /// quantity is the stated one [DOM-089], [DOM-113], [TST-010].
            #[test]
            fn a_split_and_its_inverse_leave_the_stated_quantity(
                stated in quantity(),
                numerator in ratio(),
                denominator in ratio(),
            ) {
                let opening = buy(1, stated, dec!(1000.00));
                let none = effective_quantity(&opening, &[], key(4, 0)).expect("after the buy");
                prop_assert_eq!(none.exact(), &exact(stated));

                let splits = [
                    Split::new(on(2, 0), Ratio::new(numerator, denominator)),
                    Split::new(on(3, 0), Ratio::new(denominator, numerator)),
                ];
                let effective =
                    effective_quantity(&opening, &splits, key(4, 0)).expect("after the buy");
                prop_assert_eq!(effective.exact(), &exact(stated));
                prop_assert_eq!(scaled(&effective), stated);
            }
        }
    }
}
