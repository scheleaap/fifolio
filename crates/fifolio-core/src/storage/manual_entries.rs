//! The manual entry repository.
//!
//! The entry's account and security are columns; the records it answers are their broker
//! identities in a side table, never foreign keys, so an import undo that removes those records
//! leaves the entry intact [DOM-099], [DOM-100].
//!
//! # The lifecycle across an undo and a re-import
//!
//! An entry outlives the import it answered, so at any moment it either has the records it
//! names or it is waiting for them. [`ManualEntryRepository::waiting`] lists the second case,
//! each entry naming its account, its security and the identities that are absent [DOM-109];
//! [`ManualEntryRepository::reconnected`] answers the first for one import, handing back each
//! entry that import completed together with the records it answers [DOM-108]. Both read the
//! same rule — an identity is present when a stored source record carries it — so an entry
//! cannot be reported as waiting and as reconnected at once.
//!
//! Reconnection is a lookup by identity and asks the user nothing: the entry already holds what
//! was supplied, and the records return byte for byte because identity is computed from the row
//! [DOM-022], [DOM-024]. Deriving the transaction again from that pair is the import path's
//! (SRV-055, FIF-072) and each format's, not this repository's; what is restored here is the
//! pairing that derivation needs, which is what makes the restoration automatic rather than a
//! second completion queue. DOM-108's "restored automatically" is therefore only half met by this
//! module: no caller derives from [`ManualEntryRepository::reconnected`] yet, and the wiring that
//! makes an import do it without asking is FIF-072's.

use std::num::NonZeroU32;

use sqlx::sqlite::{SqlitePool, SqliteRow};
use sqlx::{Row, query};
use vec1::Vec1;

use crate::entities::{Isin, RecordIdentity, SourceRecord};
use crate::manual_entry::{Election, ManualEntry, Ratio, Supplied};
use crate::storage::codec::{at_scale, quantity as read_quantity};
use crate::storage::{BatchId, RecordHandle, SourceRecordRepository, StorageError, row_id};

row_id!(
    /// A manual entry's key.
    ManualEntryId
);

/// The stored code of each [`Supplied`] shape [DOM-097].
const ELECTION_STOCK: &str = "election_stock";
const ELECTION_CASH: &str = "election_cash";
const SPLIT: &str = "split";
const EXCHANGE: &str = "exchange";
const DISPOSAL: &str = "disposal";

pub struct ManualEntryRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> ManualEntryRepository<'a> {
    pub(super) fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, entry: &ManualEntry) -> Result<ManualEntryId, StorageError> {
        let columns = SuppliedColumns::of(entry.supplied())?;

        let mut tx = self.pool.begin().await?;
        let inserted = query(
            "insert into manual_entry
                 (account_broker, account_id, security_isin, supplied_kind,
                  shares, quantity, ratio_numerator, ratio_denominator, target_isin)
             values (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(entry.account().broker())
        .bind(entry.account().id())
        .bind(entry.security().as_str())
        .bind(columns.kind)
        .bind(columns.shares)
        .bind(columns.quantity)
        .bind(columns.numerator)
        .bind(columns.denominator)
        .bind(columns.target)
        .execute(&mut *tx)
        .await?;
        let id = inserted.last_insert_rowid();

        for (ordinal, identity) in (0i64..).zip(entry.answers()) {
            query(
                "insert into manual_entry_answer (manual_entry_id, ordinal, record_identity)
                 values (?, ?, ?)",
            )
            .bind(id)
            .bind(ordinal)
            .bind(identity.as_str())
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(ManualEntryId::new(id))
    }

    pub async fn find(&self, id: ManualEntryId) -> Result<Option<ManualEntry>, StorageError> {
        let Some(row) = query(
            "select account_broker, account_id, security_isin, supplied_kind,
                    shares, quantity, ratio_numerator, ratio_denominator, target_isin
             from manual_entry where id = ?",
        )
        .bind(id.get())
        .fetch_optional(self.pool)
        .await?
        else {
            return Ok(None);
        };

        let answers = query(
            "select record_identity from manual_entry_answer
             where manual_entry_id = ? order by ordinal",
        )
        .bind(id.get())
        .fetch_all(self.pool)
        .await?
        .into_iter()
        .map(|row| RecordIdentity::new(row.get::<String, _>("record_identity")));

        Ok(Some(ManualEntry::new(
            crate::entities::Account::new(
                row.get::<String, _>("account_broker"),
                row.get::<String, _>("account_id"),
            ),
            Isin::new(row.get::<String, _>("security_isin")),
            supplied(&row)?,
            answers,
        )))
    }

    /// The entries whose records are absent, each naming what it expects [DOM-109].
    ///
    /// Absence is a property of the identity, not of the import that last held it: an entry is
    /// waiting whenever no stored source record carries one of the identities it names, whether
    /// that is because an import was undone or because the file it answered was never imported
    /// into this database at all.
    ///
    /// An entry that names no record is not waiting for anything and is not listed here. What
    /// such an entry means is CLI-039's and SRV-026's, and reporting it as waiting for nothing
    /// would decide it.
    pub async fn waiting(&self) -> Result<Vec<WaitingEntry>, StorageError> {
        let mut waiting = Vec::new();
        for id in self.ids(None).await? {
            let Some(entry) = self.find(id).await? else {
                continue;
            };
            let missing: Vec<RecordIdentity> = self
                .resolve(&entry)
                .await?
                .into_iter()
                .filter_map(Result::err)
                .collect();
            if !missing.is_empty() {
                waiting.push(WaitingEntry { id, entry, missing });
            }
        }
        Ok(waiting)
    }

    /// The entries `batch` reconnects: those naming a record this import stored, whose every
    /// named identity is present again [DOM-108].
    ///
    /// Each comes back with the records it answers, in the order it names them, so that the
    /// transaction it completed can be derived again without asking the user anything.
    ///
    /// Scoped to the records the batch owns, which is what an import stores. A record an
    /// approval emitted belongs to no batch [DOM-090] and is not an import's to reconnect; that
    /// path is FIF-063's and undecided (OQ-002).
    ///
    /// The predicate is presence, not history: nothing stored distinguishes an entry that was
    /// waiting from one that never lost its records, so a first import of the rows an entry names
    /// reports it here exactly as a re-import does. **The caller owns the idempotency check** —
    /// before deriving, it must establish that the entry's transaction is not already stored, or
    /// SRV-055 duplicates it.
    pub async fn reconnected(&self, batch: BatchId) -> Result<Vec<ReconnectedEntry>, StorageError> {
        let mut reconnected = Vec::new();
        for id in self.ids(Some(batch)).await? {
            let Some(entry) = self.find(id).await? else {
                continue;
            };
            // `collect` over `Result` stops at the first absent identity, which is the entry
            // still waiting rather than a failure. The entry was found through a record the batch
            // owns, so it names at least one and the empty case cannot arise here.
            if let Some(records) = self
                .resolve(&entry)
                .await?
                .into_iter()
                .collect::<Result<Vec<SourceRecord>, RecordIdentity>>()
                .ok()
                .and_then(|records| Vec1::try_from_vec(records).ok())
            {
                reconnected.push(ReconnectedEntry { id, entry, records });
            }
        }
        Ok(reconnected)
    }

    /// The stored entries, or those naming a record `batch` owns when one is given.
    async fn ids(&self, batch: Option<BatchId>) -> Result<Vec<ManualEntryId>, StorageError> {
        let rows = match batch {
            None => {
                query("select id from manual_entry order by id")
                    .fetch_all(self.pool)
                    .await?
            }
            Some(batch) => {
                query(
                    "select distinct a.manual_entry_id as id
                 from manual_entry_answer a
                 join source_record r on r.identity = a.record_identity
                 where r.batch_id = ?
                 order by a.manual_entry_id",
                )
                .bind(batch.get())
                .fetch_all(self.pool)
                .await?
            }
        };
        Ok(rows
            .iter()
            .map(|row| ManualEntryId::new(row.get("id")))
            .collect())
    }

    /// The records `entry` names as they stand: the record where the identity is present, the
    /// identity itself where it is not. The one rule both listings above read.
    async fn resolve(
        &self,
        entry: &ManualEntry,
    ) -> Result<Vec<Result<SourceRecord, RecordIdentity>>, StorageError> {
        let records = SourceRecordRepository::new(self.pool);
        let mut resolved = Vec::with_capacity(entry.answers().len());
        for identity in entry.answers() {
            resolved.push(
                records
                    .find(identity)
                    .await?
                    .ok_or_else(|| identity.clone()),
            );
        }
        Ok(resolved)
    }
}

/// A stored entry that is waiting, and the identities it is waiting for [DOM-109].
///
/// The entry carries the account and the security, so what it names is the whole of "what it
/// expects": which completion, against which holding, and which rows would satisfy it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingEntry {
    id: ManualEntryId,
    entry: ManualEntry,
    missing: Vec<RecordIdentity>,
}

impl WaitingEntry {
    #[must_use]
    pub fn id(&self) -> ManualEntryId {
        self.id
    }

    #[must_use]
    pub fn entry(&self) -> &ManualEntry {
        &self.entry
    }

    /// The identities this entry names that no stored record carries, in the order it names
    /// them.
    #[must_use]
    pub fn missing(&self) -> &[RecordIdentity] {
        &self.missing
    }
}

/// A stored entry an import reconnected, with the records it answers [DOM-108].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconnectedEntry {
    id: ManualEntryId,
    entry: ManualEntry,
    records: Vec1<SourceRecord>,
}

impl ReconnectedEntry {
    #[must_use]
    pub fn id(&self) -> ManualEntryId {
        self.id
    }

    #[must_use]
    pub fn entry(&self) -> &ManualEntry {
        &self.entry
    }

    /// The records the entry answers, in the order it names them, which is the order the
    /// completion queue showed them in [DOM-098].
    #[must_use]
    pub fn records(&self) -> &[SourceRecord] {
        &self.records
    }

    /// A handle on each of [`Self::records`], in the same order, so that the transaction the
    /// entry completed can be derived again from records storage has just read [DOM-047],
    /// [DOM-108].
    #[must_use]
    pub fn handles(&self) -> Vec1<RecordHandle> {
        self.records
            .mapped_ref(|record| RecordHandle::new(record.identity().clone()))
    }
}

/// The columns one [`Supplied`] shape fills, so every shape is written in one place and the
/// nullable columns cannot be filled inconsistently with the kind.
struct SuppliedColumns {
    kind: &'static str,
    shares: Option<String>,
    quantity: Option<String>,
    numerator: Option<i64>,
    denominator: Option<i64>,
    target: Option<String>,
}

impl SuppliedColumns {
    fn empty(kind: &'static str) -> Self {
        Self {
            kind,
            shares: None,
            quantity: None,
            numerator: None,
            denominator: None,
            target: None,
        }
    }

    fn with_ratio(self, ratio: Ratio) -> Self {
        Self {
            numerator: Some(i64::from(ratio.numerator().get())),
            denominator: Some(i64::from(ratio.denominator().get())),
            ..self
        }
    }

    fn of(supplied: &Supplied) -> Result<Self, StorageError> {
        Ok(match supplied {
            Supplied::Election(Election::Stock { shares }) => Self {
                shares: Some(at_scale("shares", *shares)?),
                ..Self::empty(ELECTION_STOCK)
            },
            Supplied::Election(Election::Cash) => Self::empty(ELECTION_CASH),
            Supplied::Split(ratio) => Self::empty(SPLIT).with_ratio(*ratio),
            Supplied::Exchange { target, ratio } => Self {
                target: Some(target.as_str().to_owned()),
                ..Self::empty(EXCHANGE).with_ratio(*ratio)
            },
            Supplied::Disposal { quantity, target } => Self {
                quantity: Some(at_scale("quantity", *quantity)?),
                target: target.as_ref().map(|isin| isin.as_str().to_owned()),
                ..Self::empty(DISPOSAL)
            },
        })
    }
}

fn required<T>(field: &'static str, value: Option<T>) -> Result<T, StorageError> {
    value.ok_or(StorageError::CorruptValue {
        field,
        value: String::new(),
    })
}

fn ratio(row: &SqliteRow) -> Result<Ratio, StorageError> {
    let part = |field: &'static str| -> Result<NonZeroU32, StorageError> {
        let stored = required(field, row.get::<Option<i64>, _>(field))?;
        u32::try_from(stored)
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or(StorageError::CorruptValue {
                field,
                value: stored.to_string(),
            })
    };
    Ok(Ratio::new(
        part("ratio_numerator")?,
        part("ratio_denominator")?,
    ))
}

fn supplied(row: &SqliteRow) -> Result<Supplied, StorageError> {
    let target = |field: &'static str| -> Option<Isin> {
        row.get::<Option<String>, _>(field).map(Isin::new)
    };

    match row.get::<String, _>("supplied_kind").as_str() {
        ELECTION_STOCK => Ok(Supplied::Election(Election::Stock {
            shares: read_quantity(
                "shares",
                &required("shares", row.get::<Option<String>, _>("shares"))?,
            )?,
        })),
        ELECTION_CASH => Ok(Supplied::Election(Election::Cash)),
        SPLIT => Ok(Supplied::Split(ratio(row)?)),
        EXCHANGE => Ok(Supplied::Exchange {
            target: required("target_isin", target("target_isin"))?,
            ratio: ratio(row)?,
        }),
        DISPOSAL => Ok(Supplied::Disposal {
            quantity: read_quantity(
                "quantity",
                &required("quantity", row.get::<Option<String>, _>("quantity"))?,
            )?,
            target: target("target_isin"),
        }),
        other => Err(StorageError::CorruptValue {
            field: "supplied_kind",
            value: other.to_owned(),
        }),
    }
}
