//! Importing a file into an account: reading it through [`import::import`] and storing what it
//! yields, as one unit [SRV-012].
//!
//! # What is stored
//!
//! One import batch, recording the file's classification counts [DOM-017]; every stored row's
//! source record that is not stored already; and every security the stored rows name that is not
//! stored already, auto-created and flagged for review [SRV-014], [DOM-006], [DOM-126]. A
//! non-position row leaves nothing [SRV-016], and a refused file leaves nothing at all, since the
//! refusal comes before the first write [SRV-058].
//!
//! # Posting a file again
//!
//! Records are matched on their identity, which is already scoped to the account [DOM-022],
//! [DOM-024], so a file posted again writes no record and no security, and changes none that is
//! stored [SRV-015]. It does write a batch of its own: every import creates one [SRV-019], and
//! the newer batch is what SRV-052 later moves ownership to (FIF-071). Until then that batch owns
//! nothing (DEC-111, provisional).
//!
//! # What is not checked here
//!
//! Holdings. A sell exceeding what the account holds is stored like any other row; it surfaces
//! when the closing is attributed and its allocations cannot cover it [SRV-018], [DOM-065].
//!
//! Nothing here asks for an FX rate: no transaction is derived on import yet, so no leg is valued.

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::entities::{Account, ImportBatch, Isin};
use crate::import::{self, Import, ImportError, Importer};
use crate::storage::{
    AccountRepository, BatchId, Database, ImportBatchRepository, SecurityRepository,
    SourceRecordRepository, StorageError,
};

/// Why a file was not imported.
#[derive(Debug, Error)]
pub enum ImportFileError {
    /// The file was unreadable or refused [SRV-058], [SRV-059].
    #[error(transparent)]
    Import(#[from] ImportError),
    /// The target account is not stored, or the database failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// What an import stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    batch: BatchId,
    import: Import,
    created: Vec<Isin>,
}

impl Imported {
    /// The batch this import created [SRV-019].
    #[must_use]
    pub fn batch(&self) -> BatchId {
        self.batch
    }

    /// What the file yielded, whether or not its records were stored before.
    #[must_use]
    pub fn import(&self) -> &Import {
        &self.import
    }

    /// The securities this import created, in the order the file first names them: those of
    /// [`Import::securities`] that were not stored before [SRV-014].
    #[must_use]
    pub fn created(&self) -> &[Isin] {
        &self.created
    }
}

/// Reads `content` as `importer`'s format into `account` and stores it, or stores nothing.
///
/// `Sync` so that a server can hold the importer across the database's awaits.
///
/// # Errors
///
/// [`StorageError::UnknownAccount`] when `account` is not stored, checked before the file is
/// read [SRV-012]; [`ImportFileError::Import`] when the file is unreadable or refused, which
/// stores nothing [SRV-058]; and a storage failure.
pub async fn import_file(
    database: &Database,
    importer: &(dyn Importer + Sync),
    account: &Account,
    filename: &str,
    content: &[u8],
    imported_at: DateTime<Utc>,
) -> Result<Imported, ImportFileError> {
    let mut tx = database.begin().await?;
    if AccountRepository::find_in(&mut tx, account.broker(), account.id())
        .await?
        .is_none()
    {
        return Err(StorageError::UnknownAccount {
            broker: account.broker().to_owned(),
            id: account.id().to_owned(),
        }
        .into());
    }

    let import = import::import(importer, account, content)?;

    let mut created = Vec::new();
    for security in import.securities() {
        if SecurityRepository::insert_if_absent_in(&mut tx, security).await? {
            created.push(security.isin().clone());
        }
    }
    let batch = ImportBatchRepository::insert_in(
        &mut tx,
        &ImportBatch::new(
            account.clone(),
            filename,
            import.format(),
            imported_at,
            import.counts(),
        ),
    )
    .await?;
    for stored in import.stored() {
        SourceRecordRepository::insert_if_absent_in(&mut tx, batch, stored.record()).await?;
    }
    tx.commit().await.map_err(StorageError::from)?;

    Ok(Imported {
        batch,
        import,
        created,
    })
}
