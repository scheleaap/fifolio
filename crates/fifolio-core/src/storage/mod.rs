//! The SQLite database, its migrations and the repositories over it.
//!
//! One local file [ARC-011], `./fifolio.db` in the working directory unless a caller names
//! another [ARC-013], created on first connection [ARC-014]. Versioned migrations are applied
//! when the database is opened [ARC-012], so a process that starts is a process whose schema is
//! current; there is no separate migrate command to forget.
//!
//! # What is stored, and what is deliberately not
//!
//! The types built so far: [`Account`], [`Security`], [`SourceRecord`], [`ImportBatch`], the six
//! [`Transaction`] variants with their [`Valued`] pairs and [`Conversion`], and [`ManualEntry`]
//! with its [`Supplied`] shapes, plus the ECB daily reference rates the imports resolve against
//! [ARC-015], the approved attributions with their allocations, and the relations the invariants
//! below read: a transaction's account and security [DOM-013] and the import that derived it, the
//! source records it was derived from [DOM-013], [DOM-016], its place in the canonical order
//! [DOM-011], a source record's owning batch and the oldest batch that supplied it [DOM-111], and
//! the `transfer_in` a `transfer_out` emitted. No column exists for a rule that is still
//! undecided — which of its records a transaction *consumes* rather than cites is DOM-101 and
//! FIF-058's — because a guessed column is a schema that must be unpicked rather than extended.
//!
//! [`Account`]: crate::entities::Account
//! [`Security`]: crate::entities::Security
//! [`SourceRecord`]: crate::entities::SourceRecord
//! [`ImportBatch`]: crate::entities::ImportBatch
//! [`Transaction`]: crate::transaction::Transaction
//! [`Valued`]: crate::valuation::Valued
//! [`Conversion`]: crate::valuation::Conversion
//! [`ManualEntry`]: crate::manual_entry::ManualEntry
//! [`Supplied`]: crate::manual_entry::Supplied
//!
//! # The scale boundary
//!
//! Persistence is one of the two boundaries at which a value must already be at its scale
//! [ARC-009, ARC-010]. The repositories enforce it: a figure carrying more decimals than its
//! kind is refused with [`StorageError::UnscaledValue`] rather than truncated on the way into a
//! column, so a rounding the caller never performed cannot be attributed to it afterwards. See
//! [`codec::at_scale`].
//!
//! # The invariants, and where they are refused
//!
//! The lifecycle invariants are enforced here rather than trusted to a caller: attribution in
//! canonical order [DOM-066], deletion of an attribution [DOM-068], the immutability of an
//! attributed transaction [DOM-069], the emitted `transfer_in` [DOM-094], and the two refusals a
//! batch deletion answers to [DOM-072], [DOM-119]. Each has an error of its own on
//! [`StorageError`], so a caller can tell which rule it met. A manual entry has no relation to an
//! import at all, which is what makes DOM-110 a property of the schema rather than a check.
//!
//! What an entry does have is the lifecycle that invariant protects: it is listed as waiting
//! while the records it names are absent and reconnected to them when an import brings them back
//! [DOM-108], [DOM-109]. Both are [`ManualEntryRepository`]'s.
//!
//! What is *not* here is computation: which openings a closing should consume is the FIFO
//! engine's, the allocated figures are derived on demand, and both are other items'. A
//! repository stores the allocation a caller approved and refuses the ones the invariants
//! forbid.

mod attributions;
mod codec;
mod entities;
mod manual_entries;
mod rates;
mod transactions;

use std::path::Path;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};

use crate::entities::RecordIdentity;
use crate::ordering::RecordPosition;

pub(crate) use attributions::Disposal;
pub use attributions::{Allocation, Attribution, AttributionId, AttributionRepository};
pub use entities::{
    AccountRepository, BatchId, ImportBatchRepository, RecordFilter, RecordStatus,
    SecurityRepository, SourceRecordRepository, StoredSourceRecord,
};
pub use manual_entries::{ManualEntryId, ManualEntryRepository, ReconnectedEntry, WaitingEntry};
pub use rates::{CachedRates, RateRepository};
pub(crate) use transactions::StoredOpening;
pub use transactions::{Placement, TransactionId, TransactionRepository};

/// The database file used when nothing names another [ARC-013].
///
/// A constant rather than a default argument: the binary that reads `--database` is the one
/// place a default belongs, and it passes whichever path it ended up with to [`Database::open`].
pub const DEFAULT_DATABASE_PATH: &str = "./fifolio.db";

/// The versioned migrations, embedded in the binary so that a deployed process carries its own
/// schema history [ARC-012].
static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Storage's word that a source record exists: the only thing a
/// [`Derivation`](crate::transaction::Derivation) is made from [DOM-047].
///
/// Its constructor is private to this module, so outside it a handle is obtained in exactly three
/// ways: [`SourceRecordRepository::insert`] issues one for the record it has just written,
/// [`SourceRecordRepository::handle`] one for a record it holds now, and
/// [`ReconnectedEntry::handles`] one for each record an import brought back. Each carries the
/// record's [`RecordPosition`], which is what a transaction's `order` is taken from [DOM-011]. A record identity
/// alone is not enough, since [`crate::identity::identify`] computes one for a row that was
/// never read; a handle is what makes "derived from source records" a fact of the types rather
/// than a check someone must remember.
///
/// Reading a stored transaction back also rebuilds handles, from its own citations. That is
/// storage vouching for what it already holds: those citations were written from handles, so
/// the induction holds rather than being broken by the read.
///
/// A handle says the record was stored when it was issued, not that it stays stored: an import
/// undo removes records together with what was derived from them, and refuses while anything
/// else cites them [DOM-119]. A handle is also not tied to the database that issued it. So
/// [`TransactionRepository::insert`] refuses a citation of a record not stored at that moment,
/// which closes both gaps where the transaction is created [DOM-047].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordHandle {
    identity: RecordIdentity,
    position: RecordPosition,
}

impl RecordHandle {
    fn new(identity: RecordIdentity, position: RecordPosition) -> Self {
        Self { identity, position }
    }

    /// A unit test outside storage has no database to be issued a handle by; this compiles into
    /// the crate's own test build and nowhere else.
    #[cfg(test)]
    pub(crate) fn for_test(identity: RecordIdentity, position: RecordPosition) -> Self {
        Self { identity, position }
    }

    #[must_use]
    pub fn identity(&self) -> &RecordIdentity {
        &self.identity
    }

    /// Where the record sits in the canonical order [DOM-111], as storage holds it: both halves
    /// are fixed when the record is first stored, so the position a handle carries cannot go
    /// stale [DOM-008], DEC-092.
    #[must_use]
    pub fn position(&self) -> RecordPosition {
        self.position
    }

    #[must_use]
    pub fn into_identity(self) -> RecordIdentity {
        self.identity
    }
}

/// Anything storage refuses or cannot make sense of.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// A security's ISIN is unique [DOM-071].
    #[error("a security with ISIN {isin} already exists")]
    DuplicateIsin { isin: String },
    /// An account is its broker and id, so a second one with both is the same account.
    #[error("an account {broker} {id} already exists")]
    DuplicateAccount { broker: String, id: String },
    #[error("no account {broker} {id} is stored")]
    UnknownAccount { broker: String, id: String },
    #[error("no security with ISIN {isin} is stored")]
    UnknownSecurity { isin: String },
    /// A transaction is derived only from records storage holds at the moment it is stored
    /// [DOM-047]; a citation may outlive its record afterwards [DOM-099], never precede it.
    #[error("no source record {identity} is stored")]
    UnknownRecord { identity: String },
    /// An account that anything stored refers to stays, and the refusal says what refers to it
    /// [SRV-008].
    #[error(
        "account {broker} {id} is referenced by {source_records} source records, {batches} \
         import batches, {manual_entries} manual entries and {transactions} transactions"
    )]
    AccountReferenced {
        broker: String,
        id: String,
        source_records: u64,
        batches: u64,
        manual_entries: u64,
        transactions: u64,
    },
    /// A security that a source record or a transaction names stays [SRV-009].
    #[error(
        "security {isin} is referenced by {source_records} source records and {transactions} \
         transactions"
    )]
    SecurityReferenced {
        isin: String,
        source_records: u64,
        transactions: u64,
    },
    /// The scale boundary, refused rather than rounded [ARC-009, ARC-010].
    #[error("{field} is {value}, which carries more than the {scale} decimals it is stored at")]
    UnscaledValue {
        field: &'static str,
        value: String,
        scale: u32,
    },
    /// A stored value no longer parses as what its column holds — a schema edited by hand, or a
    /// code written by a version that is no longer this one.
    #[error("the stored {field} {value:?} cannot be read back")]
    CorruptValue { field: &'static str, value: String },
    /// No transaction with this id is stored, so nothing can be said about it.
    #[error("no transaction {transaction} is stored")]
    UnknownTransaction { transaction: TransactionId },
    #[error("no import batch {batch} is stored")]
    UnknownBatch { batch: BatchId },
    /// Both attribution invariants are stated over closings, so an opening cannot be attributed
    /// [DOM-066], [DOM-068].
    #[error("transaction {transaction} is a {kind} and not a closing")]
    NotAClosing {
        transaction: TransactionId,
        kind: String,
    },
    /// A relation stated over two kinds refuses a transaction of any other: the emission link is
    /// between a `transfer_out` and the `transfer_in` it emitted, and freezing an unrelated
    /// transaction under it would make it deletable only through a transaction it has nothing to
    /// do with [DOM-094].
    #[error("transaction {transaction} is a {kind} and not a {expected}")]
    NotOfKind {
        transaction: TransactionId,
        expected: &'static str,
        kind: String,
    },
    /// A closing is attributed once; re-approving it is a second attribution of the same closing
    /// [DOM-066].
    #[error("closing {closing} already carries attribution {attribution}")]
    ClosingAlreadyAttributed {
        closing: TransactionId,
        attribution: AttributionId,
    },
    /// Closings are attributed in canonical order per account and security [DOM-066].
    #[error(
        "closing {closing} cannot be attributed while the earlier closing {earlier} of the same \
         account and security is unattributed"
    )]
    EarlierClosingUnattributed {
        closing: TransactionId,
        earlier: TransactionId,
    },
    /// An attribution may only be deleted if no later one exists for that pair [DOM-068].
    #[error(
        "attribution {attribution} cannot be deleted while the later attribution {later} of the \
         same account and security exists"
    )]
    LaterAttributionExists {
        attribution: AttributionId,
        later: AttributionId,
    },
    /// A transaction that participates in an attribution is immutable [DOM-069].
    #[error("transaction {transaction} participates in attribution {attribution}")]
    TransactionAttributed {
        transaction: TransactionId,
        attribution: AttributionId,
    },
    /// An emitted `transfer_in` is deleted with the `transfer_out` that emitted it, never on its
    /// own [DOM-094].
    #[error("transfer_in {transfer_in} was emitted by transfer_out {transfer_out}")]
    EmittedTransferIn {
        transfer_in: TransactionId,
        transfer_out: TransactionId,
    },
    /// A batch whose transactions are attributed stays [DOM-072].
    #[error("batch {batch} derived transactions that participate in an attribution: {}", ids(.transactions))]
    BatchTransactionAttributed {
        batch: BatchId,
        transactions: Vec<TransactionId>,
    },
    /// A batch whose records another import's transactions cite stays, and the refusal names
    /// those transactions [DOM-119].
    #[error("batch {batch} owns records cited by transactions it did not derive: {}", ids(.transactions))]
    BatchRecordsCited {
        batch: BatchId,
        transactions: Vec<TransactionId>,
    },
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),
}

/// Row ids as an error message names them: a refusal that names nothing a caller can look up is
/// a refusal they cannot act on [DOM-119].
fn ids(transactions: &[TransactionId]) -> String {
    transactions
        .iter()
        .map(|id| id.get().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// An open database: a connection pool whose schema is already current.
#[derive(Debug, Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Opens `path`, creating the file if it is not there yet [ARC-014], and applies every
    /// migration the binary carries [ARC-012].
    ///
    /// Foreign keys are switched on explicitly: SQLite leaves them off per connection by
    /// default, so a schema that declares them would not enforce them.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePool::connect_with(options).await?;
        MIGRATOR.run(&pool).await?;
        Ok(Self { pool })
    }

    #[must_use]
    pub fn accounts(&self) -> AccountRepository<'_> {
        AccountRepository::new(&self.pool)
    }

    #[must_use]
    pub fn securities(&self) -> SecurityRepository<'_> {
        SecurityRepository::new(&self.pool)
    }

    #[must_use]
    pub fn source_records(&self) -> SourceRecordRepository<'_> {
        SourceRecordRepository::new(&self.pool)
    }

    #[must_use]
    pub fn import_batches(&self) -> ImportBatchRepository<'_> {
        ImportBatchRepository::new(&self.pool)
    }

    /// The cached ECB rate table [ARC-015]; seeding it is [`crate::ecb`]'s.
    #[must_use]
    pub fn rates(&self) -> RateRepository<'_> {
        RateRepository::new(&self.pool)
    }

    #[must_use]
    pub fn transactions(&self) -> TransactionRepository<'_> {
        TransactionRepository::new(&self.pool)
    }

    /// The approved attributions and their allocations [DOM-054], [DOM-058].
    #[must_use]
    pub fn attributions(&self) -> AttributionRepository<'_> {
        AttributionRepository::new(&self.pool)
    }

    #[must_use]
    pub fn manual_entries(&self) -> ManualEntryRepository<'_> {
        ManualEntryRepository::new(&self.pool)
    }

    /// Opens a SQLite transaction for a service that reads and writes as one unit, as approving
    /// an attribution does. SQLite serializes it against every other connection's writes: its
    /// reads see one snapshot, and a write after another connection committed since fails as
    /// busy rather than building on rows that have changed.
    pub(crate) async fn begin(
        &self,
    ) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>, StorageError> {
        Ok(self.pool.begin().await?)
    }

    /// Closes the pool. Dropping it works too; this waits for the connections to go.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

/// Defines a row id: a surrogate key handed back by an insert, distinct per table so one cannot
/// be passed where another is meant.
macro_rules! row_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(i64);

        impl $name {
            /// An id as an insert returned it. Public because a caller — the service layer, a
            /// request — holds these as plain integers; a row that is not there reads back as
            /// `None` rather than as an error, so an invented id is answerable.
            #[must_use]
            pub fn new(value: i64) -> Self {
                Self(value)
            }

            #[must_use]
            pub fn get(self) -> i64 {
                self.0
            }
        }

        /// So a refusal can name the row it is about [DOM-119].
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

pub(crate) use row_id;
