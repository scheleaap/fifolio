//! Repositories for the reference entities: accounts, securities, source records and import
//! batches.

use std::collections::BTreeMap;

use sqlx::sqlite::{SqlitePool, SqliteRow};
use sqlx::{Row, query};

use crate::entities::{
    Account, ImportBatch, ImportCounts, Isin, Order, RecordIdentity, Security, SourceRecord,
};
use crate::storage::codec::{
    quotation, quotation_code, security_type, security_type_code, source_format, source_format_code,
};
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

    pub async fn insert(&self, record: &SourceRecord) -> Result<(), StorageError> {
        let parsed =
            serde_json::to_string(record.parsed()).map_err(|_| StorageError::CorruptValue {
                field: "parsed",
                value: String::new(),
            })?;

        query("insert into source_record (identity, ordering, raw, parsed) values (?, ?, ?, ?)")
            .bind(record.identity().as_str())
            .bind(i64::from(record.order().get()))
            .bind(record.raw())
            .bind(parsed)
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
