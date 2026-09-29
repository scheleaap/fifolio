//! What approving a `transfer_out` emits: one `transfer_in` per parcel it consumed [DOM-090].
//!
//! Pure: it is handed each consumed parcel with its allocated cost and buy fee already derived,
//! and builds the records; storing them, in the SQLite transaction that stores the attribution,
//! is [`crate::attribution`]'s.
//!
//! # One record per parcel, each with its own figures
//!
//! Each record carries its parcel's own allocated cost and buy fee, derived by the allocation
//! formula [DOM-059], never a share of a pooled total [DOM-106]. Pooling would give two equal
//! parcels bought at 100 and 200 the same unit cost of 150: the total right and each figure
//! wrong, surfacing only when they are sold in different years (DEC-046). The buy fee travels as
//! the record's fees, apart from its cost (DEC-081), so it is still deducted when the parcel is
//! finally sold.
//!
//! Each record keeps the parcel's acquisition date with provenance `inherited`, cites the
//! `transfer_out`'s records (DEC-079), and takes the parcel's place in the canonical order as its
//! own, so the parcels sort in the target security as they did before (DEC-105, provisional). Its
//! EUR half is the parcel's, so it keeps the parcel's conversion: that is the rate relating the
//! two halves it carries [DOM-028].
//!
//! # Quantities
//!
//! Each emitted quantity is the consumed quantity times the transfer's ratio, at the quantity
//! scale; the last record, in the canonical order of the parcels (DEC-095), takes whatever the
//! others leave of the transferred quantity times the ratio, rounded **once**, half away from
//! zero, so the records sum to exactly that [DOM-115], DEC-083. A record that would come out at
//! zero or less is refused rather than stored (DEC-106, provisional).
//!
//! # A transfer's own fee
//!
//! A transfer carrying a fee of its own is refused, naming its rows, and the fee is never divided
//! [DOM-107], DEC-080: whether it is basis or fees is decided when one first appears.

use rust_decimal::Decimal;
use thiserror::Error;

use crate::allocation::covered;
use crate::decimal::{Money, Quantity, Scaled};
use crate::effective_quantity::{as_rational, at_quantity_scale, exact};
use crate::entities::RecordIdentity;
use crate::manual_entry::Ratio;
use crate::storage::TransactionId;
use crate::transaction::{DateProvenance, Opening, TransferIn, TransferInSource, TransferOut};
use crate::valuation::Valued;

/// One parcel a `transfer_out` consumed: the opening, how much of it, in the units current at
/// the transfer [DOM-103], and the cost and buy fee allocated to that quantity [DOM-059].
#[derive(Debug, Clone, Copy)]
pub struct Parcel<'a> {
    id: TransactionId,
    opening: &'a Opening,
    consumed: Quantity,
    cost: Valued<Money>,
    buy_fee: Valued<Money>,
}

impl<'a> Parcel<'a> {
    #[must_use]
    pub fn new(
        id: TransactionId,
        opening: &'a Opening,
        consumed: Quantity,
        cost: Valued<Money>,
        buy_fee: Valued<Money>,
    ) -> Self {
        Self {
            id,
            opening,
            consumed,
            cost,
            buy_fee,
        }
    }
}

/// Why a `transfer_out` emits nothing.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EmissionError {
    /// The transfer carries a fee of its own, which is refused rather than divided [DOM-107].
    #[error("it carries a fee of its own, which is not divided; rows: {}", rows(.records))]
    CarriesFee { records: Vec<RecordIdentity> },
    /// The parcels do not sum to the transferred quantity, so the last would absorb a remainder
    /// that is not its own [DOM-065], [DOM-115].
    #[error("its parcels sum to {consumed}, not to the transferred {transferred}")]
    Uncovered {
        consumed: Decimal,
        transferred: Decimal,
    },
    /// The transfer consumed no parcel, so there is nothing to carry. [`covered`] catches this for
    /// every transfer but one of nothing; an attribution links a closing to one or more openings
    /// [DOM-018].
    #[error("it consumed no parcel")]
    NoParcels,
    /// The record carrying `opening` would open a parcel of zero or fewer units (DEC-106).
    #[error("the record carrying opening {opening} would hold {quantity} units")]
    NotPositive {
        opening: TransactionId,
        quantity: Decimal,
    },
    /// A quantity lies beyond what a decimal holds, which no holding reaches.
    #[error("an emitted quantity is beyond what a decimal holds")]
    Unrepresentable,
}

fn rows(records: &[RecordIdentity]) -> String {
    records
        .iter()
        .map(RecordIdentity::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Refuses a transfer carrying a fee of its own, in either half, naming its rows [DOM-107].
///
/// # Errors
///
/// [`EmissionError::CarriesFee`].
pub fn refuse_own_fee(transfer_out: &TransferOut) -> Result<(), EmissionError> {
    let fees = transfer_out.fees();
    if fees.native().is_zero() && fees.eur().is_zero() {
        Ok(())
    } else {
        Err(EmissionError::CarriesFee {
            records: transfer_out.derivation().cites().to_vec(),
        })
    }
}

/// The `transfer_in` records approving `transfer_out` emits, one per parcel, each keyed by the
/// opening it carries, in the canonical order of those openings [DOM-090], [DOM-106], [DOM-115].
///
/// `parcels` are every parcel the transfer consumed, in any order; their quantities must sum to
/// the transferred quantity [DOM-065]. The records open parcels of the transfer's target, which
/// the caller places them in.
///
/// # Errors
///
/// [`EmissionError`] naming the rule broken; nothing is emitted in part.
pub fn emit(
    transfer_out: &TransferOut,
    parcels: &[Parcel<'_>],
) -> Result<Vec<(TransactionId, TransferIn)>, EmissionError> {
    refuse_own_fee(transfer_out)?;
    covered(
        transfer_out.quantity(),
        parcels.iter().map(|parcel| parcel.consumed),
    )
    .map_err(|uncovered| EmissionError::Uncovered {
        consumed: uncovered.allocated,
        transferred: uncovered.closed,
    })?;
    if parcels.is_empty() {
        return Err(EmissionError::NoParcels);
    }

    let mut sorted = parcels.to_vec();
    // The canonical order, row id breaking a tie as storage and the FIFO proposal do (DEC-095),
    // which is also the order that decides which record is last [DOM-115].
    sorted.sort_by_key(|parcel| (parcel.opening.derivation().order_key(), parcel.id));

    let quantities = emitted_quantities(
        transfer_out.quantity(),
        transfer_out.ratio(),
        sorted.iter().map(|parcel| parcel.consumed),
    )
    .ok_or(EmissionError::Unrepresentable)?;

    sorted
        .iter()
        .zip(quantities)
        .map(|(parcel, quantity)| {
            if quantity.get() <= Decimal::ZERO {
                return Err(EmissionError::NotPositive {
                    opening: parcel.id,
                    quantity: quantity.get(),
                });
            }
            let opening = parcel.opening;
            Ok((
                parcel.id,
                TransferIn::new(
                    transfer_out
                        .derivation()
                        .carried(opening.derivation().order_key()),
                    quantity,
                    parcel.cost,
                    parcel.buy_fee,
                    acquisition_date(opening),
                    DateProvenance::Inherited,
                    TransferInSource::CorporateAction,
                    opening.conversion().clone(),
                ),
            ))
        })
        .collect()
}

/// Each of `consumed` times `ratio` at the quantity scale, half away from zero, except the last,
/// which is `transferred` times `ratio`, rounded once, less the others [DOM-115], DEC-083.
///
/// `None` for no parcels, or a figure beyond what a decimal holds.
fn emitted_quantities(
    transferred: Quantity,
    ratio: Ratio,
    consumed: impl ExactSizeIterator<Item = Quantity>,
) -> Option<Vec<Quantity>> {
    let ratio = as_rational(ratio);
    let scaled = |quantity: Quantity| at_quantity_scale(&(exact(quantity.get()) * &ratio));
    let total = scaled(transferred)?.get();
    let earlier = consumed.len().checked_sub(1)?;
    let mut quantities = consumed
        .take(earlier)
        .map(scaled)
        .collect::<Option<Vec<_>>>()?;
    let distributed = quantities.iter().try_fold(Decimal::ZERO, |sum, quantity| {
        sum.checked_add(quantity.get())
    })?;
    quantities.push(Quantity::new(total.checked_sub(distributed)?.normalize()));
    Some(quantities)
}

/// When the parcel was acquired, which a holding period counts from: a buy's trade date, or the
/// date a `transfer_in` already carries [DOM-083].
fn acquisition_date(opening: &Opening) -> chrono::NaiveDate {
    match opening {
        Opening::Buy(buy) => buy.trade_date(),
        Opening::TransferIn(transfer_in) => transfer_in.acquisition_date(),
    }
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
    use crate::entities::{Account, Isin, Order};
    use crate::identity::{IdentitySource, identify};
    use crate::ordering::{BatchAge, RecordPosition};
    use crate::storage::RecordHandle;
    use crate::transaction::{Buy, BuyOrigin, Derivation};
    use crate::valuation::{Conversion, Currency, RateSource};

    fn date(year: i32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, 3, day).expect("a valid date")
    }

    fn on(year: i32, day: u32, order: u32, reference: &str) -> Derivation {
        let account = Account::new("Saxo", "69900/1000000");
        let record = RecordHandle::for_test(
            identify(&account, &IdentitySource::BrokerReference(reference)),
            RecordPosition::new(Order::new(order), BatchAge::new(1)),
        );
        Derivation::new(date(year, day), vec1![record])
    }

    fn ratio(numerator: u32, denominator: u32) -> Ratio {
        Ratio::new(
            NonZeroU32::new(numerator).expect("non-zero"),
            NonZeroU32::new(denominator).expect("non-zero"),
        )
    }

    fn usd(day: u32) -> Conversion {
        Conversion::new(
            Currency::new("USD"),
            FxRate::new(dec!(1.100000)),
            RateSource::Ecb,
            date(2020, day),
        )
    }

    fn buy(day: u32, order: u32) -> Opening {
        Opening::Buy(Buy::new(
            on(2020, day, order, &format!("buy-{day}-{order}")),
            Quantity::new(dec!(10)),
            Valued::in_eur(QuotedPrice::new(dec!(1))),
            Valued::in_eur(Money::new(dec!(10.00))),
            Valued::in_eur(Money::new(dec!(1.00))),
            BuyOrigin::Purchase,
            usd(day),
        ))
    }

    fn transfer_out(quantity: Decimal, ratio: Ratio, fees: Valued<Money>) -> TransferOut {
        TransferOut::new(
            on(2024, 20, 3, "exchange"),
            Quantity::new(quantity),
            fees,
            ratio,
            Conversion::native(date(2024, 20)),
            Isin::new("IE000Y77LGG9"),
        )
    }

    fn free(quantity: Decimal, ratio: Ratio) -> TransferOut {
        transfer_out(quantity, ratio, Valued::in_eur(Money::zero()))
    }

    fn pair(native: Decimal, eur: Decimal) -> Valued<Money> {
        Valued::new(Money::new(native), Money::new(eur))
    }

    fn parcel(id: i64, opening: &Opening, consumed: Decimal) -> Parcel<'_> {
        Parcel::new(
            TransactionId::new(id),
            opening,
            Quantity::new(consumed),
            pair(dec!(10.00), dec!(9.09)),
            pair(dec!(1.00), dec!(0.91)),
        )
    }

    fn quantities(emitted: &[(TransactionId, TransferIn)]) -> Vec<Decimal> {
        emitted
            .iter()
            .map(|(_, transfer_in)| transfer_in.quantity().get())
            .collect()
    }

    /// Each record carries its own parcel's cost and buy fee, apart, the parcel's acquisition
    /// date as `inherited`, every one of the transfer's records and the parcel's place and
    /// conversion
    /// [DOM-090], [DOM-106], DEC-079, DEC-081.
    #[test]
    fn each_record_carries_its_own_parcel() {
        let cheap = buy(1, 5);
        let dear = buy(2, 1);
        // Two cited records, so a record citing only the first would be caught.
        let cited = |order, reference| {
            RecordHandle::for_test(
                identify(
                    &Account::new("Saxo", "69900/1000000"),
                    &IdentitySource::BrokerReference(reference),
                ),
                RecordPosition::new(Order::new(order), BatchAge::new(1)),
            )
        };
        let transfer_out = TransferOut::new(
            Derivation::new(
                date(2024, 20),
                vec1![cited(3, "exchange-out"), cited(4, "exchange-in")],
            ),
            Quantity::new(dec!(20)),
            Valued::in_eur(Money::zero()),
            ratio(1, 1),
            Conversion::native(date(2024, 20)),
            Isin::new("IE000Y77LGG9"),
        );
        let parcels = [
            Parcel::new(
                TransactionId::new(1),
                &cheap,
                Quantity::new(dec!(10)),
                pair(dec!(100.00), dec!(90.91)),
                pair(dec!(1.00), dec!(0.91)),
            ),
            Parcel::new(
                TransactionId::new(2),
                &dear,
                Quantity::new(dec!(10)),
                pair(dec!(200.00), dec!(181.82)),
                pair(dec!(3.00), dec!(2.73)),
            ),
        ];

        let emitted = emit(&transfer_out, &parcels).expect("emitted");

        let expected = [
            (1, &cheap, dec!(100.00), dec!(90.91), dec!(1.00), dec!(0.91)),
            (2, &dear, dec!(200.00), dec!(181.82), dec!(3.00), dec!(2.73)),
        ];
        assert_eq!(emitted.len(), expected.len(), "one record per parcel");
        for ((id, transfer_in), (opening_id, opening, cost, cost_eur, fee, fee_eur)) in
            emitted.iter().zip(expected)
        {
            assert_eq!(*id, TransactionId::new(opening_id));
            assert_eq!(transfer_in.quantity(), Quantity::new(dec!(10)));
            assert_eq!(transfer_in.cost_basis(), pair(cost, cost_eur));
            assert_eq!(transfer_in.fees(), pair(fee, fee_eur));
            assert_eq!(transfer_in.acquisition_date(), opening.trade_date());
            assert_eq!(transfer_in.date_provenance(), DateProvenance::Inherited);
            assert_eq!(transfer_in.source(), TransferInSource::CorporateAction);
            assert_eq!(transfer_in.derivation().cites().len(), 2);
            assert_eq!(
                transfer_in.derivation().cites(),
                transfer_out.derivation().cites()
            );
            assert_eq!(
                transfer_in.derivation().order_key(),
                opening.derivation().order_key()
            );
            assert_eq!(transfer_in.conversion(), opening.conversion());
        }
    }

    /// A parcel that was itself carried keeps the acquisition date it carried, not its own trade
    /// date [DOM-090].
    #[test]
    fn a_carried_parcel_keeps_its_carried_acquisition_date() {
        let carried = Opening::TransferIn(TransferIn::new(
            on(2022, 4, 0, "earlier-exchange"),
            Quantity::new(dec!(10)),
            Valued::in_eur(Money::new(dec!(10.00))),
            Valued::in_eur(Money::zero()),
            date(2015, 9),
            DateProvenance::Inherited,
            TransferInSource::CorporateAction,
            usd(4),
        ));

        let emitted = emit(
            &free(dec!(10), ratio(1, 1)),
            &[parcel(1, &carried, dec!(10))],
        )
        .expect("emitted");

        assert_eq!(emitted[0].1.acquisition_date(), date(2015, 9));
    }

    /// Each quantity is the consumed quantity times the ratio at the quantity scale, and the last
    /// takes what the others leave of the product rounded once [DOM-115], DEC-083.
    #[test]
    fn the_last_record_absorbs_the_rounding_remainder() {
        let openings = [buy(1, 0), buy(2, 0), buy(3, 0)];
        let parcels: Vec<_> = (1..)
            .zip(&openings)
            .map(|(id, opening)| parcel(id, opening, dec!(1)))
            .collect();

        let emitted = emit(&free(dec!(3), ratio(1, 3)), &parcels).expect("emitted");

        assert_eq!(
            quantities(&emitted),
            [dec!(0.33333333), dec!(0.33333333), dec!(0.33333334)]
        );
        assert_eq!(quantities(&emitted).iter().sum::<Decimal>(), dec!(1));
    }

    /// The product is rounded once, half away from zero: 0.00000001 times one-half is exactly
    /// 0.000000005, which rounds up [DOM-115], ARC-010.
    #[test]
    fn a_halfway_product_rounds_away_from_zero() {
        let opening = buy(1, 0);

        let emitted = emit(
            &free(dec!(0.00000001), ratio(1, 2)),
            &[parcel(1, &opening, dec!(0.00000001))],
        )
        .expect("emitted");

        assert_eq!(quantities(&emitted), [dec!(0.00000001)]);
    }

    /// "Last" is the last parcel in canonical order, whatever order the parcels arrive in; the
    /// row id breaks a tie on the order key (DEC-095). The consumed quantities differ so that
    /// each quantity lands on its own parcel: computed in the order given and matched to the
    /// sorted parcels afterwards, the 4 units' 1.33333333 would land on opening 4 instead.
    #[test]
    fn the_last_record_is_the_last_parcel_in_canonical_order() {
        let later = buy(2, 0);
        let earlier = buy(1, 9);
        let tied = buy(1, 9);
        let parcels = [
            parcel(3, &later, dec!(4)),
            parcel(8, &tied, dec!(1)),
            parcel(4, &earlier, dec!(1)),
        ];

        let emitted = emit(&free(dec!(6), ratio(1, 3)), &parcels).expect("emitted");

        let by_id: Vec<_> = emitted
            .iter()
            .map(|(id, transfer_in)| (id.get(), transfer_in.quantity().get()))
            .collect();
        // 6 at one-for-three is exactly 2; the last, opening 3, takes what 0.33333333 twice
        // leaves, one quantum more than its own rounded 1.33333333.
        assert_eq!(
            by_id,
            [
                (4, dec!(0.33333333)),
                (8, dec!(0.33333333)),
                (3, dec!(1.33333334))
            ]
        );
    }

    /// A fee of the transfer's own, in either half, is refused naming its rows, and nothing is
    /// emitted [DOM-107], DEC-080.
    #[test]
    fn a_transfer_with_a_fee_of_its_own_is_refused_naming_its_rows() {
        let opening = buy(1, 0);
        for fees in [pair(dec!(1.00), dec!(0.00)), pair(dec!(0.00), dec!(0.01))] {
            let transfer_out = transfer_out(dec!(10), ratio(1, 1), fees);

            assert_eq!(
                emit(&transfer_out, &[parcel(1, &opening, dec!(10))]),
                Err(EmissionError::CarriesFee {
                    records: transfer_out.derivation().cites().to_vec()
                })
            );
        }
    }

    /// No parcel at all is refused as such, even for a transfer of nothing, which the sum rule
    /// lets through, rather than as a quantity beyond what a decimal holds [DOM-018], [DOM-090].
    #[test]
    fn a_transfer_of_no_parcels_is_refused() {
        assert_eq!(
            emit(&free(dec!(0), ratio(1, 1)), &[]),
            Err(EmissionError::NoParcels)
        );
        assert_eq!(
            emit(&free(dec!(10), ratio(1, 1)), &[]),
            Err(EmissionError::Uncovered {
                consumed: dec!(0),
                transferred: dec!(10),
            })
        );
    }

    /// Parcels not summing to the transferred quantity are refused, so no record absorbs a
    /// remainder that is not its own [DOM-065], [DOM-115].
    #[test]
    fn parcels_not_covering_the_transfer_are_refused() {
        let opening = buy(1, 0);

        assert_eq!(
            emit(
                &free(dec!(10), ratio(1, 1)),
                &[parcel(1, &opening, dec!(9.99999999))]
            ),
            Err(EmissionError::Uncovered {
                consumed: dec!(9.99999999),
                transferred: dec!(10),
            })
        );
    }

    /// When the earlier records round up past the rounded total, the last would open a parcel of
    /// less than nothing, and is refused naming its opening (DEC-106, provisional).
    #[test]
    fn a_record_of_no_units_is_refused() {
        let openings: Vec<_> = (1..=5).map(|day| buy(day, 0)).collect();
        let parcels: Vec<_> = (1..)
            .zip(&openings)
            .map(|(id, opening)| parcel(id, opening, dec!(0.00000001)))
            .collect();

        // Each 0.000000005 rounds to 0.00000001; the total 0.000000025 rounds to 0.00000003.
        assert_eq!(
            emit(&free(dec!(0.00000005), ratio(1, 2)), &parcels),
            Err(EmissionError::NotPositive {
                opening: TransactionId::new(5),
                quantity: dec!(-0.00000001),
            })
        );
    }

    /// A record of exactly zero units is refused as well, whether an earlier parcel too small
    /// to survive the ratio or a last record the others leave nothing (DEC-106, provisional).
    #[test]
    fn a_record_of_exactly_zero_units_is_refused() {
        let openings: Vec<_> = (1..=3).map(|day| buy(day, 0)).collect();
        let tiny_first = [
            parcel(1, &openings[0], dec!(0.00000001)),
            parcel(2, &openings[1], dec!(1)),
        ];
        let tiny_all: Vec<_> = (1..)
            .zip(&openings)
            .map(|(id, opening)| parcel(id, opening, dec!(0.00000001)))
            .collect();

        // 0.00000001 at one-for-three is 0.0000000033…, which rounds to nothing.
        assert_eq!(
            emit(&free(dec!(1.00000001), ratio(1, 3)), &tiny_first),
            Err(EmissionError::NotPositive {
                opening: TransactionId::new(1),
                quantity: dec!(0),
            })
        );
        // Each 0.000000005 rounds to 0.00000001; the total 0.000000015 rounds to 0.00000002,
        // which the first two exhaust.
        assert_eq!(
            emit(&free(dec!(0.00000003), ratio(1, 2)), &tiny_all),
            Err(EmissionError::NotPositive {
                opening: TransactionId::new(3),
                quantity: dec!(0),
            })
        );
    }

    /// A product beyond what a decimal at the quantity scale holds is refused rather than
    /// wrapped or truncated, and the largest one that fits is emitted exactly [DOM-115], ARC-010.
    #[test]
    fn a_product_beyond_the_quantity_scale_is_unrepresentable() {
        // The largest decimal mantissa, 2^96 - 1, at the quantity scale of 8.
        let largest = Decimal::from_i128_with_scale((1i128 << 96) - 1, 8);
        let half_of_largest = Decimal::from_i128_with_scale(((1i128 << 96) - 1) / 2, 8);
        let opening = buy(1, 0);

        assert_eq!(
            emit(&free(largest, ratio(2, 1)), &[parcel(1, &opening, largest)]),
            Err(EmissionError::Unrepresentable)
        );
        let emitted = emit(
            &free(half_of_largest, ratio(2, 1)),
            &[parcel(1, &opening, half_of_largest)],
        )
        .expect("emitted");
        assert_eq!(
            quantities(&emitted),
            [Decimal::from_i128_with_scale((1i128 << 96) - 2, 8)]
        );
    }

    proptest! {
        /// Whatever the parcels, the order they arrive in and the ratio, there is one record per
        /// parcel carrying that parcel's own cost and buy fee unchanged; each but the last in
        /// canonical order holds its consumed quantity times the ratio at the quantity scale, half
        /// away from zero, and the last what they leave of the transferred quantity times the
        /// ratio rounded once, so the records sum to within half a quantum of the exact product.
        /// Where that makes a record zero or less, the first such is refused, naming its opening
        /// [DOM-106], [DOM-115], TST-010 (DEC-106).
        ///
        /// The oracle rounds in integers of the 10^-8 quantum, `(2un + d) div 2d` for a positive
        /// `u * n / d`, so a wrong rounding mode or scale in the shared rational helpers is caught
        /// rather than repeated.
        #[test]
        fn records_preserve_count_quantity_and_basis(
            (consumed, arrival) in (1usize..8).prop_flat_map(|count| (
                prop::collection::vec(1i64..=1_000_000_000_000, count),
                Just((0..count).collect::<Vec<_>>()).prop_shuffle(),
            )),
            costs in prop::collection::vec(0i64..=10_000_000, 8),
            numerator in 1u32..=12,
            denominator in 1u32..=12,
        ) {
            // Parcel `k` is bought on day `k + 1`, so the canonical order is `k`'s; its row id
            // counts down, so that sorting by id instead would reverse it.
            let count = consumed.len();
            let id_of = |k: usize| i64::try_from(100 - k).expect("small");
            let openings: Vec<_> = (1..).take(count).map(|day| buy(day, 0)).collect();
            let parcels: Vec<_> = arrival
                .iter()
                .map(|&k| {
                    Parcel::new(
                        TransactionId::new(id_of(k)),
                        &openings[k],
                        Quantity::new(Decimal::new(consumed[k], 8)),
                        pair(Decimal::new(costs[k], 2), Decimal::new(costs[k] / 3, 2)),
                        pair(Decimal::new(costs[k] / 7, 2), Decimal::new(costs[k] / 11, 2)),
                    )
                })
                .collect();
            let transferred_units: i128 = consumed.iter().map(|&units| i128::from(units)).sum();
            let (n, d) = (i128::from(numerator), i128::from(denominator));
            let round = |units: i128| (2 * units * n + d).div_euclid(2 * d);

            let mut expected: Vec<i128> =
                consumed[..count - 1].iter().map(|&units| round(i128::from(units))).collect();
            expected.push(round(transferred_units) - expected.iter().sum::<i128>());
            let expected: Vec<Decimal> = expected
                .into_iter()
                .map(|units| Decimal::from_i128_with_scale(units, 8))
                .collect();

            let emitted = emit(
                &free(Decimal::from_i128_with_scale(transferred_units, 8), ratio(numerator, denominator)),
                &parcels,
            );

            match expected.iter().position(|quantity| *quantity <= Decimal::ZERO) {
                Some(k) => prop_assert_eq!(
                    emitted,
                    Err(EmissionError::NotPositive {
                        opening: TransactionId::new(id_of(k)),
                        quantity: expected[k],
                    })
                ),
                None => {
                    let emitted = emitted.expect("emitted");
                    prop_assert_eq!(emitted.len(), count);
                    for (k, ((id, transfer_in), quantity)) in
                        emitted.iter().zip(&expected).enumerate()
                    {
                        let parcel = parcels
                            .iter()
                            .find(|parcel| parcel.id == *id)
                            .expect("a record of a given parcel");
                        prop_assert_eq!(*id, TransactionId::new(id_of(k)), "canonical order");
                        prop_assert_eq!(transfer_in.quantity().get(), *quantity);
                        prop_assert_eq!(transfer_in.cost_basis(), parcel.cost);
                        prop_assert_eq!(transfer_in.fees(), parcel.buy_fee);
                    }
                    // Within half a quantum of the exact product: |sum * 10^8 * d - u * n| <= d / 2.
                    let sum = quantities(&emitted).iter().sum::<Decimal>();
                    let sum_units = i128::try_from(sum * Decimal::new(100_000_000, 0))
                        .expect("a whole number of quanta");
                    prop_assert!(2 * (sum_units * d - transferred_units * n).abs() <= d);
                }
            }
        }
    }
}
