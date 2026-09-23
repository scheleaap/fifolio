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
//! [ARC-015]. No column exists for a rule that is still undecided — a
//! transaction has no relation to an account, a security or a source record, that being DOM-013
//! and FIF-076's — because a guessed column is a schema that must be unpicked rather than
//! extended.
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
//! # Why every repository is insert-and-read-back
//!
//! Deletion, listing and the invariants that refuse a write are FIF-012's and later; this item
//! owns the file, the migrations and the mapping. Adding a method here that no requirement asks
//! for would be guessing at the query the service layer will want.

mod codec;
mod entities;
mod manual_entries;
mod rates;
mod transactions;

use std::path::Path;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};

pub use entities::{
    AccountRepository, BatchId, ImportBatchRepository, SecurityRepository, SourceRecordRepository,
};
pub use manual_entries::{ManualEntryId, ManualEntryRepository};
pub use rates::{CachedRates, RateRepository};
pub use transactions::{TransactionId, TransactionRepository};

/// The database file used when nothing names another [ARC-013].
///
/// A constant rather than a default argument: the binary that reads `--database` is the one
/// place a default belongs, and it passes whichever path it ended up with to [`Database::open`].
pub const DEFAULT_DATABASE_PATH: &str = "./fifolio.db";

/// The versioned migrations, embedded in the binary so that a deployed process carries its own
/// schema history [ARC-012].
static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Anything storage refuses or cannot make sense of.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// A security's ISIN is unique [DOM-071].
    #[error("a security with ISIN {isin} already exists")]
    DuplicateIsin { isin: String },
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
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),
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

    #[must_use]
    pub fn manual_entries(&self) -> ManualEntryRepository<'_> {
        ManualEntryRepository::new(&self.pool)
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
    };
}

pub(crate) use row_id;
