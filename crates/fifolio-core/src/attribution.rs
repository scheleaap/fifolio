//! Approving an attribution: the allocations the user was shown, stored unchanged or not at all
//! [DOM-054], once they are checked to be an attribution at all [DOM-018], [DOM-019], [DOM-020],
//! [DOM-065], DEC-103.
//!
//! # Approve or decline, and nothing else
//!
//! [`approve`] hands the allocations to storage exactly as given, in their order, or refuses them
//! whole; nothing here adjusts, drops or reorders one, and there is no update path. Tying the
//! posted allocations to the proposal that was displayed is the server's fingerprint [SRV-040]:
//! this service does not recompute the FIFO proposal, so it stores any allocations that pass the
//! checks below, FIFO or not (DEC-104, provisional).
//!
//! Declining has no function because it does nothing: it stores nothing [DOM-055]. The closing
//! stays unattributed, and storage keeps refusing every later closing of its account and security
//! until it is approved [DOM-066]; the block is a consequence of the stored state, not a record
//! of the decline.
//!
//! # Where the checks sit
//!
//! In front of storage's own refusals — not a closing, already attributed, an earlier closing
//! unattributed — and in the same SQLite transaction as they and the write. Reading on the pool
//! instead would leave a window in which the checked opening is deleted and, row ids being
//! reused (`integer primary key` without `autoincrement`), a new transaction takes its id: the
//! allocation's foreign key would hold while nothing checked the new row.
//!
//! # A `transfer_out` emits as it is approved
//!
//! Approving a `transfer_out` also stores the `transfer_in` records it emits, one per consumed
//! parcel, and the links that tie them to it, in the same SQLite transaction as the allocations,
//! so approval and emission commit or fail together [DOM-090]. What each record carries is
//! [`crate::transfer`]'s; the cost and buy fee it is handed are each parcel's own opening side,
//! derived here from every allocation against that parcel, this one included [DOM-106]. A
//! transfer carrying a fee of its own is refused before anything is derived [DOM-107].

use std::collections::HashSet;

use rust_decimal::Decimal;
use sqlx::SqliteConnection;
use thiserror::Error;

use crate::allocation::{
    AgainstOpening, AllocationError, Half, OpeningShares, Uncovered, covered, opening_shares,
};
use crate::decimal::{Quantity, Scaled};
use crate::entities::{Account, Isin};
use crate::ordering::OrderKey;
use crate::storage::{
    Allocation, AttributionId, AttributionRepository, Database, Placement, StorageError,
    TransactionId, TransactionRepository,
};
use crate::transaction::{Closing, Opening, Transaction, TransferIn, TransferOut};
use crate::transfer::{EmissionError, Parcel, emit, refuse_own_fee};
use crate::valuation::Valued;

/// Why an attribution is refused before storage is asked to write it.
#[derive(Debug, Error)]
pub enum AttributionError {
    /// Only a closing is attributed [DOM-018].
    #[error("transaction {transaction} is not a closing")]
    NotAClosing { transaction: TransactionId },
    /// An `expiration` states no quantity, so its allocations have nothing to sum to [DOM-092],
    /// FIF-079 (DEC-102, provisional).
    #[error("expiration {closing} states no quantity for its allocations to sum to")]
    QuantityNotStated { closing: TransactionId },
    /// An attribution links a closing to one or more openings [DOM-018].
    #[error("closing {closing} is allocated to no opening")]
    NoAllocations { closing: TransactionId },
    /// Every allocated transaction must open a parcel [DOM-018].
    #[error("transaction {transaction} is not an opening")]
    NotAnOpening { transaction: TransactionId },
    /// An allocation consumes a positive quantity of its opening (DEC-103, provisional).
    #[error("the allocation against opening {opening} is not a positive quantity")]
    NotPositive { opening: TransactionId },
    /// An opening appears at most once among a closing's allocations, so the allocation that
    /// exhausts a parcel is one row [DOM-062] (DEC-103, provisional).
    #[error("opening {opening} is allocated more than once")]
    RepeatedOpening { opening: TransactionId },
    /// Every allocated opening belongs to the closing's account and security [DOM-019].
    #[error("opening {opening} is not of the account and security of closing {closing}")]
    OtherAccountOrSecurity {
        opening: TransactionId,
        closing: TransactionId,
    },
    /// Every allocated opening precedes the closing in canonical order [DOM-020].
    #[error("opening {opening} does not precede closing {closing} in canonical order")]
    NotBeforeClosing {
        opening: TransactionId,
        closing: TransactionId,
    },
    /// The allocations do not sum to the closing's quantity [DOM-065].
    #[error(transparent)]
    Uncovered(#[from] Uncovered),
    /// The `transfer_out` emits nothing, for the reason given: among them a fee of its own,
    /// which names its rows [DOM-107].
    #[error("transfer out {transfer_out} cannot be approved: {source}")]
    Emission {
        transfer_out: TransactionId,
        #[source]
        source: EmissionError,
    },
    /// A parcel's allocated cost or buy fee, which the record carrying it takes, cannot be
    /// derived [DOM-106].
    #[error(transparent)]
    Allocation(#[from] AllocationError),
    /// The records the `transfer_out` would emit into `target` sort before `transaction`, a split
    /// or closing there that they would reach back past (DEC-105, provisional).
    #[error(
        "transfer out {transfer_out} would emit parcels of {} that reach back past its \
         transaction {transaction}", .target.as_str()
    )]
    ReachesBackPast {
        transfer_out: TransactionId,
        target: Isin,
        transaction: TransactionId,
    },
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Stores the attribution of `closing` to `allocations`, unchanged, or refuses it and stores
/// nothing [DOM-054].
///
/// # Errors
///
/// An [`AttributionError`] naming the rule an allocation breaks; storage's own refusals and an
/// unknown transaction arrive as [`AttributionError::Storage`].
pub async fn approve(
    database: &Database,
    closing: TransactionId,
    allocations: &[Allocation],
) -> Result<AttributionId, AttributionError> {
    let mut tx = database.begin().await.map_err(AttributionError::Storage)?;

    let (closing_transaction, closing_party) = read(&mut tx, closing).await?;
    let closed = match closing_transaction
        .closing()
        .ok_or(AttributionError::NotAClosing {
            transaction: closing,
        })? {
        Closing::Sell(sell) => sell.quantity(),
        Closing::TransferOut(transfer_out) => {
            refuse_own_fee(transfer_out).map_err(|source| AttributionError::Emission {
                transfer_out: closing,
                source,
            })?;
            transfer_out.quantity()
        }
        Closing::Expiration(_) => return Err(AttributionError::QuantityNotStated { closing }),
    };

    let mut openings = Vec::with_capacity(allocations.len());
    let mut parcels = Vec::with_capacity(allocations.len());
    for allocation in allocations {
        let (transaction, party) = read(&mut tx, allocation.opening()).await?;
        let Transaction::Opening(opening) = transaction else {
            return Err(AttributionError::NotAnOpening {
                transaction: allocation.opening(),
            });
        };
        openings.push((party, allocation.quantity()));
        parcels.push(opening);
    }

    check(&closing_party, closed, &openings)?;

    // Derived before anything is written, so every allocation against a parcel read here is an
    // earlier closing's: DOM-066 leaves no later closing of the pair attributed.
    let emitted = match closing_transaction.closing() {
        Some(Closing::TransferOut(transfer_out)) => {
            emission(&mut tx, &closing_party, transfer_out, &openings, &parcels).await?
        }
        _ => Vec::new(),
    };

    let id = AttributionRepository::approve_in(&mut tx, closing, allocations).await?;
    for (parcel, target, transfer_in) in emitted {
        let placement = Placement::emitted(closing_party.account.clone(), target);
        let transfer_in =
            TransactionRepository::insert_in(&mut tx, &placement, &transfer_in.into()).await?;
        TransactionRepository::record_emission_in(&mut tx, closing, transfer_in, Some(parcel))
            .await?;
    }
    tx.commit().await.map_err(StorageError::from)?;
    Ok(id)
}

/// The `transfer_in` records approving `transfer_out` emits, each with the opening whose parcel it
/// carries [DOM-096] and the security it is placed in, derived from each consumed parcel's own
/// opening side [DOM-090], [DOM-106].
async fn emission(
    connection: &mut SqliteConnection,
    closing: &Party,
    transfer_out: &TransferOut,
    openings: &[(Party, Quantity)],
    parcels: &[Opening],
) -> Result<Vec<(TransactionId, Isin, TransferIn)>, AttributionError> {
    let splits =
        TransactionRepository::splits_in(&mut *connection, &closing.account, &closing.security)
            .await?;

    let mut carried = Vec::with_capacity(parcels.len());
    for ((party, quantity), opening) in openings.iter().zip(parcels) {
        let mut against: Vec<AgainstOpening> =
            AttributionRepository::against_opening_in(&mut *connection, party.id)
                .await?
                .into_iter()
                .map(|(closing, closed_at, quantity)| {
                    AgainstOpening::new(closing, closed_at, quantity)
                })
                .collect();
        against.push(AgainstOpening::new(closing.id, closing.key, *quantity));
        let share = |half| -> Result<OpeningShares, AllocationError> {
            opening_shares(opening, &splits, &against, half)?
                .into_iter()
                .find_map(|(of, shares)| (of == closing.id).then_some(shares))
                .ok_or(AllocationError::Unmeasurable {
                    closing: closing.id,
                })
        };
        let (native, eur) = (share(Half::Native)?, share(Half::Eur)?);
        carried.push(Parcel::new(
            party.id,
            opening,
            *quantity,
            Valued::new(native.cost(), eur.cost()),
            Valued::new(native.buy_fee(), eur.buy_fee()),
        ));
    }

    let target = transfer_out.target();
    if let Some(earliest) = openings.iter().map(|(party, _)| party.key).min()
        && let Some(transaction) = TransactionRepository::reached_back_past_in(
            connection,
            &closing.account,
            target,
            earliest,
            closing.key,
        )
        .await?
    {
        return Err(AttributionError::ReachesBackPast {
            transfer_out: closing.id,
            target: target.clone(),
            transaction,
        });
    }

    Ok(emit(transfer_out, &carried)
        .map_err(|source| AttributionError::Emission {
            transfer_out: closing.id,
            source,
        })?
        .into_iter()
        .map(|(parcel, transfer_in)| (parcel, target.clone(), transfer_in))
        .collect())
}

/// What the checks read of a stored transaction: its key, the pair it belongs to [DOM-013], and
/// its place in the canonical order [DOM-011].
#[derive(Debug, Clone, PartialEq, Eq)]
struct Party {
    id: TransactionId,
    account: Account,
    security: Isin,
    key: OrderKey,
}

async fn read(
    connection: &mut SqliteConnection,
    id: TransactionId,
) -> Result<(Transaction, Party), StorageError> {
    let unknown = || StorageError::UnknownTransaction { transaction: id };
    let transaction = TransactionRepository::find_in(&mut *connection, id)
        .await?
        .ok_or_else(unknown)?;
    let (account, security) = TransactionRepository::account_and_security_in(connection, id)
        .await?
        .ok_or_else(unknown)?;
    let key = transaction.order_key();
    Ok((
        transaction,
        Party {
            id,
            account,
            security,
            key,
        },
    ))
}

/// The rules an attribution answers to beyond storage's, in the order they are refused: at
/// least one opening [DOM-018]; per allocation, a positive quantity and an opening not already
/// allocated (DEC-103), the closing's account and security [DOM-019] and a place strictly before
/// it [DOM-020]; and a sum equal to the closed quantity [DOM-065].
fn check(
    closing: &Party,
    closed: Quantity,
    openings: &[(Party, Quantity)],
) -> Result<(), AttributionError> {
    if openings.is_empty() {
        return Err(AttributionError::NoAllocations {
            closing: closing.id,
        });
    }
    let mut seen = HashSet::with_capacity(openings.len());
    for (opening, quantity) in openings {
        if quantity.get() <= Decimal::ZERO {
            return Err(AttributionError::NotPositive {
                opening: opening.id,
            });
        }
        if !seen.insert(opening.id) {
            return Err(AttributionError::RepeatedOpening {
                opening: opening.id,
            });
        }
        if (&opening.account, &opening.security) != (&closing.account, &closing.security) {
            return Err(AttributionError::OtherAccountOrSecurity {
                opening: opening.id,
                closing: closing.id,
            });
        }
        // The stored key, then the row id for the tie two transactions derived from the same
        // lowest record in the same leg can share, as storage and the FIFO proposal order them
        // (DEC-095).
        if (opening.key, opening.id) >= (closing.key, closing.id) {
            return Err(AttributionError::NotBeforeClosing {
                opening: opening.id,
                closing: closing.id,
            });
        }
    }
    Ok(covered(
        closed,
        openings.iter().map(|(_, quantity)| *quantity),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    use crate::entities::Order;
    use crate::ordering::{BatchAge, Leg, RecordPosition};

    fn account() -> Account {
        Account::new("Saxo", "69900/1000000")
    }

    fn isin() -> Isin {
        Isin::new("NL0000009538")
    }

    fn key(day: u32, order: u32) -> OrderKey {
        OrderKey::new(
            NaiveDate::from_ymd_opt(2024, 5, day).expect("a valid date"),
            RecordPosition::new(Order::new(order), BatchAge::new(1)),
            Leg::Lead,
        )
    }

    fn party(id: i64, key: OrderKey) -> Party {
        Party {
            id: TransactionId::new(id),
            account: account(),
            security: isin(),
            key,
        }
    }

    fn closing() -> Party {
        party(10, key(10, 0))
    }

    fn quantity(value: rust_decimal::Decimal) -> Quantity {
        Quantity::new(value)
    }

    /// Two earlier openings of the same pair covering the closing exactly pass every check
    /// [DOM-019], [DOM-020], [DOM-065].
    #[test]
    fn earlier_openings_of_the_pair_covering_the_closing_pass() {
        let openings = [
            (party(1, key(1, 0)), quantity(dec!(6))),
            (party(2, key(2, 0)), quantity(dec!(4))),
        ];

        assert!(check(&closing(), quantity(dec!(10)), &openings).is_ok());
    }

    /// An attribution links a closing to one or more openings [DOM-018].
    #[test]
    fn no_opening_is_refused() {
        assert!(matches!(
            check(&closing(), quantity(dec!(10)), &[]),
            Err(AttributionError::NoAllocations { .. })
        ));
    }

    /// Another account or another security is refused, naming the opening [DOM-019].
    #[test]
    fn an_opening_of_another_account_or_security_is_refused() {
        let other_account = Party {
            account: Account::new("Saxo", "69900/2000000"),
            ..party(1, key(1, 0))
        };
        let other_security = Party {
            security: Isin::new("NL0011821202"),
            ..party(2, key(1, 0))
        };

        for (opening, id) in [(other_account, 1), (other_security, 2)] {
            match check(
                &closing(),
                quantity(dec!(10)),
                &[(opening, quantity(dec!(10)))],
            ) {
                Err(AttributionError::OtherAccountOrSecurity { opening, closing }) => {
                    assert_eq!(opening, TransactionId::new(id));
                    assert_eq!(closing, TransactionId::new(10));
                }
                other => panic!("expected the DOM-019 refusal, got {other:?}"),
            }
        }
    }

    /// An opening after the closing, or on a later record of the same day, is refused [DOM-020].
    #[test]
    fn an_opening_after_the_closing_is_refused() {
        for later in [key(11, 0), key(10, 1)] {
            assert!(matches!(
                check(
                    &closing(),
                    quantity(dec!(10)),
                    &[(party(1, later), quantity(dec!(10)))]
                ),
                Err(AttributionError::NotBeforeClosing { .. })
            ));
        }
    }

    /// On a shared order key the row id decides, strictly: a lower id precedes, a higher one does
    /// not, and the closing itself never precedes itself [DOM-020] (DEC-095).
    #[test]
    fn a_tied_key_is_decided_by_row_id() {
        let tied = closing().key;
        let lower = [(party(9, tied), quantity(dec!(10)))];
        let higher = [(party(11, tied), quantity(dec!(10)))];
        let itself = [(party(10, tied), quantity(dec!(10)))];

        assert!(check(&closing(), quantity(dec!(10)), &lower).is_ok());
        for openings in [higher, itself] {
            assert!(matches!(
                check(&closing(), quantity(dec!(10)), &openings),
                Err(AttributionError::NotBeforeClosing { .. })
            ));
        }
    }

    /// An opening on the closing's date, on an earlier record, precedes it though its row id is
    /// higher: the stored order decides before the row id does [DOM-020] (DEC-095).
    #[test]
    fn a_same_day_opening_on_an_earlier_record_precedes_whatever_its_row_id() {
        let closing = party(10, key(10, 1));
        let opening = [(party(99, key(10, 0)), quantity(dec!(10)))];

        assert!(check(&closing, quantity(dec!(10)), &opening).is_ok());
    }

    /// An allocation of zero, or a negative one offset by an excess elsewhere so the sum still
    /// holds, is refused, naming its opening (DEC-103, provisional).
    #[test]
    fn a_non_positive_allocation_is_refused() {
        let zero = [
            (party(1, key(1, 0)), quantity(dec!(10))),
            (party(2, key(2, 0)), quantity(dec!(0))),
        ];
        let negative = [
            (party(1, key(1, 0)), quantity(dec!(15))),
            (party(2, key(2, 0)), quantity(dec!(-5))),
        ];

        for openings in [zero, negative] {
            match check(&closing(), quantity(dec!(10)), &openings) {
                Err(AttributionError::NotPositive { opening }) => {
                    assert_eq!(opening, TransactionId::new(2));
                }
                other => panic!("expected the DEC-103 refusal, got {other:?}"),
            }
        }
    }

    /// One opening allocated twice is refused, naming it, though the sum holds (DEC-103,
    /// provisional).
    #[test]
    fn an_opening_allocated_twice_is_refused() {
        let openings = [
            (party(1, key(1, 0)), quantity(dec!(5))),
            (party(1, key(1, 0)), quantity(dec!(5))),
        ];

        match check(&closing(), quantity(dec!(10)), &openings) {
            Err(AttributionError::RepeatedOpening { opening }) => {
                assert_eq!(opening, TransactionId::new(1));
            }
            other => panic!("expected the DEC-103 refusal, got {other:?}"),
        }
    }

    /// One quantum short or one quantum over is refused, carrying both sums [DOM-065] (DEC-091).
    #[test]
    fn allocations_off_by_one_quantum_are_refused() {
        for allocated in [dec!(9.99999999), dec!(10.00000001)] {
            match check(
                &closing(),
                quantity(dec!(10)),
                &[(party(1, key(1, 0)), quantity(allocated))],
            ) {
                Err(AttributionError::Uncovered(uncovered)) => {
                    assert_eq!(uncovered.allocated, allocated);
                    assert_eq!(uncovered.closed, dec!(10));
                }
                other => panic!("expected the DOM-065 refusal, got {other:?}"),
            }
        }
    }

    /// A closing stated finer than the quantity scale is covered by its view there, not by what
    /// it states (DEC-091, DEC-100).
    #[test]
    fn a_closing_finer_than_the_quantity_scale_is_covered_by_its_view() {
        let opening = |allocated| [(party(1, key(1, 0)), quantity(allocated))];
        let closed = quantity(dec!(1.000000005));

        assert!(check(&closing(), closed, &opening(dec!(1.00000001))).is_ok());
        assert!(matches!(
            check(&closing(), closed, &opening(dec!(1.000000005))),
            Err(AttributionError::Uncovered(_))
        ));
    }
}
