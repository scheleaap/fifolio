//! The FIFO proposal: which openings a closing consumes, and how much of each [DOM-056], or the
//! quantity it is short by [DOM-057].
//!
//! Pure: it reads the transactions and allocations it is handed and stores nothing. Approving
//! the proposal is the user's [DOM-054] and storing it is [`crate::storage::AttributionRepository`]'s.
//!
//! # What the caller passes
//!
//! The openings, splits and allocations of the closing's own account and security. Those
//! relations are held by storage [DOM-013], not by a transaction, so the account and security
//! are applied by whoever reads them out; nothing here could check them.
//!
//! # How much of an opening is left
//!
//! An opening's remaining quantity is its effective quantity **as of the closing** less every
//! allocation already made against it, each rescaled from its own closing to this one. The two
//! sides are each viewed through [`EffectiveQuantity::at_quantity_scale`] before one is
//! subtracted from the other (DEC-091, DEC-097, DEC-099) [DOM-064], so what this engine takes
//! from a parcel is exactly what the next closing sees as gone. That is the view FIF-014 and FIF-078 compare through too, so a parcel this engine
//! sees as exhausted is one they see as exhausted. The effective quantity has no answer before
//! the opening, so only openings that precede the closing are candidates.
//!
//! # Order
//!
//! Candidates are taken in canonical order [DOM-011], [DOM-111]. Two transactions derived from the
//! same lowest record in the same leg share an [`OrderKey`]; the row id decides between them, as
//! it does in storage (DEC-095).

use std::collections::HashMap;

use rust_decimal::Decimal;
use thiserror::Error;

use crate::decimal::{Quantity, Scaled};
use crate::effective_quantity::unattributed_quantity;
use crate::ordering::OrderKey;
use crate::storage::{Allocation, TransactionId};
use crate::transaction::{Closing, Opening, Split};

/// An allocation already made, with the position of the closing it belongs to: its quantity is
/// in the units current there [DOM-103].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriorAllocation {
    closed_at: OrderKey,
    allocation: Allocation,
}

impl PriorAllocation {
    #[must_use]
    pub fn new(closed_at: OrderKey, allocation: Allocation) -> Self {
        Self {
            closed_at,
            allocation,
        }
    }
}

/// What the engine offers for a closing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proposal {
    /// The openings to consume, oldest first, the last normally only in part [DOM-056]. The
    /// quantities sum to the closed quantity exactly [DOM-065].
    Allocate(Vec<Allocation>),
    /// Too little is unattributed to cover the closing, so nothing is proposed [DOM-057].
    Shortfall {
        /// How much more would have to be open, in the closing's own units.
        missing: Quantity,
    },
}

/// Why no answer, neither proposal nor shortfall, can be given.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProposalError {
    /// An `expiration` states no quantity: its quantity is the unattributed remainder [DOM-092],
    /// which is FIF-079's and undecided.
    #[error("an expiration's quantity is the unattributed remainder, which is not yet decided")]
    QuantityNotStated,
    /// A closing of nothing, or of less than nothing, is a data problem for the caller to report
    /// [DOM-114], not one to answer with an empty proposal (DEC-098, provisional).
    #[error("the closing's quantity {quantity} is not positive")]
    NothingClosed { quantity: Decimal },
    /// The remainder of `opening` cannot be measured as of the closing: an allocation against it
    /// belongs to a closing before it, it states no quantity, or its remainder is beyond what a
    /// decimal holds.
    #[error("the remaining quantity of opening {opening} cannot be measured")]
    Unmeasurable { opening: TransactionId },
    /// More of `opening` is allocated than it holds [DOM-064]. Proposing past it would build on
    /// figures that are already wrong (DEC-098, provisional).
    #[error("opening {opening} is over-allocated: {remaining} remains")]
    OverAllocated {
        opening: TransactionId,
        remaining: Decimal,
    },
}

/// The proposal for `closing`: the oldest openings with unattributed quantity left, in canonical
/// order, until its quantity is covered, the final one split [DOM-056]; or, if what is left falls
/// short, the missing quantity [DOM-057].
///
/// `openings`, `splits` and `allocated` are those of the closing's account and security, in any
/// order. Openings at or after the closing are not candidates.
///
/// # Errors
///
/// [`ProposalError`] when the closing states no positive quantity or a candidate's remainder is
/// unmeasurable or negative.
pub fn propose<'a>(
    closing: &Closing,
    openings: impl IntoIterator<Item = (TransactionId, &'a Opening)>,
    splits: &[Split],
    allocated: &[PriorAllocation],
) -> Result<Proposal, ProposalError> {
    let stated = closed_quantity(closing)?;
    // DOM-065 sums a closing's allocations against its quantity at the quantity scale, and each
    // remainder is viewed there too (DEC-091), so a stated quantity finer than 8 decimals is
    // compared, and allocated, as it rounds.
    let closed = stated.rounded().get();
    if closed <= Decimal::ZERO {
        return Err(ProposalError::NothingClosed {
            quantity: stated.get(),
        });
    }
    let at = closing.derivation().order_key();

    let mut consumed: HashMap<TransactionId, Vec<(Quantity, OrderKey)>> = HashMap::new();
    for prior in allocated {
        consumed
            .entry(prior.allocation.opening())
            .or_default()
            .push((prior.allocation.quantity(), prior.closed_at));
    }

    let mut candidates: Vec<_> = openings
        .into_iter()
        .map(|(id, opening)| (opening.derivation().order_key(), id, opening))
        .filter(|(key, _, _)| *key < at)
        .collect();
    candidates.sort_by_key(|(key, id, _)| (*key, *id));

    let mut needed = closed;
    let mut proposal = Vec::new();
    for (_, id, opening) in candidates {
        if needed.is_zero() {
            break;
        }
        let remaining = unattributed_quantity(
            opening,
            splits,
            at,
            consumed.get(&id).into_iter().flatten().copied(),
        )
        .ok_or(ProposalError::Unmeasurable { opening: id })?
        .get();
        if remaining < Decimal::ZERO {
            return Err(ProposalError::OverAllocated {
                opening: id,
                remaining,
            });
        }
        // Zero at the quantity scale is an exhausted parcel (DEC-091), and it is skipped rather
        // than proposed as an allocation of nothing.
        if remaining.is_zero() {
            continue;
        }
        let take = remaining.min(needed);
        proposal.push(Allocation::new(id, Quantity::new(take.normalize())));
        needed -= take;
    }

    Ok(if needed.is_zero() {
        Proposal::Allocate(proposal)
    } else {
        Proposal::Shortfall {
            missing: Quantity::new(needed.normalize()),
        }
    })
}

/// The quantity `closing` states. A sell and a `transfer_out` state one; an `expiration` does not
/// (FIF-079).
fn closed_quantity(closing: &Closing) -> Result<Quantity, ProposalError> {
    match closing {
        Closing::Sell(sell) => Ok(sell.quantity()),
        Closing::TransferOut(transfer_out) => Ok(transfer_out.quantity()),
        Closing::Expiration(_) => Err(ProposalError::QuantityNotStated),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::num::NonZeroU32;

    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use vec1::vec1;

    use crate::decimal::{Money, QuotedPrice};
    use crate::entities::{Account, Order};
    use crate::identity::{IdentitySource, identify};
    use crate::manual_entry::Ratio;
    use crate::ordering::{BatchAge, RecordPosition};
    use crate::storage::RecordHandle;
    use crate::transaction::{Buy, BuyOrigin, Derivation, Expiration, Sell, TransferOut};
    use crate::valuation::{Conversion, Valued};

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 3, day).expect("a valid date")
    }

    /// A derivation on `day` from a record at `order`, so two on one day can be placed either
    /// way round.
    fn on(day: u32, order: u32) -> Derivation {
        let account = Account::new("Saxo", "69900/1000000");
        let reference = format!("row-{day}-{order}");
        let record = RecordHandle::for_test(
            identify(&account, &IdentitySource::BrokerReference(&reference)),
            RecordPosition::new(Order::new(order), BatchAge::new(1)),
        );
        Derivation::new(date(day), vec1![record])
    }

    fn buy_at(derivation: Derivation, quantity: Decimal) -> Opening {
        let day = derivation.trade_date();
        let gross = dec!(100.00) * quantity;
        Opening::Buy(Buy::new(
            derivation,
            Quantity::new(quantity),
            Valued::in_eur(QuotedPrice::new(dec!(100))),
            Valued::in_eur(Money::new(gross)),
            Valued::in_eur(Money::zero()),
            BuyOrigin::Purchase,
            Conversion::native(day),
        ))
    }

    fn buy(day: u32, quantity: Decimal) -> Opening {
        buy_at(on(day, 0), quantity)
    }

    fn sell_at(derivation: Derivation, quantity: Decimal) -> Closing {
        let day = derivation.trade_date();
        Closing::Sell(Sell::new(
            derivation,
            Quantity::new(quantity),
            Valued::in_eur(QuotedPrice::new(dec!(100))),
            Valued::in_eur(Money::new(dec!(100.00) * quantity)),
            Valued::in_eur(Money::zero()),
            Conversion::native(day),
        ))
    }

    fn sell(day: u32, quantity: Decimal) -> Closing {
        sell_at(on(day, 0), quantity)
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

    fn id(value: i64) -> TransactionId {
        TransactionId::new(value)
    }

    fn allocation(opening: i64, quantity: Decimal) -> Allocation {
        Allocation::new(id(opening), Quantity::new(quantity))
    }

    /// `quantity` of opening `opening` allocated to a closing on `day`.
    fn prior(day: u32, opening: i64, quantity: Decimal) -> PriorAllocation {
        PriorAllocation::new(on(day, 0).order_key(), allocation(opening, quantity))
    }

    fn numbered(openings: &[Opening]) -> Vec<(TransactionId, &Opening)> {
        (1..).map(id).zip(openings).collect()
    }

    /// The oldest openings are consumed first and the last one only in part [DOM-056].
    #[test]
    fn the_oldest_openings_are_consumed_and_the_last_is_split() {
        let openings = [buy(1, dec!(10)), buy(2, dec!(5)), buy(3, dec!(7))];

        let proposal = propose(&sell(10, dec!(12)), numbered(&openings), &[], &[]);

        assert_eq!(
            proposal,
            Ok(Proposal::Allocate(vec![
                allocation(1, dec!(10)),
                allocation(2, dec!(2)),
            ]))
        );
    }

    /// A closing covered exactly by whole openings consumes nothing beyond them [DOM-056].
    #[test]
    fn an_exact_cover_stops_at_the_last_whole_opening() {
        let openings = [buy(1, dec!(10)), buy(2, dec!(5)), buy(3, dec!(7))];

        let proposal = propose(&sell(10, dec!(15)), numbered(&openings), &[], &[]);

        assert_eq!(
            proposal,
            Ok(Proposal::Allocate(vec![
                allocation(1, dec!(10)),
                allocation(2, dec!(5)),
            ]))
        );
    }

    /// A closing equal to everything unattributed takes every candidate whole; one 1e-8 more is
    /// short by exactly that [DOM-056], [DOM-057].
    #[test]
    fn the_boundary_between_a_proposal_and_a_shortfall() {
        let openings = [buy(1, dec!(10)), buy(2, dec!(5))];

        assert_eq!(
            propose(&sell(10, dec!(15)), numbered(&openings), &[], &[]),
            Ok(Proposal::Allocate(vec![
                allocation(1, dec!(10)),
                allocation(2, dec!(5)),
            ]))
        );
        assert_eq!(
            propose(&sell(10, dec!(15.00000001)), numbered(&openings), &[], &[]),
            Ok(Proposal::Shortfall {
                missing: Quantity::new(dec!(0.00000001))
            })
        );
    }

    /// A closing stated finer than the quantity scale is compared at that scale, as DOM-065 sums
    /// it: 1.000000004 is 1 and covered by an opening of 1; 1.000000005 is 1.00000001, half away
    /// from zero, and short by 0.00000001. No allocation is finer than 8 decimals (DEC-091)
    /// [DOM-057], [DOM-065].
    #[test]
    fn a_closing_finer_than_the_quantity_scale_is_compared_at_it() {
        let openings = [buy(1, dec!(1))];

        assert_eq!(
            propose(&sell(10, dec!(1.000000004)), numbered(&openings), &[], &[]),
            Ok(Proposal::Allocate(vec![allocation(1, dec!(1))]))
        );
        assert_eq!(
            propose(&sell(10, dec!(1.000000005)), numbered(&openings), &[], &[]),
            Ok(Proposal::Shortfall {
                missing: Quantity::new(dec!(0.00000001))
            })
        );
    }

    /// A closing that rounds to nothing at the quantity scale closes nothing (DEC-091, DEC-098).
    #[test]
    fn a_closing_below_the_quantity_scale_is_refused() {
        let openings = [buy(1, dec!(1))];

        assert_eq!(
            propose(&sell(10, dec!(0.000000004)), numbered(&openings), &[], &[]),
            Err(ProposalError::NothingClosed {
                quantity: dec!(0.000000004)
            })
        );
    }

    /// Openings are taken in canonical order, not in the order passed: by date, then by `order`
    /// within one date [DOM-011].
    #[test]
    fn candidates_are_taken_in_canonical_order() {
        let openings = [
            buy(3, dec!(4)),
            buy_at(on(1, 7), dec!(4)),
            buy_at(on(1, 2), dec!(4)),
        ];

        let proposal = propose(&sell(10, dec!(6)), numbered(&openings), &[], &[]);

        assert_eq!(
            proposal,
            Ok(Proposal::Allocate(vec![
                allocation(3, dec!(4)),
                allocation(2, dec!(2)),
            ]))
        );
    }

    /// Two openings at one position are told apart by row id, as storage tells them (DEC-095).
    #[test]
    fn a_tied_position_is_decided_by_row_id() {
        let first = buy(1, dec!(4));
        let second = buy(1, dec!(4));
        let openings = [(id(9), &first), (id(3), &second)];

        let proposal = propose(&sell(10, dec!(5)), openings, &[], &[]);

        assert_eq!(
            proposal,
            Ok(Proposal::Allocate(vec![
                allocation(3, dec!(4)),
                allocation(9, dec!(1)),
            ]))
        );
    }

    /// What earlier closings consumed is not offered again; an exhausted parcel is skipped and a
    /// partly consumed one offers its remainder [DOM-056], [DOM-064].
    #[test]
    fn earlier_allocations_are_not_offered_again() {
        let openings = [buy(1, dec!(10)), buy(2, dec!(5)), buy(3, dec!(7))];
        let allocated = [
            prior(4, 1, dec!(6)),
            prior(5, 1, dec!(4)),
            prior(5, 2, dec!(3)),
        ];

        let proposal = propose(&sell(10, dec!(4)), numbered(&openings), &[], &allocated);

        assert_eq!(
            proposal,
            Ok(Proposal::Allocate(vec![
                allocation(2, dec!(2)),
                allocation(3, dec!(2)),
            ]))
        );
    }

    /// Too little open: no proposal, and the missing quantity named [DOM-057].
    #[test]
    fn a_shortfall_names_the_missing_quantity() {
        let openings = [buy(1, dec!(10)), buy(2, dec!(5))];
        let allocated = [prior(4, 1, dec!(3))];

        let proposal = propose(&sell(10, dec!(14.5)), numbered(&openings), &[], &allocated);

        assert_eq!(
            proposal,
            Ok(Proposal::Shortfall {
                missing: Quantity::new(dec!(2.5))
            })
        );
    }

    /// With nothing open at all, the whole closed quantity is missing [DOM-057].
    #[test]
    fn with_nothing_open_the_whole_quantity_is_missing() {
        let proposal = propose(&sell(10, dec!(3)), [], &[], &[]);

        assert_eq!(
            proposal,
            Ok(Proposal::Shortfall {
                missing: Quantity::new(dec!(3))
            })
        );
    }

    /// An opening at or after the closing is no candidate, so a sale before its buy is short
    /// rather than covered by it (DEC-097) [DOM-056], [DOM-057].
    #[test]
    fn only_openings_before_the_closing_are_candidates() {
        let openings = [
            buy(1, dec!(2)),
            buy(10, dec!(10)),
            buy_at(on(5, 1), dec!(10)),
            buy_at(on(5, 0), dec!(10)),
        ];

        let proposal = propose(&sell(5, dec!(3)), numbered(&openings), &[], &[]);

        assert_eq!(
            proposal,
            Ok(Proposal::Shortfall {
                missing: Quantity::new(dec!(1))
            })
        );
    }

    /// On the closing's own date, an opening from an earlier record is a candidate and one from a
    /// later record is not [DOM-011], [DOM-056].
    #[test]
    fn on_the_closing_date_the_record_order_decides_candidacy() {
        let openings = [buy_at(on(5, 0), dec!(2)), buy_at(on(5, 2), dec!(10))];

        let proposal = propose(&sell_at(on(5, 1), dec!(3)), numbered(&openings), &[], &[]);

        assert_eq!(
            proposal,
            Ok(Proposal::Shortfall {
                missing: Quantity::new(dec!(1))
            })
        );
    }

    /// Buy 10, split 2:1, sell 15: as of the sale the parcel is 20, so it covers the sale alone
    /// [DOM-089], [DOM-103].
    #[test]
    fn remaining_quantity_is_measured_as_of_the_closing() {
        let openings = [buy(1, dec!(10)), buy(2, dec!(10))];
        let splits = [split(5, 2, 1)];

        let proposal = propose(&sell(10, dec!(15)), numbered(&openings), &splits, &[]);

        assert_eq!(
            proposal,
            Ok(Proposal::Allocate(vec![allocation(1, dec!(15))]))
        );
    }

    /// Buy 10, split 2:1, sell 25: the parcel is 20 as of the sale, so 5 are missing in the
    /// sale's own units, not the 2.5 pre-split units [DOM-057], [DOM-103].
    #[test]
    fn a_shortfall_is_named_in_the_closing_units() {
        let openings = [buy(1, dec!(10))];
        let splits = [split(5, 2, 1)];

        let proposal = propose(&sell(10, dec!(25)), numbered(&openings), &splits, &[]);

        assert_eq!(
            proposal,
            Ok(Proposal::Shortfall {
                missing: Quantity::new(dec!(5))
            })
        );
    }

    /// Buy 10, sell 4, split 2:1, sell 13: the 4 sold are 8 post-split units, so 12 remain and
    /// the second sale takes 1 of the next parcel's post-split 20 [DOM-064], [DOM-103].
    #[test]
    fn an_earlier_allocation_is_rescaled_across_a_split() {
        let openings = [buy(1, dec!(10)), buy(2, dec!(10))];
        let splits = [split(5, 2, 1)];
        let allocated = [prior(3, 1, dec!(4))];

        let proposal = propose(
            &sell(10, dec!(13)),
            numbered(&openings),
            &splits,
            &allocated,
        );

        assert_eq!(
            proposal,
            Ok(Proposal::Allocate(vec![
                allocation(1, dec!(12)),
                allocation(2, dec!(1)),
            ]))
        );
    }

    /// Through a one-for-three, 10 is 10/3; once 3.33333333 is sold the residue rounds to zero,
    /// so the parcel is exhausted and the next sale goes to the next one (DEC-091) [DOM-064].
    #[test]
    fn a_residue_below_the_quantity_scale_is_exhausted() {
        let openings = [buy(1, dec!(10)), buy(4, dec!(1))];
        let splits = [split(2, 1, 3)];
        let allocated = [prior(3, 1, dec!(3.33333333))];

        let proposal = propose(
            &sell(10, dec!(0.5)),
            numbered(&openings),
            &splits,
            &allocated,
        );

        assert_eq!(
            proposal,
            Ok(Proposal::Allocate(vec![allocation(2, dec!(0.5))]))
        );
    }

    /// 12.34567891 through a one-for-two is 6.172839455, a halfway tie viewed as 6.17283946. The
    /// engine offers that view in full at 8 decimals, and once it is allocated the parcel is
    /// exhausted, not over-allocated, so the next sale goes to the next parcel (DEC-091, DEC-099)
    /// [DOM-056], [DOM-064], [TST-016].
    #[test]
    fn a_halfway_remainder_taken_in_full_exhausts_the_parcel() {
        let openings = [buy(1, dec!(12.34567891)), buy(4, dec!(1))];
        let splits = [split(2, 1, 2)];

        assert_eq!(
            propose(
                &sell(3, dec!(6.17283946)),
                numbered(&openings),
                &splits,
                &[]
            ),
            Ok(Proposal::Allocate(vec![allocation(1, dec!(6.17283946))]))
        );

        let allocated = [prior(3, 1, dec!(6.17283946))];
        assert_eq!(
            propose(
                &sell(10, dec!(0.5)),
                numbered(&openings),
                &splits,
                &allocated
            ),
            Ok(Proposal::Allocate(vec![allocation(2, dec!(0.5))]))
        );
    }

    /// A `transfer_out` closes a stated quantity like a sell [DOM-056].
    #[test]
    fn a_transfer_out_is_proposed_like_a_sell() {
        let openings = [buy(1, dec!(10))];
        let transfer_out = Closing::TransferOut(TransferOut::new(
            on(10, 0),
            Quantity::new(dec!(3)),
            Valued::in_eur(Money::zero()),
            Conversion::native(date(10)),
        ));

        let proposal = propose(&transfer_out, numbered(&openings), &[], &[]);

        assert_eq!(
            proposal,
            Ok(Proposal::Allocate(vec![allocation(1, dec!(3))]))
        );
    }

    /// An expiration's quantity is undecided (FIF-079), so no figure is guessed [DOM-092].
    #[test]
    fn an_expiration_is_refused() {
        let openings = [buy(1, dec!(10))];
        let expiration = Closing::Expiration(Expiration::new(
            on(10, 0),
            Valued::in_eur(Money::zero()),
            Valued::in_eur(Money::zero()),
            Conversion::native(date(10)),
        ));

        assert_eq!(
            propose(&expiration, numbered(&openings), &[], &[]),
            Err(ProposalError::QuantityNotStated)
        );
    }

    /// A closing of nothing is refused rather than answered with an empty proposal.
    #[test]
    fn a_closing_of_nothing_is_refused() {
        let openings = [buy(1, dec!(10))];

        assert_eq!(
            propose(&sell(10, dec!(0)), numbered(&openings), &[], &[]),
            Err(ProposalError::NothingClosed { quantity: dec!(0) })
        );
    }

    /// A closing of less than nothing is refused too, not answered with a negative shortfall
    /// (DEC-098) [DOM-114].
    #[test]
    fn a_negative_closing_is_refused() {
        let openings = [buy(1, dec!(10))];

        assert_eq!(
            propose(&sell(10, dec!(-1)), numbered(&openings), &[], &[]),
            Err(ProposalError::NothingClosed { quantity: dec!(-1) })
        );
    }

    /// A parcel allocated past what it holds is refused, not treated as exhausted [DOM-064].
    #[test]
    fn an_over_allocated_opening_is_refused() {
        let openings = [buy(1, dec!(10)), buy(2, dec!(10))];
        let allocated = [prior(3, 1, dec!(11))];

        assert_eq!(
            propose(&sell(10, dec!(1)), numbered(&openings), &[], &allocated),
            Err(ProposalError::OverAllocated {
                opening: id(1),
                remaining: dec!(-1),
            })
        );
    }

    /// Allocated past the parcel by the smallest step the quantity scale has, it is refused, not
    /// treated as exhausted (DEC-098) [DOM-064].
    #[test]
    fn an_opening_over_allocated_by_one_step_is_refused() {
        let openings = [buy(1, dec!(10)), buy(2, dec!(10))];
        let allocated = [prior(3, 1, dec!(10.00000001))];

        assert_eq!(
            propose(&sell(10, dec!(1)), numbered(&openings), &[], &allocated),
            Err(ProposalError::OverAllocated {
                opening: id(1),
                remaining: dec!(-0.00000001),
            })
        );
    }

    /// An over-allocated opening after the ones that cover the closing is never met, so the
    /// proposal stands: it consumes only earlier parcels, whose figures the broken one does not
    /// enter (DEC-098) [DOM-056], [DOM-064].
    #[test]
    fn an_over_allocated_opening_past_the_cover_is_not_met() {
        let openings = [buy(1, dec!(10)), buy(2, dec!(10))];
        let allocated = [prior(3, 2, dec!(11))];

        assert_eq!(
            propose(&sell(10, dec!(4)), numbered(&openings), &[], &allocated),
            Ok(Proposal::Allocate(vec![allocation(1, dec!(4))]))
        );
    }

    /// An opening of nothing is exhausted and skipped; with an allocation against it there is no
    /// quantity to rescale that allocation by, so its remainder is unanswerable [DOM-064].
    #[test]
    fn an_opening_of_nothing_is_skipped_and_unmeasurable_once_allocated() {
        let openings = [buy(1, dec!(0)), buy(2, dec!(10))];

        assert_eq!(
            propose(&sell(10, dec!(4)), numbered(&openings), &[], &[]),
            Ok(Proposal::Allocate(vec![allocation(2, dec!(4))]))
        );
        assert_eq!(
            propose(
                &sell(10, dec!(4)),
                numbered(&openings),
                &[],
                &[prior(3, 1, dec!(1))]
            ),
            Err(ProposalError::Unmeasurable { opening: id(1) })
        );
    }

    /// An allocation to a closing before its opening leaves the remainder unanswerable
    /// (DEC-097).
    #[test]
    fn an_allocation_before_its_opening_is_unmeasurable() {
        let openings = [buy(5, dec!(10))];
        let allocated = [prior(2, 1, dec!(1))];

        assert_eq!(
            propose(&sell(10, dec!(1)), numbered(&openings), &[], &allocated),
            Err(ProposalError::Unmeasurable { opening: id(1) })
        );
    }
}
