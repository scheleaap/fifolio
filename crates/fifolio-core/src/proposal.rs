//! The attribution proposal as it is shown: a stored closing, the FIFO allocations offered for
//! it, and each allocation's derived figures [SRV-036], [DOM-054], [DOM-056], [DOM-059]; or the
//! refusal that stands in its place [SRV-038], [SRV-039].
//!
//! Read-only. Everything is read inside one SQLite transaction that is dropped, never committed,
//! so the queue, the pending records, the openings and the allocations it derives from are one
//! snapshot and nothing is written [SRV-036].
//!
//! # What is refused, in order
//!
//! For a named closing: an unknown transaction, one that is not a closing, a security with
//! pending records in the closing's account [SRV-039], [DOM-049], a closing already attributed,
//! and a closing with an earlier one of its account and security still unattributed [DOM-066]
//! (DEC-127, provisional). For "the next closing awaiting attribution" [SRV-037]: an unknown
//! account or security, pending records, and nothing awaiting. Past those, the FIFO engine either
//! proposes or names the shortfall [DOM-057], [SRV-038].
//!
//! # Which figures
//!
//! The EUR half only: the tax figures are the EUR ones [DOM-084], and a gain across a native
//! opening and a native closing in two currencies would be no figure at all (DEC-127,
//! provisional). A `sell` shows all five of [`Figures`]; a `transfer_out` realizes nothing
//! [DOM-093] and shows only the opening side it carries onward [DOM-106]. Each is derived exactly
//! as approving would store it and as the reports then read it: the opening side from every
//! allocation already made against the parcel plus this one, the closing side from this closing's
//! own proposed allocations [DOM-060], [DOM-062], [DOM-063].
//!
//! The fingerprint over what is shown is the server's [SRV-050]: it hashes the serialization it
//! displays, so it covers exactly the figures a client was shown and no others.

use sqlx::SqliteConnection;
use thiserror::Error;

use crate::allocation::{
    AgainstOpening, AllocationError, Figures, Half, OfClosing, OpeningShares, closing_shares,
    opening_shares,
};
use crate::decimal::{Quantity, Scaled};
use crate::entities::{Account, Isin, RecordIdentity};
use crate::fifo::{PriorAllocation, Proposal, ProposalError, propose};
use crate::storage::{
    Allocation, AttributionRepository, Database, RecordFilter, RecordStatus,
    SourceRecordRepository, StorageError, StoredTransaction, TransactionFilter, TransactionId,
    TransactionRepository,
};
use crate::transaction::{Closing, Opening, Transaction};

/// A proposal as it is displayed: the closing, and the allocations offered for it in canonical
/// order of their openings, each with its figures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    closing: StoredTransaction,
    allocations: Vec<OfferedAllocation>,
}

impl Offer {
    #[must_use]
    pub fn closing(&self) -> &StoredTransaction {
        &self.closing
    }

    #[must_use]
    pub fn allocations(&self) -> &[OfferedAllocation] {
        &self.allocations
    }
}

/// One offered allocation and the EUR figures it would carry once approved [DOM-059].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfferedAllocation {
    allocation: Allocation,
    figures: OfferedFigures,
}

impl OfferedAllocation {
    #[must_use]
    pub fn allocation(&self) -> Allocation {
        self.allocation
    }

    #[must_use]
    pub fn figures(&self) -> OfferedFigures {
        self.figures
    }
}

/// The figures an allocation shows, by the kind of closing it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferedFigures {
    /// A `sell`: both sides and the gain they make [DOM-059], [DOM-125].
    Disposal(Figures),
    /// A `transfer_out`: the cost and buy fee the parcel carries onward, and no proceeds or gain
    /// [DOM-093], [DOM-106].
    Transfer(OpeningShares),
}

/// Why no proposal is shown.
#[derive(Debug, Error)]
pub enum ProposalRefusal {
    /// Only a closing is proposed for [DOM-054].
    #[error("transaction {transaction} is not a closing")]
    NotAClosing { transaction: TransactionId },
    /// The account's holdings of the security are known to be incomplete [SRV-039], [DOM-049].
    #[error(
        "security {} has pending source records in account {} {}: {}",
        .security.as_str(), .account.broker(), .account.id(), list(.records)
    )]
    PendingRecords {
        account: Account,
        security: Isin,
        records: Vec<RecordIdentity>,
    },
    /// An attributed closing has nothing left to propose; its attribution is what was approved.
    #[error("closing {closing} is already attributed")]
    AlreadyAttributed { closing: TransactionId },
    /// Closings are attributed in canonical order, so `earlier` comes first [DOM-066].
    #[error("closing {closing} waits on the earlier unattributed closing {earlier}")]
    EarlierUnattributed {
        closing: TransactionId,
        earlier: TransactionId,
    },
    /// Every closing of the account and security is attributed [SRV-037].
    #[error(
        "no closing of security {} in account {} {} awaits attribution",
        .security.as_str(), .account.broker(), .account.id()
    )]
    NothingAwaiting { account: Account, security: Isin },
    /// Too little is unattributed to cover the closing [DOM-057], [SRV-038].
    #[error(
        "closing {closing} cannot be covered: {} more would have to be open", .missing.get()
    )]
    Shortfall {
        closing: TransactionId,
        missing: Quantity,
    },
    /// The engine gives neither a proposal nor a shortfall.
    #[error("no proposal for closing {closing}: {source}")]
    Engine {
        closing: TransactionId,
        #[source]
        source: ProposalError,
    },
    /// An offered allocation's figures cannot be derived.
    #[error("the figures of closing {closing}'s proposal cannot be derived: {source}")]
    Figures {
        closing: TransactionId,
        #[source]
        source: AllocationError,
    },
    #[error(transparent)]
    Storage(#[from] StorageError),
}

fn list(records: &[RecordIdentity]) -> String {
    records
        .iter()
        .map(RecordIdentity::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The proposal for `closing` [SRV-036].
///
/// # Errors
///
/// A [`ProposalRefusal`], in the order the module documentation lists.
pub async fn propose_for(
    database: &Database,
    closing: TransactionId,
) -> Result<Offer, ProposalRefusal> {
    let mut tx = database.begin().await?;
    let stored = TransactionRepository::read_in(&mut tx, closing)
        .await?
        .ok_or(StorageError::UnknownTransaction {
            transaction: closing,
        })?;
    if stored.transaction().closing().is_none() {
        return Err(ProposalRefusal::NotAClosing {
            transaction: closing,
        });
    }
    refuse_pending(&mut tx, stored.account(), stored.security()).await?;
    // The pair's queue answers both: a closing absent from it is attributed, and one behind
    // another waits on it.
    let queue = awaiting(&mut tx, stored.account(), stored.security()).await?;
    if !queue.iter().any(|waiting| waiting.id() == closing) {
        return Err(ProposalRefusal::AlreadyAttributed { closing });
    }
    if let Some(earlier) = queue.first()
        && earlier.id() != closing
    {
        return Err(ProposalRefusal::EarlierUnattributed {
            closing,
            earlier: earlier.id(),
        });
    }
    offer(&mut tx, stored).await
}

/// The proposal for the first closing of `security` in `account` that awaits attribution, in
/// canonical order [SRV-037]. DOM-066 lets no other be attributed first.
///
/// # Errors
///
/// A [`ProposalRefusal`]; an account or security that is not stored arrives as
/// [`ProposalRefusal::Storage`], as it does for the transaction list.
pub async fn propose_next(
    database: &Database,
    account: &Account,
    security: &Isin,
) -> Result<Offer, ProposalRefusal> {
    let mut tx = database.begin().await?;
    // Read first, so an unknown account or security is refused as such rather than reported as
    // having nothing pending.
    let queue = awaiting(&mut tx, account, security).await?;
    refuse_pending(&mut tx, account, security).await?;
    let first = queue
        .into_iter()
        .next()
        .ok_or_else(|| ProposalRefusal::NothingAwaiting {
            account: account.clone(),
            security: security.clone(),
        })?;
    offer(&mut tx, first).await
}

/// The unattributed closings of the pair, in canonical order [SRV-029].
async fn awaiting(
    connection: &mut SqliteConnection,
    account: &Account,
    security: &Isin,
) -> Result<Vec<StoredTransaction>, StorageError> {
    TransactionRepository::list_in(
        connection,
        &TransactionFilter {
            account: Some(account.clone()),
            security: Some(security.clone()),
            unattributed_closings: true,
            ..TransactionFilter::default()
        },
    )
    .await
}

/// Refuses while any source record of the account naming the security is pending, as the
/// completion queue lists them (DEC-119, DEC-120, provisional) [SRV-039].
async fn refuse_pending(
    connection: &mut SqliteConnection,
    account: &Account,
    security: &Isin,
) -> Result<(), ProposalRefusal> {
    let records: Vec<RecordIdentity> = SourceRecordRepository::list_in(
        connection,
        &RecordFilter {
            account: Some(account.clone()),
            security: Some(security.clone()),
            status: Some(RecordStatus::Pending),
            ..RecordFilter::default()
        },
    )
    .await?
    .iter()
    .map(|stored| stored.record().identity().clone())
    .collect();
    if records.is_empty() {
        Ok(())
    } else {
        Err(ProposalRefusal::PendingRecords {
            account: account.clone(),
            security: security.clone(),
            records,
        })
    }
}

/// An opening of the pair with every allocation already made against it.
struct Candidate {
    id: TransactionId,
    opening: Opening,
    against: Vec<AgainstOpening>,
    prior: Vec<PriorAllocation>,
}

/// The FIFO proposal for `stored`, a closing, with its figures, or its shortfall.
async fn offer(
    connection: &mut SqliteConnection,
    stored: StoredTransaction,
) -> Result<Offer, ProposalRefusal> {
    let closing_id = stored.id();
    let Transaction::Closing(closing) = stored.transaction() else {
        return Err(ProposalRefusal::NotAClosing {
            transaction: closing_id,
        });
    };
    let splits =
        TransactionRepository::splits_in(&mut *connection, stored.account(), stored.security())
            .await?;

    let mut candidates = Vec::new();
    for stored_opening in
        TransactionRepository::openings_in(&mut *connection, Some(stored.account()))
            .await?
            .into_iter()
            .filter(|opening| opening.security == *stored.security())
    {
        let id = stored_opening.id;
        let Some(Transaction::Opening(opening)) =
            TransactionRepository::find_in(&mut *connection, id).await?
        else {
            return Err(StorageError::CorruptValue {
                field: "kind",
                value: id.to_string(),
            }
            .into());
        };
        let stored_against =
            AttributionRepository::against_opening_in(&mut *connection, id).await?;
        candidates.push(Candidate {
            id,
            opening,
            against: stored_against
                .iter()
                .map(|(closing, closed_at, quantity)| {
                    AgainstOpening::new(*closing, *closed_at, *quantity)
                })
                .collect(),
            prior: stored_against
                .iter()
                .map(|(_, closed_at, quantity)| {
                    PriorAllocation::new(*closed_at, Allocation::new(id, *quantity))
                })
                .collect(),
        });
    }

    let prior: Vec<PriorAllocation> = candidates
        .iter()
        .flat_map(|candidate| candidate.prior.iter().copied())
        .collect();
    let allocations = match propose(
        closing,
        candidates
            .iter()
            .map(|candidate| (candidate.id, &candidate.opening)),
        &splits,
        &prior,
    )
    .map_err(|source| ProposalRefusal::Engine {
        closing: closing_id,
        source,
    })? {
        Proposal::Allocate(allocations) => allocations,
        Proposal::Shortfall { missing } => {
            return Err(ProposalRefusal::Shortfall {
                closing: closing_id,
                missing,
            });
        }
    };

    let figures = |source| ProposalRefusal::Figures {
        closing: closing_id,
        source,
    };
    let at = closing.derivation().order_key();
    let mut opening_side = Vec::with_capacity(allocations.len());
    for allocation in &allocations {
        // `propose` offers only openings it was given, so the candidate is there.
        let candidate = candidates
            .iter()
            .find(|candidate| candidate.id == allocation.opening())
            .ok_or_else(|| {
                figures(AllocationError::Unmeasurable {
                    closing: closing_id,
                })
            })?;
        // This allocation joins the stored ones, as approving would store it, so the one that
        // exhausts the parcel absorbs its drift [DOM-062].
        let mut against = candidate.against.clone();
        against.push(AgainstOpening::new(closing_id, at, allocation.quantity()));
        let shares = opening_shares(&candidate.opening, &splits, &against, Half::Eur)
            .map_err(figures)?
            .into_iter()
            .find_map(|(of, shares)| (of == closing_id).then_some(shares))
            .ok_or_else(|| {
                figures(AllocationError::Unmeasurable {
                    closing: closing_id,
                })
            })?;
        opening_side.push((
            OfClosing::new(
                candidate.id,
                candidate.opening.derivation().order_key(),
                allocation.quantity(),
            ),
            shares,
        ));
    }

    let closing_side = match closing {
        Closing::TransferOut(_) => None,
        Closing::Sell(_) | Closing::Expiration(_) => {
            let of_closing: Vec<OfClosing> = opening_side.iter().map(|(of, _)| *of).collect();
            Some(closing_shares(closing, &of_closing, Half::Eur).map_err(figures)?)
        }
    };

    let offered = allocations
        .iter()
        .zip(&opening_side)
        .map(|(allocation, (_, opening))| {
            let figures_of = match &closing_side {
                None => OfferedFigures::Transfer(*opening),
                Some(shares) => OfferedFigures::Disposal(Figures::new(
                    *opening,
                    shares
                        .iter()
                        .find_map(|(of, shares)| (*of == allocation.opening()).then_some(*shares))
                        .ok_or_else(|| {
                            figures(AllocationError::Unmeasurable {
                                closing: closing_id,
                            })
                        })?,
                )),
            };
            Ok(OfferedAllocation {
                allocation: *allocation,
                figures: figures_of,
            })
        })
        .collect::<Result<Vec<_>, ProposalRefusal>>()?;

    Ok(Offer {
        closing: stored,
        allocations: offered,
    })
}
