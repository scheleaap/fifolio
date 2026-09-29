//! An allocation's money: cost, buy fee, proceeds, sell fee and gain, derived from its parent
//! transactions on demand [DOM-058], by the formulas of [DOM-059].
//!
//! Pure: an allocation stores an opening, a closing and a quantity, and nothing here stores
//! anything either. Every figure is recomputed from the parents each time it is asked for, so it
//! cannot drift from them; DOM-069 keeps the parents still while an attribution stands.
//!
//! # Two sides, two divisors
//!
//! The opening side (cost, buy fee) divides by the opening's effective quantity **as of the
//! closing** [DOM-103]: the allocation's quantity is in the units current there, and dividing by
//! today's would misprice every unit sold before a split. The closing side (proceeds, sell fee)
//! divides by the closing's own stated quantity (DEC-100, provisional).
//!
//! A sell fee is spread over the allocations of its own closing and nowhere else [DOM-060]:
//! [`closing_shares`] reads one closing and its own allocations, so it has no way to reach a
//! parcel the closing did not touch.
//!
//! # The drift rule
//!
//! Each share is the exact quotient rounded once to 2 decimals, half away from zero [ARC-010],
//! independently of the others; the last share is not divided at all but is the parent figure
//! less the rounded shares before it, so the shares sum exactly to the parent [DOM-061]. That
//! rounding is part of the answer, the one exception ARC-009 names (see [`crate::precision`]).
//!
//! * Opening side: the last share is the allocation that **exhausts** the parcel [DOM-062], its
//!   remainder reaching zero at the quantity scale (DEC-091). A parcel not yet exhausted has no
//!   last share, and what its shares leave over stays with its unsold units.
//! * Closing side: the last share is the closing's last allocation in canonical order [DOM-063],
//!   which is the order [`crate::fifo`] proposes in: the openings' [`OrderKey`], row id breaking
//!   a tie (DEC-095).
//!
//! Quotients are taken on exact rationals, not decimals. A decimal quotient is cut at 28
//! significant digits, rounding half to even, and a cut that lands on `…5000` would then round
//! the wrong way at 2 decimals; the effective quantity is an exact rational anyway [DOM-113].
//!
//! # Which closings
//!
//! Cash closings only. A `transfer_out` has no proceeds: its gross is the basis it carries onward,
//! derived from its own allocations (FIF-080, DOM-112), and it realizes no gain [DOM-093]. Its
//! opening side is ordinary and is what it carries. An `expiration` states no quantity to divide
//! by (FIF-079, DOM-092). Both are refused on the closing side rather than answered.
//!
//! # Which half
//!
//! Shares derive from a transaction's native figures exactly as from its EUR ones and are never
//! stored for either [DOM-084]; [`Half`] says which is read. The tax figures are the EUR ones.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};
use rust_decimal::Decimal;
use thiserror::Error;

use crate::decimal::{MONEY_SCALE, Money, Quantity, Scaled};
use crate::effective_quantity::{effective_quantity, exact, unattributed_quantity};
use crate::ordering::OrderKey;
use crate::storage::TransactionId;
use crate::transaction::{Closing, Opening, Split};
use crate::valuation::Valued;

/// Which half of a native/EUR pair a share is derived from [DOM-084].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Half {
    Native,
    Eur,
}

impl Half {
    fn of(self, pair: Valued<Money>) -> Money {
        match self {
            Self::Native => pair.native(),
            Self::Eur => pair.eur(),
        }
    }
}

/// An allocation against an opening, as the opening sees it: the closing it belongs to, where
/// that closing sits, and the quantity, in the units current there [DOM-103].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgainstOpening {
    closing: TransactionId,
    closed_at: OrderKey,
    quantity: Quantity,
}

impl AgainstOpening {
    #[must_use]
    pub fn new(closing: TransactionId, closed_at: OrderKey, quantity: Quantity) -> Self {
        Self {
            closing,
            closed_at,
            quantity,
        }
    }
}

/// An allocation of a closing, as the closing sees it: the opening it consumes, where that
/// opening sits, and the quantity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfClosing {
    opening: TransactionId,
    opened_at: OrderKey,
    quantity: Quantity,
}

impl OfClosing {
    #[must_use]
    pub fn new(opening: TransactionId, opened_at: OrderKey, quantity: Quantity) -> Self {
        Self {
            opening,
            opened_at,
            quantity,
        }
    }
}

/// An allocation's share of its opening's cost and fees [DOM-059].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpeningShares {
    cost: Money,
    buy_fee: Money,
}

impl OpeningShares {
    #[must_use]
    pub fn cost(&self) -> Money {
        self.cost
    }

    #[must_use]
    pub fn buy_fee(&self) -> Money {
        self.buy_fee
    }
}

/// An allocation's share of its closing's proceeds and fees [DOM-059].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosingShares {
    proceeds: Money,
    sell_fee: Money,
}

impl ClosingShares {
    #[must_use]
    pub fn proceeds(&self) -> Money {
        self.proceeds
    }

    #[must_use]
    pub fn sell_fee(&self) -> Money {
        self.sell_fee
    }
}

/// The four rounded shares of one allocation of a cash closing, and the gain they make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Figures {
    opening: OpeningShares,
    closing: ClosingShares,
}

impl Figures {
    /// Both sides of one allocation, read from the same [`Half`].
    #[must_use]
    pub fn new(opening: OpeningShares, closing: ClosingShares) -> Self {
        Self { opening, closing }
    }

    #[must_use]
    pub fn cost(&self) -> Money {
        self.opening.cost
    }

    #[must_use]
    pub fn buy_fee(&self) -> Money {
        self.opening.buy_fee
    }

    #[must_use]
    pub fn proceeds(&self) -> Money {
        self.closing.proceeds
    }

    #[must_use]
    pub fn sell_fee(&self) -> Money {
        self.closing.sell_fee
    }

    /// Proceeds less sell fee, cost and buy fee [DOM-059].
    ///
    /// Arithmetic on the four **rounded** shares, deliberately not the exact gain rounded
    /// afterwards [DOM-125], DEC-066. It can be up to two cents from the exact figure per
    /// allocation, and that is the price of every row reconciling to its own columns and every
    /// report total to the rows above it. Computing it exactly would be "more accurate" and is
    /// the reading that was rejected; do not change it.
    #[must_use]
    pub fn gain(&self) -> Money {
        Money::new(
            self.proceeds().get()
                - self.sell_fee().get()
                - self.cost().get()
                - self.buy_fee().get(),
        )
    }
}

/// Why an allocation's figures cannot be derived.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AllocationError {
    /// An `expiration` states no quantity to divide its gross and fees by [DOM-092], FIF-079.
    #[error("an expiration's quantity is the unattributed remainder, which is not yet decided")]
    QuantityNotStated,
    /// A `transfer_out` has no proceeds: the gross it carries onward is derived from its own
    /// allocations (FIF-080, DOM-112), and it realizes no gain [DOM-093].
    #[error("a transfer out has no proceeds and realizes no gain")]
    NoProceeds,
    /// A closing of nothing leaves nothing to divide by [DOM-114].
    #[error("the closing's quantity {quantity} is not positive")]
    NothingClosed { quantity: Decimal },
    /// The allocations given do not sum to the closing's quantity [DOM-065], so the last of them
    /// is not the closing's last and would absorb a drift that is not its own.
    #[error("the allocations sum to {allocated}, not to the closing's {closed}")]
    Incomplete { allocated: Decimal, closed: Decimal },
    /// The opening's effective quantity as of `closing` cannot be divided by: the closing
    /// precedes the opening, the quantity is zero, or a figure is beyond what a decimal holds.
    #[error("the allocation to closing {closing} cannot be measured against its opening")]
    Unmeasurable { closing: TransactionId },
    /// A share lies beyond what a decimal holds, which no holding reaches; reporting it beats a
    /// wrapped figure.
    #[error("an allocated share is beyond what a decimal holds")]
    Unrepresentable,
    /// The opening is allocated past what it holds by the allocation to `closing` [DOM-064].
    #[error("the allocation to closing {closing} over-allocates its opening")]
    OverAllocated { closing: TransactionId },
}

/// A closing's allocations that do not sum to its quantity [DOM-065].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("the allocations sum to {allocated}, not to the closing's {closed}")]
pub struct Uncovered {
    pub allocated: Decimal,
    pub closed: Decimal,
}

/// Whether `allocated` sums exactly to `closed`, the closing's stated quantity viewed at the
/// quantity scale (DEC-091, DEC-100) [DOM-065].
///
/// The one statement of the sum rule: the closing side below reads it, and so does approving an
/// attribution ([`crate::attribution`]), so a closing that can be approved is one whose shares
/// can be derived. Allocated quantities are summed as given; storage refuses one finer than the
/// quantity scale [ARC-010].
///
/// # Errors
///
/// [`Uncovered`], carrying both sums, when they differ.
pub fn covered(
    closed: Quantity,
    allocated: impl IntoIterator<Item = Quantity>,
) -> Result<(), Uncovered> {
    let allocated: Decimal = allocated.into_iter().map(Quantity::get).sum();
    let closed = closed.rounded().get();
    if allocated == closed {
        Ok(())
    } else {
        Err(Uncovered { allocated, closed })
    }
}

/// The cost and buy fee share of each allocation against `opening`, keyed by closing, in the
/// canonical order of the closings [DOM-059], [DOM-062].
///
/// `allocations` must be **every** allocation against the opening, in any order, and `splits`
/// those of its account and security: which allocation exhausts the parcel, and so absorbs the
/// drift, is only known from all of them.
///
/// # Errors
///
/// [`AllocationError::Unmeasurable`] or [`AllocationError::OverAllocated`], naming the closing,
/// or [`AllocationError::Unrepresentable`].
pub fn opening_shares(
    opening: &Opening,
    splits: &[Split],
    allocations: &[AgainstOpening],
    half: Half,
) -> Result<Vec<(TransactionId, OpeningShares)>, AllocationError> {
    let mut sorted = allocations.to_vec();
    sorted.sort_by_key(|allocation| (allocation.closed_at, allocation.closing));

    let mut cost = Drift::new(half.of(opening.gross()));
    let mut fees = Drift::new(half.of(opening.fees()));
    let mut shares = Vec::with_capacity(sorted.len());
    for (index, allocation) in sorted.iter().enumerate() {
        let unmeasurable = AllocationError::Unmeasurable {
            closing: allocation.closing,
        };
        let divisor = effective_quantity(opening, splits, allocation.closed_at)
            .map(|quantity| quantity.exact().clone())
            .filter(|quantity| !quantity.is_zero())
            .ok_or_else(|| unmeasurable.clone())?;
        // What is left once this allocation and every one before it are taken, at the quantity
        // scale as the FIFO proposal measures it, so a parcel it saw exhausted is one this sees
        // exhausted (DEC-091, DEC-099).
        let remaining = unattributed_quantity(
            opening,
            splits,
            allocation.closed_at,
            sorted[..=index]
                .iter()
                .map(|earlier| (earlier.quantity, earlier.closed_at)),
        )
        .ok_or(unmeasurable)?
        .get();
        if remaining < Decimal::ZERO {
            return Err(AllocationError::OverAllocated {
                closing: allocation.closing,
            });
        }
        let last = remaining.is_zero();
        shares.push((
            allocation.closing,
            OpeningShares {
                cost: cost
                    .share(allocation.quantity, &divisor, last)
                    .ok_or(AllocationError::Unrepresentable)?,
                buy_fee: fees
                    .share(allocation.quantity, &divisor, last)
                    .ok_or(AllocationError::Unrepresentable)?,
            },
        ));
    }
    Ok(shares)
}

/// The proceeds and sell fee share of each allocation of `closing`, keyed by opening, in
/// canonical order of the openings [DOM-059], [DOM-060], [DOM-063].
///
/// `allocations` are the closing's own, in any order; they must sum to its quantity [DOM-065].
///
/// # Errors
///
/// [`AllocationError::NoProceeds`] for a `transfer_out`, [`AllocationError::QuantityNotStated`]
/// for an `expiration`, [`AllocationError::NothingClosed`] for a closing of nothing, and
/// [`AllocationError::Incomplete`] when the allocations do not cover the closing, or
/// [`AllocationError::Unrepresentable`].
pub fn closing_shares(
    closing: &Closing,
    allocations: &[OfClosing],
    half: Half,
) -> Result<Vec<(TransactionId, ClosingShares)>, AllocationError> {
    let (quantity, gross) = match closing {
        Closing::Sell(sell) => (sell.quantity(), sell.gross()),
        Closing::Expiration(_) => return Err(AllocationError::QuantityNotStated),
        Closing::TransferOut(_) => return Err(AllocationError::NoProceeds),
    };
    if quantity.get() <= Decimal::ZERO {
        return Err(AllocationError::NothingClosed {
            quantity: quantity.get(),
        });
    }
    covered(quantity, allocations.iter().map(|a| a.quantity)).map_err(
        |Uncovered { allocated, closed }| AllocationError::Incomplete { allocated, closed },
    )?;

    let mut sorted = allocations.to_vec();
    sorted.sort_by_key(|allocation| (allocation.opened_at, allocation.opening));

    let divisor = exact(quantity.get());
    let mut proceeds = Drift::new(half.of(gross));
    let mut fees = Drift::new(half.of(closing.fees()));
    let count = sorted.len();
    sorted
        .iter()
        .enumerate()
        .map(|(index, allocation)| {
            let last = index + 1 == count;
            Ok((
                allocation.opening,
                ClosingShares {
                    proceeds: proceeds
                        .share(allocation.quantity, &divisor, last)
                        .ok_or(AllocationError::Unrepresentable)?,
                    sell_fee: fees
                        .share(allocation.quantity, &divisor, last)
                        .ok_or(AllocationError::Unrepresentable)?,
                },
            ))
        })
        .collect()
}

/// One parent figure being shared out, and how much of it the shares so far have taken.
struct Drift {
    parent: Money,
    distributed: Decimal,
}

impl Drift {
    fn new(parent: Money) -> Self {
        // A stored figure is already at the money scale (DEC-067); rounding here only pads it,
        // so the last share comes out at 2 decimals like the rest.
        Self {
            parent: parent.rounded(),
            distributed: Decimal::ZERO,
        }
    }

    /// The share of `quantity` over `divisor`, rounded on its own [DOM-061]; or, for the `last`,
    /// whatever the shares before it left of the parent, which absorbs their drift.
    ///
    /// Neither the last share nor the running total is bounded: when the earlier shares round
    /// up past the parent, the last share is negative, and a parcel not yet exhausted may have
    /// handed out more than it has (DEC-101, provisional). Clamping would re-round shares
    /// already given, and on the opening side those are earlier closings' figures.
    fn share(&mut self, quantity: Quantity, divisor: &BigRational, last: bool) -> Option<Money> {
        let share = if last {
            self.parent.get().checked_sub(self.distributed)?
        } else {
            to_money(exact(self.parent.get()) * exact(quantity.get()) / divisor)?
        };
        self.distributed = self.distributed.checked_add(share)?;
        Some(Money::new(share))
    }
}

/// `value` at the money scale, half away from zero [ARC-010]. `Ratio::round` rounds half away
/// from zero; [`crate::decimal::round_to`] cannot be used because no decimal holds the exact
/// value to round from.
fn to_money(value: BigRational) -> Option<Decimal> {
    let cents = (value * BigRational::from_integer(BigInt::from(10).pow(MONEY_SCALE)))
        .round()
        .to_integer()
        .to_i128()?;
    Decimal::try_from_i128_with_scale(cents, MONEY_SCALE).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::num::NonZeroU32;

    use chrono::NaiveDate;
    use proptest::prelude::*;
    use rust_decimal_macros::dec;
    use vec1::vec1;

    use crate::decimal::{FxRate, QuotedPrice};
    use crate::entities::{Account, Order};
    use crate::identity::{IdentitySource, identify};
    use crate::manual_entry::Ratio;
    use crate::ordering::{BatchAge, RecordPosition};
    use crate::storage::RecordHandle;
    use crate::transaction::{
        Buy, BuyOrigin, DateProvenance, Derivation, Expiration, Sell, TransferIn, TransferInSource,
        TransferOut,
    };
    use crate::valuation::{Conversion, Currency, RateSource};

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 3, day).expect("a valid date")
    }

    fn on(day: u32, order: u32) -> Derivation {
        let account = Account::new("Saxo", "69900/1000000");
        let reference = format!("row-{day}-{order}");
        let record = RecordHandle::for_test(
            identify(&account, &IdentitySource::BrokerReference(&reference)),
            RecordPosition::new(Order::new(order), BatchAge::new(1)),
        );
        Derivation::new(date(day), vec1![record])
    }

    fn key(day: u32) -> OrderKey {
        on(day, 0).order_key()
    }

    fn money(value: Decimal) -> Money {
        Money::new(value)
    }

    fn id(value: i64) -> TransactionId {
        TransactionId::new(value)
    }

    /// A buy in EUR on `day`, of `quantity` for `gross` plus `fees`.
    fn buy(day: u32, quantity: Decimal, gross: Decimal, fees: Decimal) -> Opening {
        Opening::Buy(Buy::new(
            on(day, 0),
            Quantity::new(quantity),
            Valued::in_eur(QuotedPrice::new(dec!(1))),
            Valued::in_eur(money(gross)),
            Valued::in_eur(money(fees)),
            BuyOrigin::Purchase,
            Conversion::native(date(day)),
        ))
    }

    /// A sell in EUR on `day`, of `quantity` for `gross` less `fees`.
    fn sell(day: u32, quantity: Decimal, gross: Decimal, fees: Decimal) -> Closing {
        Closing::Sell(Sell::new(
            on(day, 0),
            Quantity::new(quantity),
            Valued::in_eur(QuotedPrice::new(dec!(1))),
            Valued::in_eur(money(gross)),
            Valued::in_eur(money(fees)),
            Conversion::native(date(day)),
        ))
    }

    fn split(day: u32, numerator: u32, denominator: u32) -> Split {
        Split::new(
            on(day, 0),
            Ratio::new(
                NonZeroU32::new(numerator).expect("non-zero"),
                NonZeroU32::new(denominator).expect("non-zero"),
            ),
        )
    }

    /// `quantity` of the opening allocated to closing `closing`, which sits on `day`.
    fn against(closing: i64, day: u32, quantity: Decimal) -> AgainstOpening {
        AgainstOpening::new(id(closing), key(day), Quantity::new(quantity))
    }

    /// `quantity` of opening `opening`, which sits on `day`, allocated to the closing.
    fn of(opening: i64, day: u32, quantity: Decimal) -> OfClosing {
        OfClosing::new(id(opening), key(day), Quantity::new(quantity))
    }

    fn costs(shares: &[(TransactionId, OpeningShares)]) -> Vec<(i64, Decimal, Decimal)> {
        shares
            .iter()
            .map(|(closing, share)| (closing.get(), share.cost().get(), share.buy_fee().get()))
            .collect()
    }

    fn proceeds(shares: &[(TransactionId, ClosingShares)]) -> Vec<(i64, Decimal, Decimal)> {
        shares
            .iter()
            .map(|(opening, share)| {
                (
                    opening.get(),
                    share.proceeds().get(),
                    share.sell_fee().get(),
                )
            })
            .collect()
    }

    /// A division that comes out exact leaves no drift on either side [DOM-059], [DOM-061],
    /// [TST-016].
    #[test]
    fn an_exact_division_leaves_no_drift() {
        let opening = buy(1, dec!(10), dec!(1000.00), dec!(10.00));
        let opened = opening_shares(
            &opening,
            &[],
            &[against(20, 5, dec!(4)), against(21, 6, dec!(6))],
            Half::Eur,
        );
        assert_eq!(
            opened.as_deref().map(costs),
            Ok(vec![
                (20, dec!(400.00), dec!(4.00)),
                (21, dec!(600.00), dec!(6.00)),
            ])
        );

        let closing = sell(10, dec!(10), dec!(1500.00), dec!(5.00));
        let closed = closing_shares(&closing, &[of(1, 1, dec!(4)), of(2, 2, dec!(6))], Half::Eur);
        assert_eq!(
            closed.as_deref().map(proceeds),
            Ok(vec![
                (1, dec!(600.00), dec!(2.00)),
                (2, dec!(900.00), dec!(3.00)),
            ])
        );
    }

    /// 100.00 in thirds rounds to 33.33 twice, and the last share takes 33.34 so the three sum
    /// to the parent [DOM-061]. On the opening side the last is the allocation that exhausts the
    /// parcel, however the allocations are passed [DOM-062]; on the closing side it is the
    /// closing's last allocation in canonical order [DOM-063], [TST-016].
    #[test]
    fn a_division_leaving_one_cent_puts_it_on_the_last_share() {
        let opening = buy(1, dec!(3), dec!(100.00), dec!(1.00));
        let opened = opening_shares(
            &opening,
            &[],
            &[
                against(22, 7, dec!(1)),
                against(20, 5, dec!(1)),
                against(21, 6, dec!(1)),
            ],
            Half::Eur,
        );
        assert_eq!(
            opened.as_deref().map(costs),
            Ok(vec![
                (20, dec!(33.33), dec!(0.33)),
                (21, dec!(33.33), dec!(0.33)),
                (22, dec!(33.34), dec!(0.34)),
            ])
        );

        let closing = sell(10, dec!(3), dec!(100.00), dec!(1.00));
        let closed = closing_shares(
            &closing,
            &[of(3, 3, dec!(1)), of(1, 1, dec!(1)), of(2, 2, dec!(1))],
            Half::Eur,
        );
        assert_eq!(
            closed.as_deref().map(proceeds),
            Ok(vec![
                (1, dec!(33.33), dec!(0.33)),
                (2, dec!(33.33), dec!(0.33)),
                (3, dec!(33.34), dec!(0.34)),
            ])
        );
    }

    /// 1.00 in sixteenths is 0.0625, which rounds to 0.06 fifteen times; the last share absorbs
    /// all four cents of drift and is 0.10 [DOM-061], [DOM-062], [DOM-063], [TST-016].
    #[test]
    fn a_division_leaving_many_cents_puts_them_all_on_the_last_share() {
        let sixteenth = |share: Decimal| {
            std::iter::repeat_n(dec!(0.06), 15)
                .chain([share])
                .collect::<Vec<_>>()
        };

        let opening = buy(1, dec!(16), dec!(1.00), dec!(1.00));
        let allocations: Vec<_> = (1..=16_u32)
            .map(|n| against(100 + i64::from(n), 1 + n, dec!(1)))
            .collect();
        let opened = opening_shares(&opening, &[], &allocations, Half::Eur).expect("measurable");
        assert_eq!(
            opened
                .iter()
                .map(|(_, s)| s.cost().get())
                .collect::<Vec<_>>(),
            sixteenth(dec!(0.10))
        );
        assert_eq!(
            opened
                .iter()
                .map(|(_, s)| s.buy_fee().get())
                .collect::<Vec<_>>(),
            sixteenth(dec!(0.10))
        );

        let closing = sell(20, dec!(16), dec!(1.00), dec!(1.00));
        let allocations: Vec<_> = (1..=16_u32).map(|n| of(i64::from(n), n, dec!(1))).collect();
        let closed = closing_shares(&closing, &allocations, Half::Eur).expect("covered");
        assert_eq!(
            closed
                .iter()
                .map(|(_, s)| s.proceeds().get())
                .collect::<Vec<_>>(),
            sixteenth(dec!(0.10))
        );
        assert_eq!(
            closed
                .iter()
                .map(|(_, s)| s.sell_fee().get())
                .collect::<Vec<_>>(),
            sixteenth(dec!(0.10))
        );
    }

    /// Buy 3 for 10.00 with 0.01 fees, sell 1 of them for 5.00. The shares are 3.33 cost, 0.00
    /// buy fee, 5.00 proceeds and 0.00 sell fee, so the gain is 1.67; the exact gain
    /// 5 - 10/3 - 0.01/3 = 1.66333… would round to 1.66. The rounded shares win [DOM-125].
    #[test]
    fn a_gain_is_arithmetic_on_the_rounded_shares() {
        let opening = buy(1, dec!(3), dec!(10.00), dec!(0.01));
        let closing = sell(5, dec!(1), dec!(5.00), dec!(0.00));

        let opened = opening_shares(&opening, &[], &[against(2, 5, dec!(1))], Half::Eur)
            .expect("measurable");
        let closed = closing_shares(&closing, &[of(1, 1, dec!(1))], Half::Eur).expect("covered");
        let figures = Figures::new(opened[0].1, closed[0].1);

        assert_eq!(
            (
                figures.proceeds().get(),
                figures.sell_fee().get(),
                figures.cost().get(),
                figures.buy_fee().get(),
            ),
            (dec!(5.00), dec!(0.00), dec!(3.33), dec!(0.00))
        );
        assert_eq!(figures.gain(), money(dec!(1.67)));
        let exact_then_rounded =
            crate::decimal::round_to(dec!(5) - dec!(10) / dec!(3) - dec!(0.01) / dec!(3), 2);
        assert_eq!(exact_then_rounded, dec!(1.66));
    }

    /// A loss is an ordinary negative gain: 3.00 proceeds less 0.50 sell fee, 4.00 cost and 0.25
    /// buy fee is -1.75 [DOM-059].
    #[test]
    fn a_loss_is_a_negative_gain() {
        let opening = buy(1, dec!(1), dec!(4.00), dec!(0.25));
        let closing = sell(5, dec!(1), dec!(3.00), dec!(0.50));

        let opened = opening_shares(&opening, &[], &[against(2, 5, dec!(1))], Half::Eur)
            .expect("measurable");
        let closed = closing_shares(&closing, &[of(1, 1, dec!(1))], Half::Eur).expect("covered");

        assert_eq!(
            Figures::new(opened[0].1, closed[0].1).gain(),
            money(dec!(-1.75))
        );
    }

    /// A parcel not yet exhausted has no last share: every share is rounded on its own, and the
    /// cent left over stays with the unsold unit [DOM-062].
    #[test]
    fn a_parcel_not_exhausted_keeps_its_remainder() {
        let opening = buy(1, dec!(3), dec!(100.00), dec!(1.00));

        let opened = opening_shares(
            &opening,
            &[],
            &[against(20, 5, dec!(1)), against(21, 6, dec!(1))],
            Half::Eur,
        );

        assert_eq!(
            opened.as_deref().map(costs),
            Ok(vec![
                (20, dec!(33.33), dec!(0.33)),
                (21, dec!(33.33), dec!(0.33)),
            ])
        );
    }

    /// Buy 10 for 1000.00, sell 5, split 2:1, sell the post-split 10: the first sale's basis is
    /// 500.00, divided by the 10 current then and not by the post-split 20 [DOM-103]; the second
    /// exhausts the parcel and takes the rest [DOM-059], [DOM-062].
    #[test]
    fn the_opening_side_divides_by_the_effective_quantity_as_of_the_closing() {
        let opening = buy(1, dec!(10), dec!(1000.00), dec!(0.00));
        let splits = [split(5, 2, 1)];

        let opened = opening_shares(
            &opening,
            &splits,
            &[against(20, 3, dec!(5)), against(21, 10, dec!(10))],
            Half::Eur,
        );
        assert_eq!(
            opened.as_deref().map(costs),
            Ok(vec![
                (20, dec!(500.00), dec!(0.00)),
                (21, dec!(500.00), dec!(0.00)),
            ])
        );

        let after = opening_shares(&opening, &splits, &[against(21, 10, dec!(5))], Half::Eur);
        assert_eq!(
            after.as_deref().map(costs),
            Ok(vec![(21, dec!(250.00), dec!(0.00))])
        );
    }

    /// Through a one-for-three, 10 units for 100.00 with 0.05 fees are 10/3. Sales of 1 and 1
    /// take 30.00 and 0.015, rounded to 0.02, each. A sale of 1.33333333 leaves a residue below
    /// the quantity scale, which exhausts the parcel (DEC-091), so it takes what is left: 40.00
    /// and 0.01. Its own rounded fee share, 0.05 x 1.33333333 x 3/10 = 0.019999…, would be
    /// 0.02, so only the last-share path gives 0.01 [DOM-062], [DOM-064], [DOM-113], [TST-016].
    #[test]
    fn a_residue_below_the_quantity_scale_exhausts_the_parcel() {
        let opening = buy(1, dec!(10), dec!(100.00), dec!(0.05));
        let splits = [split(2, 1, 3)];

        let opened = opening_shares(
            &opening,
            &splits,
            &[
                against(20, 3, dec!(1)),
                against(21, 4, dec!(1)),
                against(22, 5, dec!(1.33333333)),
            ],
            Half::Eur,
        );

        assert_eq!(
            opened.as_deref().map(costs),
            Ok(vec![
                (20, dec!(30.00), dec!(0.02)),
                (21, dec!(30.00), dec!(0.02)),
                (22, dec!(40.00), dec!(0.01)),
            ])
        );
    }

    /// Two closings at one position are ordered by row id, as the FIFO proposal orders them, so
    /// the higher id exhausts the parcel and absorbs the drift, however they are passed
    /// (DEC-095) [DOM-062].
    #[test]
    fn a_tied_position_on_the_opening_side_is_decided_by_row_id() {
        let opening = buy(1, dec!(3), dec!(100.00), dec!(0.00));

        let opened = opening_shares(
            &opening,
            &[],
            &[
                against(9, 6, dec!(1)),
                against(3, 6, dec!(1)),
                against(5, 5, dec!(1)),
            ],
            Half::Eur,
        );

        assert_eq!(
            opened.as_deref().map(costs),
            Ok(vec![
                (5, dec!(33.33), dec!(0.00)),
                (3, dec!(33.33), dec!(0.00)),
                (9, dec!(33.34), dec!(0.00)),
            ])
        );
    }

    /// 1.00 in eighths is 0.125 a share, an exact tie, which rounds half away from zero to 0.13
    /// and not half to even to 0.12; the last share takes 1.00 - 7 x 0.13 = 0.09. 0.01 in halves
    /// is 0.005, which rounds to 0.01, leaving 0.00 for the last [DOM-061], [ARC-010], [TST-016].
    #[test]
    fn a_half_cent_tie_rounds_away_from_zero() {
        let eighths = |share: Decimal| {
            std::iter::repeat_n(dec!(0.13), 7)
                .chain([share])
                .collect::<Vec<_>>()
        };

        let opening = buy(1, dec!(8), dec!(1.00), dec!(0.01));
        let allocations: Vec<_> = (1..=8_u32)
            .map(|n| against(100 + i64::from(n), 1 + n, dec!(1)))
            .collect();
        let opened = opening_shares(&opening, &[], &allocations, Half::Eur).expect("measurable");
        assert_eq!(
            opened
                .iter()
                .map(|(_, s)| s.cost().get())
                .collect::<Vec<_>>(),
            eighths(dec!(0.09))
        );

        let closing = sell(20, dec!(8), dec!(1.00), dec!(0.00));
        let allocations: Vec<_> = (1..=8_u32).map(|n| of(i64::from(n), n, dec!(1))).collect();
        let closed = closing_shares(&closing, &allocations, Half::Eur).expect("covered");
        assert_eq!(
            closed
                .iter()
                .map(|(_, s)| s.proceeds().get())
                .collect::<Vec<_>>(),
            eighths(dec!(0.09))
        );

        let halves = opening_shares(
            &buy(1, dec!(2), dec!(0.01), dec!(0.00)),
            &[],
            &[against(20, 5, dec!(1)), against(21, 6, dec!(1))],
            Half::Eur,
        );
        assert_eq!(
            halves.as_deref().map(costs),
            Ok(vec![
                (20, dec!(0.01), dec!(0.00)),
                (21, dec!(0.00), dec!(0.00)),
            ])
        );
    }

    /// 0.03 in sixths is 0.005 a share, which rounds up to 0.01 five times, so the last share is
    /// 0.03 - 0.05 = -0.02: negative, and still summing to the parent (DEC-101). Before its last
    /// unit is sold, the parcel has handed out 0.05 of its 0.03 [DOM-061], [DOM-062], [DOM-063].
    #[test]
    fn the_last_share_may_be_negative() {
        let sixths = |share: Decimal| {
            std::iter::repeat_n(dec!(0.01), 5)
                .chain([share])
                .collect::<Vec<_>>()
        };

        let opening = buy(1, dec!(6), dec!(6.00), dec!(0.03));
        let allocations: Vec<_> = (1..=6_u32)
            .map(|n| against(100 + i64::from(n), 1 + n, dec!(1)))
            .collect();
        let opened = opening_shares(&opening, &[], &allocations, Half::Eur).expect("measurable");
        assert_eq!(
            opened
                .iter()
                .map(|(_, s)| s.buy_fee().get())
                .collect::<Vec<_>>(),
            sixths(dec!(-0.02))
        );
        let early =
            opening_shares(&opening, &[], &allocations[..5], Half::Eur).expect("measurable");
        assert_eq!(
            early
                .iter()
                .map(|(_, s)| s.buy_fee().get())
                .sum::<Decimal>(),
            dec!(0.05)
        );

        let closing = sell(20, dec!(6), dec!(6.00), dec!(0.03));
        let allocations: Vec<_> = (1..=6_u32).map(|n| of(i64::from(n), n, dec!(1))).collect();
        let closed = closing_shares(&closing, &allocations, Half::Eur).expect("covered");
        assert_eq!(
            closed
                .iter()
                .map(|(_, s)| s.sell_fee().get())
                .collect::<Vec<_>>(),
            sixths(dec!(-0.02))
        );
    }

    /// Two openings at one position are ordered by row id, as the FIFO proposal orders them, so
    /// the higher id is the closing's last and absorbs the drift (DEC-095) [DOM-063].
    #[test]
    fn a_tied_position_on_the_closing_side_is_decided_by_row_id() {
        let closing = sell(10, dec!(3), dec!(100.00), dec!(0.00));

        let closed = closing_shares(
            &closing,
            &[of(9, 2, dec!(1)), of(3, 2, dec!(1)), of(5, 1, dec!(1))],
            Half::Eur,
        );

        assert_eq!(
            closed.as_deref().map(proceeds),
            Ok(vec![
                (5, dec!(33.33), dec!(0.00)),
                (3, dec!(33.33), dec!(0.00)),
                (9, dec!(33.34), dec!(0.00)),
            ])
        );
    }

    /// A sell fee is shared among its own closing's allocations and sums to it [DOM-060]. Two
    /// closings against the same two openings, with different fees and different remainders,
    /// each spread their own: 1.00 over 2 and 1 is 0.67 and 0.33, 0.10 over 1 and 2 is 0.03 and
    /// 0.07, and a closing with no fee spreads none.
    ///
    /// The rule itself is enforced by the signature: [`closing_shares`] is given one closing and
    /// its own allocations, so it has no way to reach another's. This test names the rule and
    /// pins the per-closing sums.
    #[test]
    fn a_sell_fee_stays_with_its_own_closing() {
        let first = sell(10, dec!(3), dec!(300.00), dec!(1.00));
        let second = sell(11, dec!(3), dec!(300.00), dec!(0.10));
        let free = sell(12, dec!(3), dec!(300.00), dec!(0.00));

        let first = closing_shares(&first, &[of(1, 1, dec!(2)), of(2, 2, dec!(1))], Half::Eur);
        let second = closing_shares(&second, &[of(1, 1, dec!(1)), of(2, 2, dec!(2))], Half::Eur);
        let free = closing_shares(&free, &[of(1, 1, dec!(2)), of(2, 2, dec!(1))], Half::Eur);

        assert_eq!(
            first.as_deref().map(proceeds),
            Ok(vec![
                (1, dec!(200.00), dec!(0.67)),
                (2, dec!(100.00), dec!(0.33))
            ])
        );
        assert_eq!(
            second.as_deref().map(proceeds),
            Ok(vec![
                (1, dec!(100.00), dec!(0.03)),
                (2, dec!(200.00), dec!(0.07))
            ])
        );
        assert_eq!(
            free.as_deref().map(proceeds),
            Ok(vec![
                (1, dec!(200.00), dec!(0.00)),
                (2, dec!(100.00), dec!(0.00))
            ])
        );
    }

    /// Shares derive from the native half by the same rule as from the EUR half [DOM-084]: a USD
    /// sale of 110.00 in thirds is 36.67, 36.67, 36.66 natively and 33.33, 33.33, 33.34 in EUR.
    #[test]
    fn native_shares_derive_as_eur_shares_do() {
        let closing = Closing::Sell(Sell::new(
            on(10, 0),
            Quantity::new(dec!(3)),
            Valued::new(
                QuotedPrice::new(dec!(36.666667)),
                QuotedPrice::new(dec!(33.333333)),
            ),
            Valued::new(money(dec!(110.00)), money(dec!(100.00))),
            Valued::new(money(dec!(3.00)), money(dec!(2.73))),
            Conversion::new(
                Currency::new("USD"),
                FxRate::new(dec!(1.100000)),
                RateSource::Ecb,
                date(10),
            ),
        ));
        let allocations = [of(1, 1, dec!(1)), of(2, 2, dec!(1)), of(3, 3, dec!(1))];

        assert_eq!(
            closing_shares(&closing, &allocations, Half::Native)
                .as_deref()
                .map(proceeds),
            Ok(vec![
                (1, dec!(36.67), dec!(1.00)),
                (2, dec!(36.67), dec!(1.00)),
                (3, dec!(36.66), dec!(1.00)),
            ])
        );
        assert_eq!(
            closing_shares(&closing, &allocations, Half::Eur)
                .as_deref()
                .map(proceeds),
            Ok(vec![
                (1, dec!(33.33), dec!(0.91)),
                (2, dec!(33.33), dec!(0.91)),
                (3, dec!(33.34), dec!(0.91)),
            ])
        );
    }

    /// The opening side reads the same [`Half`]: a USD buy of 3 for 110.00 plus 3.30 fees,
    /// 100.00 plus 3.00 in EUR, sold 1 at a time, is 36.67, 36.67, 36.66 cost and 1.10 fees
    /// natively and 33.33, 33.33, 33.34 and 1.00 in EUR; the drift lands down in one half and up
    /// in the other [DOM-084], [DOM-059], [DOM-062].
    #[test]
    fn native_opening_shares_derive_as_eur_ones_do() {
        let opening = Opening::Buy(Buy::new(
            on(1, 0),
            Quantity::new(dec!(3)),
            Valued::new(
                QuotedPrice::new(dec!(36.666667)),
                QuotedPrice::new(dec!(33.333333)),
            ),
            Valued::new(money(dec!(110.00)), money(dec!(100.00))),
            Valued::new(money(dec!(3.30)), money(dec!(3.00))),
            BuyOrigin::Purchase,
            Conversion::new(
                Currency::new("USD"),
                FxRate::new(dec!(1.100000)),
                RateSource::Ecb,
                date(1),
            ),
        ));
        let allocations = [
            against(20, 5, dec!(1)),
            against(21, 6, dec!(1)),
            against(22, 7, dec!(1)),
        ];

        assert_eq!(
            opening_shares(&opening, &[], &allocations, Half::Native)
                .as_deref()
                .map(costs),
            Ok(vec![
                (20, dec!(36.67), dec!(1.10)),
                (21, dec!(36.67), dec!(1.10)),
                (22, dec!(36.66), dec!(1.10)),
            ])
        );
        assert_eq!(
            opening_shares(&opening, &[], &allocations, Half::Eur)
                .as_deref()
                .map(costs),
            Ok(vec![
                (20, dec!(33.33), dec!(1.00)),
                (21, dec!(33.33), dec!(1.00)),
                (22, dec!(33.34), dec!(1.00)),
            ])
        );
    }

    /// A `transfer_out` has no proceeds and no gain [DOM-093], FIF-080; its opening side, the
    /// basis it carries onward, is derived as for any closing.
    #[test]
    fn a_transfer_out_has_an_opening_side_and_no_closing_side() {
        let opening = buy(1, dec!(3), dec!(100.00), dec!(1.00));
        let transfer_out = Closing::TransferOut(TransferOut::new(
            on(10, 0),
            Quantity::new(dec!(3)),
            Valued::in_eur(Money::zero()),
            Conversion::native(date(10)),
        ));

        assert_eq!(
            opening_shares(&opening, &[], &[against(2, 10, dec!(3))], Half::Eur)
                .as_deref()
                .map(costs),
            Ok(vec![(2, dec!(100.00), dec!(1.00))])
        );
        assert_eq!(
            closing_shares(&transfer_out, &[of(1, 1, dec!(3))], Half::Eur),
            Err(AllocationError::NoProceeds)
        );
    }

    /// An expiration's quantity is undecided (FIF-079), so nothing is divided by a guess
    /// [DOM-092].
    #[test]
    fn an_expiration_is_refused() {
        let expiration = Closing::Expiration(Expiration::new(
            on(10, 0),
            Valued::in_eur(Money::zero()),
            Valued::in_eur(money(dec!(1.00))),
            Conversion::native(date(10)),
        ));

        assert_eq!(
            closing_shares(&expiration, &[of(1, 1, dec!(3))], Half::Eur),
            Err(AllocationError::QuantityNotStated)
        );
    }

    /// A closing of nothing, or of less than nothing, leaves nothing to divide by [DOM-114].
    #[test]
    fn a_closing_of_nothing_is_refused() {
        assert_eq!(
            closing_shares(&sell(10, dec!(0), dec!(0.00), dec!(0.00)), &[], Half::Eur),
            Err(AllocationError::NothingClosed { quantity: dec!(0) })
        );
        assert_eq!(
            closing_shares(
                &sell(10, dec!(-1), dec!(5.00), dec!(0.00)),
                &[of(1, 1, dec!(-1))],
                Half::Eur,
            ),
            Err(AllocationError::NothingClosed { quantity: dec!(-1) })
        );
    }

    /// The smallest closing there is, one quantum, is accepted, and its one allocation takes the
    /// whole gross and fee [DOM-059], [DOM-114].
    #[test]
    fn a_closing_of_one_quantum_is_shared() {
        assert_eq!(
            closing_shares(
                &sell(10, dec!(0.00000001), dec!(0.05), dec!(0.01)),
                &[of(1, 1, dec!(0.00000001))],
                Half::Eur,
            )
            .as_deref()
            .map(proceeds),
            Ok(vec![(1, dec!(0.05), dec!(0.01))])
        );
    }

    /// A share beyond what a decimal holds is refused rather than wrapped. No holding reaches
    /// one; it takes a closing stated at half a quantum, which views as one, so that an
    /// allocation of one quantum ahead of an allocation of nothing is twice the closing and gets
    /// twice a gross already near the decimal limit [DOM-059].
    #[test]
    fn a_share_beyond_a_decimal_is_refused() {
        let closing = sell(
            10,
            dec!(0.000000005),
            dec!(500000000000000000000000000.00),
            dec!(0.00),
        );

        assert_eq!(
            closing_shares(
                &closing,
                &[of(1, 1, dec!(0.00000001)), of(2, 2, dec!(0))],
                Half::Eur,
            ),
            Err(AllocationError::Unrepresentable)
        );
    }

    /// Allocations short of the closing would make a middle allocation the "last" and hand it
    /// drift that is not its own, so they are refused [DOM-063], [DOM-065].
    #[test]
    fn allocations_that_do_not_cover_the_closing_are_refused() {
        let closing = sell(10, dec!(3), dec!(100.00), dec!(0.00));

        assert_eq!(
            closing_shares(&closing, &[of(1, 1, dec!(1)), of(2, 2, dec!(1))], Half::Eur),
            Err(AllocationError::Incomplete {
                allocated: dec!(2),
                closed: dec!(3),
            })
        );
    }

    /// A closing before its opening, or an opening of nothing, gives no quantity to divide by
    /// (DEC-097) [DOM-103].
    #[test]
    fn an_unmeasurable_opening_is_refused() {
        assert_eq!(
            opening_shares(
                &buy(5, dec!(3), dec!(100.00), dec!(0.00)),
                &[],
                &[against(20, 2, dec!(1))],
                Half::Eur,
            ),
            Err(AllocationError::Unmeasurable { closing: id(20) })
        );
        assert_eq!(
            opening_shares(
                &buy(1, dec!(0), dec!(100.00), dec!(0.00)),
                &[],
                &[against(20, 2, dec!(1))],
                Half::Eur,
            ),
            Err(AllocationError::Unmeasurable { closing: id(20) })
        );
    }

    /// Allocations over the closing by one quantum are refused like those short of it, and so
    /// are none at all [DOM-065], DEC-091.
    #[test]
    fn allocations_past_the_closing_by_one_quantum_are_refused() {
        let closing = sell(10, dec!(3), dec!(100.00), dec!(0.00));

        assert_eq!(
            closing_shares(
                &closing,
                &[of(1, 1, dec!(1)), of(2, 2, dec!(2.00000001))],
                Half::Eur,
            ),
            Err(AllocationError::Incomplete {
                allocated: dec!(3.00000001),
                closed: dec!(3),
            })
        );
        assert_eq!(
            closing_shares(&closing, &[], Half::Eur),
            Err(AllocationError::Incomplete {
                allocated: dec!(0),
                closed: dec!(3),
            })
        );
    }

    /// A closing stated finer than the quantity scale, 1.000000004, is covered by allocations
    /// summing to its view, 1.00000000 [DOM-065], DEC-091. The shares divide by the stated
    /// quantity (DEC-100): 0.01 x 0.5 / 1.000000004 = 0.00499999998 rounds to 0.00, where the
    /// view would give an exact 0.005 and 0.01; the last share takes the 0.01 [DOM-059].
    #[test]
    fn a_closing_finer_than_the_quantity_scale_divides_by_what_it_states() {
        let closing = sell(10, dec!(1.000000004), dec!(0.01), dec!(0.00));

        assert_eq!(
            closing_shares(
                &closing,
                &[of(1, 1, dec!(0.5)), of(2, 2, dec!(0.5))],
                Half::Eur,
            )
            .as_deref()
            .map(proceeds),
            Ok(vec![
                (1, dec!(0.00), dec!(0.00)),
                (2, dec!(0.01), dec!(0.00))
            ])
        );
    }

    /// A closing stated at 1.000000005 views half away from zero as 1.00000001, so allocations
    /// summing to 1.00000001 cover it and allocations summing to 1 do not [DOM-065], DEC-091,
    /// [ARC-010]. 0.01 x 0.50000001 / 1.000000005 is just over 0.005, so 0.01; the last takes
    /// the 0.00 left [DOM-059].
    #[test]
    fn a_closing_rounding_up_at_the_quantity_scale_is_covered_by_its_view() {
        let closing = sell(10, dec!(1.000000005), dec!(0.01), dec!(0.00));

        assert_eq!(
            closing_shares(
                &closing,
                &[of(1, 1, dec!(0.50000001)), of(2, 2, dec!(0.5))],
                Half::Eur,
            )
            .as_deref()
            .map(proceeds),
            Ok(vec![
                (1, dec!(0.01), dec!(0.00)),
                (2, dec!(0.00), dec!(0.00))
            ])
        );
        assert_eq!(
            closing_shares(
                &closing,
                &[of(1, 1, dec!(0.5)), of(2, 2, dec!(0.5))],
                Half::Eur,
            ),
            Err(AllocationError::Incomplete {
                allocated: dec!(1.0),
                closed: dec!(1.00000001),
            })
        );
    }

    /// An opening allocated past what it holds, here by the least it can be, one quantum, is
    /// refused, naming the allocation that did it [DOM-064].
    #[test]
    fn an_opening_over_allocated_by_one_quantum_is_refused() {
        let opening = buy(1, dec!(3), dec!(100.00), dec!(0.00));

        assert_eq!(
            opening_shares(
                &opening,
                &[],
                &[against(20, 5, dec!(1)), against(21, 6, dec!(2.00000001))],
                Half::Eur,
            ),
            Err(AllocationError::OverAllocated { closing: id(21) })
        );
    }

    /// A `transfer_in` opens a parcel like a buy: its carried cost basis is the cost shared out
    /// and its own fees the buy fee. 1000.00 and 3.00 over 10 units, sold 4 and 6 [DOM-059],
    /// [DOM-083], [DOM-085].
    #[test]
    fn a_transfer_in_shares_its_cost_basis_and_fees() {
        let opening = Opening::TransferIn(TransferIn::new(
            on(1, 0),
            Quantity::new(dec!(10)),
            Valued::in_eur(money(dec!(1000.00))),
            Valued::in_eur(money(dec!(3.00))),
            date(1),
            DateProvenance::TransferDate,
            TransferInSource::Broker,
            Conversion::native(date(1)),
        ));

        assert_eq!(
            opening_shares(
                &opening,
                &[],
                &[against(20, 5, dec!(4)), against(21, 6, dec!(6))],
                Half::Eur,
            )
            .as_deref()
            .map(costs),
            Ok(vec![
                (20, dec!(400.00), dec!(1.20)),
                (21, dec!(600.00), dec!(1.80)),
            ])
        );
    }

    /// Every closing variant answers its own fees, the figure the sell fee is shared from
    /// [DOM-105].
    #[test]
    fn every_closing_reads_its_own_fees() {
        let fees = |value| Valued::in_eur(money(value));
        let expiration = Closing::Expiration(Expiration::new(
            on(10, 0),
            Valued::in_eur(Money::zero()),
            fees(dec!(1.00)),
            Conversion::native(date(10)),
        ));
        let transfer_out = Closing::TransferOut(TransferOut::new(
            on(10, 0),
            Quantity::new(dec!(3)),
            fees(dec!(2.00)),
            Conversion::native(date(10)),
        ));
        let sold = sell(10, dec!(3), dec!(100.00), dec!(3.00));

        assert_eq!(
            [expiration.fees(), transfer_out.fees(), sold.fees()],
            [fees(dec!(1.00)), fees(dec!(2.00)), fees(dec!(3.00))]
        );
    }

    /// Quantities at the full 8-decimal scale: 0.3 for 10.00 with 1.00 fees, sold 0.00000001,
    /// 0.12345678 and 0.17654321. The first share is 10.00 x 0.00000001 / 0.3, a fraction of a
    /// cent, so 0.00 and 0.00; the second 4.115226 and 0.4115226, so 4.12 and 0.41; the last
    /// takes 5.88 and 0.59 [DOM-061], [TST-016].
    #[test]
    fn fractional_quantities_share_like_whole_ones() {
        let opening = buy(1, dec!(0.3), dec!(10.00), dec!(1.00));

        let opened = opening_shares(
            &opening,
            &[],
            &[
                against(20, 5, dec!(0.00000001)),
                against(21, 6, dec!(0.12345678)),
                against(22, 7, dec!(0.17654321)),
            ],
            Half::Eur,
        );

        assert_eq!(
            opened.as_deref().map(costs),
            Ok(vec![
                (20, dec!(0.00), dec!(0.00)),
                (21, dec!(4.12), dec!(0.41)),
                (22, dec!(5.88), dec!(0.59)),
            ])
        );
    }

    /// Up to 20 allocation quantities of up to 1000 units at the 8-decimal scale.
    fn quantities() -> impl Strategy<Value = Vec<Decimal>> {
        prop::collection::vec(1..100_000_000_000_i64, 1..20)
            .prop_map(|units| units.into_iter().map(|u| Decimal::new(u, 8)).collect())
    }

    /// A figure of up to a million euros, in cents.
    fn cents() -> impl Strategy<Value = Decimal> {
        (0..100_000_000_i64).prop_map(|c| Decimal::new(c, 2))
    }

    proptest! {
        /// A closing's shares sum exactly to its proceeds and to its fees, for every division and
        /// every remainder, and each is at 2 decimals [DOM-061], [DOM-063], [TST-010].
        #[test]
        fn closing_shares_sum_to_the_parent(
            quantities in quantities(),
            gross in cents(),
            fees in cents(),
        ) {
            let total: Decimal = quantities.iter().sum();
            let closing = sell(28, total, gross, fees);
            let allocations: Vec<_> = (1..)
                .zip(&quantities)
                .map(|(n, q)| of(n, 1, *q))
                .collect();

            let shares = closing_shares(&closing, &allocations, Half::Eur).expect("covered");

            prop_assert_eq!(shares.iter().map(|(_, s)| s.proceeds().get()).sum::<Decimal>(), gross);
            prop_assert_eq!(shares.iter().map(|(_, s)| s.sell_fee().get()).sum::<Decimal>(), fees);
            prop_assert!(shares.iter().all(|(_, s)| s.proceeds().get().scale() == 2));
            prop_assert!(shares.iter().all(|(_, s)| s.sell_fee().get().scale() == 2));
        }

        /// An opening's shares sum exactly to its cost and fees once it is exhausted, and before
        /// that every share is its own rounded quotient, so nothing is distributed early; each is
        /// at 2 decimals. A split lands before the closings from `split_at` on, whose quantities
        /// are in the post-split units and divide by the post-split effective quantity
        /// [DOM-061], [DOM-062], [DOM-103], [TST-010].
        #[test]
        fn opening_shares_sum_to_the_parent_once_exhausted(
            (multiples, split_at) in prop::collection::vec(1..10_000_000_000_i64, 1..20)
                .prop_flat_map(|m| { let len = m.len(); (Just(m), 0..=len) }),
            numerator in 1..=10_u32,
            denominator in 1..=10_u32,
            gross in cents(),
            fees in cents(),
        ) {
            // Each allocation is a whole number of `denominator` quanta before the split, so it
            // is a whole number of `numerator` quanta after it and every figure stays exact at
            // the quantity scale: the parcel is exhausted exactly by the last allocation.
            let pre = |m: i64| m * i64::from(denominator);
            let post = |m: i64| m * i64::from(numerator);
            let total = Decimal::new(multiples.iter().copied().map(pre).sum(), 8);
            let opening = buy(1, total, gross, fees);
            let splits = [split(3, numerator, denominator)];
            let allocations: Vec<_> = (1..)
                .zip(&multiples)
                .enumerate()
                .map(|(index, (n, &m))| {
                    let order = u32::try_from(n).expect("small");
                    let (day, quanta) = if index < split_at { (2, pre(m)) } else { (4, post(m)) };
                    AgainstOpening::new(id(n), on(day, order).order_key(), Quantity::new(Decimal::new(quanta, 8)))
                })
                .collect();

            let shares = opening_shares(&opening, &splits, &allocations, Half::Eur).expect("measurable");
            prop_assert_eq!(shares.iter().map(|(_, s)| s.cost().get()).sum::<Decimal>(), gross);
            prop_assert_eq!(shares.iter().map(|(_, s)| s.buy_fee().get()).sum::<Decimal>(), fees);
            prop_assert!(shares.iter().all(|(_, s)| s.cost().get().scale() == 2));
            prop_assert!(shares.iter().all(|(_, s)| s.buy_fee().get().scale() == 2));

            let unfinished = &allocations[..allocations.len() - 1];
            let early = opening_shares(&opening, &splits, unfinished, Half::Eur).expect("measurable");
            let independent = |parent: Decimal| unfinished.iter().enumerate().map(move |(index, a)| {
                let effective = if index < split_at {
                    rational(total)
                } else {
                    rational(total) * BigRational::new(numerator.into(), denominator.into())
                };
                cents_half_away(rational(parent) * rational(a.quantity.get()) / effective)
            });
            prop_assert!(early.iter().map(|(_, s)| s.cost().get()).eq(independent(gross)));
            prop_assert!(early.iter().map(|(_, s)| s.buy_fee().get()).eq(independent(fees)));
        }
    }

    /// The test oracle's own exact reading of a decimal, kept apart from the code under test.
    fn rational(value: Decimal) -> BigRational {
        BigRational::new(value.mantissa().into(), BigInt::from(10).pow(value.scale()))
    }

    /// The test oracle's own rounding to cents, half away from zero, by integer arithmetic on
    /// numerator and denominator rather than `Ratio::round` as [`to_money`] uses.
    fn cents_half_away(value: BigRational) -> Decimal {
        let hundredths = value * BigRational::from_integer(100.into());
        let (numerator, denominator) = (hundredths.numer().clone(), hundredths.denom().clone());
        // `BigRational` keeps the denominator positive, and BigInt division truncates toward
        // zero, so adding half the denominator to the magnitude rounds a tie away from zero.
        let magnitude = (numerator.magnitude() * 2u32 + denominator.magnitude())
            / (denominator.magnitude() * 2u32);
        let cents = i128::try_from(BigInt::from(magnitude)).expect("in range");
        let signed = if numerator < BigInt::zero() {
            -cents
        } else {
            cents
        };
        Decimal::from_i128_with_scale(signed, 2)
    }
}
