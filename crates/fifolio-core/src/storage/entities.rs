//! Repositories for the reference entities: accounts, securities, source records and import
//! batches.

use std::collections::BTreeMap;

use sqlx::sqlite::{SqliteConnection, SqlitePool, SqliteRow};
use sqlx::{Row, query};

use crate::entities::{
    Account, ImportBatch, ImportCounts, Isin, Order, RecordIdentity, Security, SourceRecord,
};
use crate::storage::codec::{
    quotation, quotation_code, security_type, security_type_code, source_format, source_format_code,
};
use crate::storage::transactions::{TransactionId, delete_transactions, participating_attribution};
use crate::storage::{StorageError, row_id};

row_id!(
    /// An import batch's key. Surrogate because a batch has no natural one: the same file may be
    /// imported into the same account twice.
    BatchId
);

/// A count as SQLite holds it. The counts are `u32`, and a negative or oversized integer in the
/// column is a database someone edited, not a state this crate can produce.
fn count(field: &'static str, stored: i64) -> Result<u32, StorageError> {
    u32::try_from(stored).map_err(|_| StorageError::CorruptValue {
        field,
        value: stored.to_string(),
    })
}

/// Accounts, keyed by broker and the broker's own id.
pub struct AccountRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> AccountRepository<'a> {
    pub(super) fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, account: &Account) -> Result<(), StorageError> {
        query("insert into account (broker, id) values (?, ?)")
            .bind(account.broker())
            .bind(account.id())
            .execute(self.pool)
            .await?;
        Ok(())
    }

    pub async fn find(&self, broker: &str, id: &str) -> Result<Option<Account>, StorageError> {
        let row = query("select broker, id from account where broker = ? and id = ?")
            .bind(broker)
            .bind(id)
            .fetch_optional(self.pool)
            .await?;
        Ok(row.map(|row| account_from_row(&row, "broker", "id")))
    }
}

fn account_from_row(row: &SqliteRow, broker: &str, id: &str) -> Account {
    Account::new(row.get::<String, _>(broker), row.get::<String, _>(id))
}

/// Securities, keyed by ISIN [DOM-071].
pub struct SecurityRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> SecurityRepository<'a> {
    pub(super) fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// A second security with an ISIN already stored is refused by the primary key, which this
    /// reports as [`StorageError::DuplicateIsin`] rather than as a driver error [DOM-071].
    pub async fn insert(&self, security: &Security) -> Result<(), StorageError> {
        query(
            "insert into security (isin, name, security_type, quotation, auto_created)
             values (?, ?, ?, ?, ?)",
        )
        .bind(security.isin().as_str())
        .bind(security.name())
        .bind(security_type_code(security.security_type()))
        .bind(quotation_code(security.quotation()))
        .bind(security.is_auto_created())
        .execute(self.pool)
        .await
        .map_err(|error| match &error {
            sqlx::Error::Database(database) if database.is_unique_violation() => {
                StorageError::DuplicateIsin {
                    isin: security.isin().as_str().to_owned(),
                }
            }
            _ => StorageError::Database(error),
        })?;
        Ok(())
    }

    pub async fn find(&self, isin: &Isin) -> Result<Option<Security>, StorageError> {
        let Some(row) = query(
            "select isin, name, security_type, quotation, auto_created
             from security where isin = ?",
        )
        .bind(isin.as_str())
        .fetch_optional(self.pool)
        .await?
        else {
            return Ok(None);
        };

        let isin = Isin::new(row.get::<String, _>("isin"));
        let name = row.get::<String, _>("name");
        let security_type = security_type(&row.get::<String, _>("security_type"))?;
        let quotation = quotation(&row.get::<String, _>("quotation"))?;

        Ok(Some(if row.get::<bool, _>("auto_created") {
            Security::auto_created(isin, name, security_type, quotation)
        } else {
            Security::new(isin, name, security_type, quotation)
        }))
    }
}

/// Source records, keyed by the identity that already carries their account scoping [DOM-024].
pub struct SourceRecordRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> SourceRecordRepository<'a> {
    pub(super) fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Stores `record` as owned by `batch`, which is what "the records it owns" means when a
    /// deletion of that batch is refused [DOM-119] or carried out.
    pub async fn insert(&self, batch: BatchId, record: &SourceRecord) -> Result<(), StorageError> {
        let parsed =
            serde_json::to_string(record.parsed()).map_err(|_| StorageError::CorruptValue {
                field: "parsed",
                value: String::new(),
            })?;

        query(
            "insert into source_record (identity, ordering, raw, parsed, batch_id)
             values (?, ?, ?, ?, ?)",
        )
        .bind(record.identity().as_str())
        .bind(i64::from(record.order().get()))
        .bind(record.raw())
        .bind(parsed)
        .bind(batch.get())
        .execute(self.pool)
        .await?;
        Ok(())
    }

    pub async fn find(
        &self,
        identity: &RecordIdentity,
    ) -> Result<Option<SourceRecord>, StorageError> {
        let Some(row) =
            query("select identity, ordering, raw, parsed from source_record where identity = ?")
                .bind(identity.as_str())
                .fetch_optional(self.pool)
                .await?
        else {
            return Ok(None);
        };

        let parsed = row.get::<String, _>("parsed");
        let parsed: BTreeMap<String, String> =
            serde_json::from_str(&parsed).map_err(|_| StorageError::CorruptValue {
                field: "parsed",
                value: parsed.clone(),
            })?;

        Ok(Some(SourceRecord::new(
            RecordIdentity::new(row.get::<String, _>("identity")),
            Order::new(count("ordering", row.get::<i64, _>("ordering"))?),
            row.get::<String, _>("raw"),
            parsed,
        )))
    }
}

/// Import batches, keyed by a surrogate id [DOM-017].
pub struct ImportBatchRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> ImportBatchRepository<'a> {
    pub(super) fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, batch: &ImportBatch) -> Result<BatchId, StorageError> {
        let counts = batch.counts();
        let inserted = query(
            "insert into import_batch
                 (account_broker, account_id, filename, format, imported_at,
                  derived, pending, non_position, failed)
             values (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(batch.account().broker())
        .bind(batch.account().id())
        .bind(batch.filename())
        .bind(source_format_code(batch.format()))
        .bind(batch.imported_at())
        .bind(i64::from(counts.derived))
        .bind(i64::from(counts.pending))
        .bind(i64::from(counts.non_position))
        .bind(i64::from(counts.failed))
        .execute(self.pool)
        .await?;

        Ok(BatchId::new(inserted.last_insert_rowid()))
    }

    /// Deletes a batch with the records it owns and the transactions it derived, refusing it in
    /// two cases.
    ///
    /// Refused while any transaction the batch derived participates in an attribution [DOM-072],
    /// and while any record it owns is cited by a transaction it did not derive, the refusal
    /// naming those transactions so that the user can see what holds the batch in place
    /// [DOM-119].
    ///
    /// What it does **not** touch is a manual entry: none belongs to a batch, and the records an
    /// entry answers are broker identities rather than foreign keys, so an undo has nothing of it
    /// to remove [DOM-110], [DOM-099]. That is a property of the schema, not a case below.
    pub async fn delete(&self, batch: BatchId) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;

        let derived = derived_transactions(&mut tx, batch).await?;

        let attributed = attributed_of(&mut tx, &derived).await?;
        if !attributed.is_empty() {
            return Err(StorageError::BatchTransactionAttributed {
                batch,
                transactions: attributed,
            });
        }

        let citing = foreign_citations(&mut tx, batch).await?;
        if !citing.is_empty() {
            return Err(StorageError::BatchRecordsCited {
                batch,
                transactions: citing,
            });
        }

        delete_transactions(&mut tx, &derived).await?;
        query("delete from source_record where batch_id = ?")
            .bind(batch.get())
            .execute(&mut *tx)
            .await?;
        query("delete from import_batch where id = ?")
            .bind(batch.get())
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn find(&self, id: BatchId) -> Result<Option<ImportBatch>, StorageError> {
        let Some(row) = query(
            "select account_broker, account_id, filename, format, imported_at,
                    derived, pending, non_position, failed
             from import_batch where id = ?",
        )
        .bind(id.get())
        .fetch_optional(self.pool)
        .await?
        else {
            return Ok(None);
        };

        let counts = ImportCounts {
            derived: count("derived", row.get::<i64, _>("derived"))?,
            pending: count("pending", row.get::<i64, _>("pending"))?,
            non_position: count("non_position", row.get::<i64, _>("non_position"))?,
            failed: count("failed", row.get::<i64, _>("failed"))?,
        };

        Ok(Some(ImportBatch::new(
            account_from_row(&row, "account_broker", "account_id"),
            row.get::<String, _>("filename"),
            source_format(&row.get::<String, _>("format"))?,
            row.get("imported_at"),
            counts,
        )))
    }
}

/// The transactions `batch` derived [DOM-072].
async fn derived_transactions(
    connection: &mut SqliteConnection,
    batch: BatchId,
) -> Result<Vec<TransactionId>, StorageError> {
    Ok(query(
        "select transaction_id from transaction_placement
         where derived_by_batch = ? order by transaction_id",
    )
    .bind(batch.get())
    .fetch_all(connection)
    .await?
    .iter()
    .map(|row| TransactionId::new(row.get("transaction_id")))
    .collect())
}

/// Those of `transactions` that participate in an attribution [DOM-072].
async fn attributed_of(
    connection: &mut SqliteConnection,
    transactions: &[TransactionId],
) -> Result<Vec<TransactionId>, StorageError> {
    let mut attributed = Vec::new();
    for &transaction in transactions {
        if participating_attribution(connection, transaction)
            .await?
            .is_some()
        {
            attributed.push(transaction);
        }
    }
    Ok(attributed)
}

/// The transactions that cite a record `batch` owns without having been derived by it [DOM-119].
///
/// A transaction citing another import's record is the audit trail of a multi-file event, and
/// deleting the record underneath it would leave it citing nothing.
async fn foreign_citations(
    connection: &mut SqliteConnection,
    batch: BatchId,
) -> Result<Vec<TransactionId>, StorageError> {
    Ok(query(
        "select distinct c.transaction_id from transaction_citation c
              join source_record r on r.identity = c.record_identity
              left join transaction_placement p on p.transaction_id = c.transaction_id
         where r.batch_id = ?
           and (p.derived_by_batch is null or p.derived_by_batch <> ?)
         order by c.transaction_id",
    )
    .bind(batch.get())
    .bind(batch.get())
    .fetch_all(connection)
    .await?
    .iter()
    .map(|row| TransactionId::new(row.get("transaction_id")))
    .collect())
}
