//! Repositories for the reference entities: accounts, securities, source records and import
//! batches.

use std::collections::BTreeMap;

use sqlx::sqlite::{SqliteConnection, SqlitePool, SqliteRow};
use sqlx::{Row, query};

use crate::entities::{
    Account, ImportBatch, ImportCounts, Isin, Order, Quotation, RecordIdentity, Security,
    SecurityType, SourceRecord,
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

    /// An account already stored is refused as [`StorageError::DuplicateAccount`] rather than
    /// as a driver error.
    pub async fn insert(&self, account: &Account) -> Result<(), StorageError> {
        query("insert into account (broker, id) values (?, ?)")
            .bind(account.broker())
            .bind(account.id())
            .execute(self.pool)
            .await
            .map_err(|error| duplicate_account(error, account))?;
        Ok(())
    }

    /// Every account, in key order.
    pub async fn list(&self) -> Result<Vec<Account>, StorageError> {
        Ok(query("select broker, id from account order by broker, id")
            .fetch_all(self.pool)
            .await?
            .iter()
            .map(|row| account_from_row(row, "broker", "id"))
            .collect())
    }

    /// Gives `from` the key `to`, refused while anything refers to it.
    ///
    /// An account is nothing but its key, so this is the only edit it has. It is refused on the
    /// same grounds as a deletion because a stored record's identity is scoped to the account
    /// it was imported into [DOM-024]: renaming under it would make the next import of the
    /// same file miss every record it already holds.
    pub async fn rename(&self, from: &Account, to: &Account) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        refuse_if_account_referenced(&mut tx, from).await?;
        query("update account set broker = ?, id = ? where broker = ? and id = ?")
            .bind(to.broker())
            .bind(to.id())
            .bind(from.broker())
            .bind(from.id())
            .execute(&mut *tx)
            .await
            .map_err(|error| duplicate_account(error, to))?;
        tx.commit().await?;
        Ok(())
    }

    /// Deletes an account, refused while any source record was imported into it [SRV-008].
    ///
    /// Refused too while a batch, a manual entry or a transaction names it: those carry a
    /// foreign key to the account, and the alternative to refusing is deleting them with it.
    pub async fn delete(&self, account: &Account) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        refuse_if_account_referenced(&mut tx, account).await?;
        query("delete from account where broker = ? and id = ?")
            .bind(account.broker())
            .bind(account.id())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
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

fn duplicate_account(error: sqlx::Error, account: &Account) -> StorageError {
    match &error {
        sqlx::Error::Database(database) if database.is_unique_violation() => {
            StorageError::DuplicateAccount {
                broker: account.broker().to_owned(),
                id: account.id().to_owned(),
            }
        }
        _ => StorageError::Database(error),
    }
}

fn unknown_account(account: &Account) -> StorageError {
    StorageError::UnknownAccount {
        broker: account.broker().to_owned(),
        id: account.id().to_owned(),
    }
}

/// Refuses an account that is not stored, or that anything stored refers to [SRV-008].
///
/// A source record reaches its account through the batch that owns it, which is the account
/// it was imported into.
async fn refuse_if_account_referenced(
    connection: &mut SqliteConnection,
    account: &Account,
) -> Result<(), StorageError> {
    let row = query(
        "select
             (select count(*) from account where broker = ?1 and id = ?2) as stored,
             (select count(*) from source_record r join import_batch b on b.id = r.batch_id
               where b.account_broker = ?1 and b.account_id = ?2) as source_records,
             (select count(*) from import_batch
               where account_broker = ?1 and account_id = ?2) as batches,
             (select count(*) from manual_entry
               where account_broker = ?1 and account_id = ?2) as manual_entries,
             (select count(*) from transaction_placement
               where account_broker = ?1 and account_id = ?2) as transactions",
    )
    .bind(account.broker())
    .bind(account.id())
    .fetch_one(connection)
    .await?;

    if row.get::<i64, _>("stored") == 0 {
        return Err(unknown_account(account));
    }
    let references = (
        reference_count(&row, "source_records")?,
        reference_count(&row, "batches")?,
        reference_count(&row, "manual_entries")?,
        reference_count(&row, "transactions")?,
    );
    match references {
        (0, 0, 0, 0) => Ok(()),
        (source_records, batches, manual_entries, transactions) => {
            Err(StorageError::AccountReferenced {
                broker: account.broker().to_owned(),
                id: account.id().to_owned(),
                source_records,
                batches,
                manual_entries,
                transactions,
            })
        }
    }
}

/// `count(*)` is never negative; a negative one is not a database this crate wrote.
fn reference_count(row: &SqliteRow, field: &'static str) -> Result<u64, StorageError> {
    let stored = row.get::<i64, _>(field);
    u64::try_from(stored).map_err(|_| StorageError::CorruptValue {
        field,
        value: stored.to_string(),
    })
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
        query(
            "select isin, name, security_type, quotation, auto_created
             from security where isin = ?",
        )
        .bind(isin.as_str())
        .fetch_optional(self.pool)
        .await?
        .as_ref()
        .map(security_from_row)
        .transpose()
    }

    /// Every security, in ISIN order.
    pub async fn list(&self) -> Result<Vec<Security>, StorageError> {
        query(
            "select isin, name, security_type, quotation, auto_created
             from security order by isin",
        )
        .fetch_all(self.pool)
        .await?
        .iter()
        .map(security_from_row)
        .collect()
    }

    /// Replaces what a security says about itself other than its ISIN, and returns it as stored.
    ///
    /// Type and quotation are editable so that an auto-created record can be corrected
    /// [SRV-011], and each moves without the other [DOM-037]. The auto-created flag is left
    /// alone: it records where the security came from, not whether anyone has looked at it
    /// [DOM-006].
    pub async fn update(
        &self,
        isin: &Isin,
        name: &str,
        security_type: SecurityType,
        quotation: Quotation,
    ) -> Result<Security, StorageError> {
        query(
            "update security set name = ?, security_type = ?, quotation = ? where isin = ?
             returning isin, name, security_type, quotation, auto_created",
        )
        .bind(name)
        .bind(security_type_code(security_type))
        .bind(quotation_code(quotation))
        .bind(isin.as_str())
        .fetch_optional(self.pool)
        .await?
        .as_ref()
        .map(security_from_row)
        .transpose()?
        .ok_or_else(|| unknown_security(isin))
    }

    /// Deletes a security, refused while any source record names it [SRV-009], or any
    /// transaction is placed on it, which the foreign key would refuse anyway.
    ///
    /// A source record has no column for its security: what it holds is the file's own fields,
    /// under the format's own names — `Instrument ISIN` in a Saxo row, `symbol` in a Trade
    /// Republic one. So a record names the security when any of its parsed values is the ISIN.
    /// That also catches a record naming it in a second role, as a corporate action's target
    /// does, and it needs no list of column names that a new format would have to extend.
    pub async fn delete(&self, isin: &Isin) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        // Values are normalized as `Isin::new` normalizes, so a lowercase or padded spelling
        // in a file still counts as naming the security.
        let row = query(
            "select
                 (select count(*) from security where isin = ?1) as stored,
                 (select count(*) from source_record r
                   where exists (select 1 from json_each(r.parsed)
                                  where upper(trim(json_each.value)) = ?1)) as source_records,
                 (select count(*) from transaction_placement
                   where security_isin = ?1) as transactions",
        )
        .bind(isin.as_str())
        .fetch_one(&mut *tx)
        .await?;

        if row.get::<i64, _>("stored") == 0 {
            return Err(unknown_security(isin));
        }
        match (
            reference_count(&row, "source_records")?,
            reference_count(&row, "transactions")?,
        ) {
            (0, 0) => {}
            (source_records, transactions) => {
                return Err(StorageError::SecurityReferenced {
                    isin: isin.as_str().to_owned(),
                    source_records,
                    transactions,
                });
            }
        }

        query("delete from security where isin = ?")
            .bind(isin.as_str())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

fn security_from_row(row: &SqliteRow) -> Result<Security, StorageError> {
    let isin = Isin::new(row.get::<String, _>("isin"));
    let name = row.get::<String, _>("name");
    let security_type = security_type(&row.get::<String, _>("security_type"))?;
    let quotation = quotation(&row.get::<String, _>("quotation"))?;

    Ok(if row.get::<bool, _>("auto_created") {
        Security::auto_created(isin, name, security_type, quotation)
    } else {
        Security::new(isin, name, security_type, quotation)
    })
}

fn unknown_security(isin: &Isin) -> StorageError {
    StorageError::UnknownSecurity {
        isin: isin.as_str().to_owned(),
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
///
/// An emitted `transfer_in` has no batch of its own and cites its `transfer_out`'s records
/// [DEC-079]; it counts as derived by the batch that derived its `transfer_out`, since deleting
/// that `transfer_out` takes it along [DOM-094], [DEC-086]. A transaction with neither a batch nor
/// an emitter stays foreign.
async fn foreign_citations(
    connection: &mut SqliteConnection,
    batch: BatchId,
) -> Result<Vec<TransactionId>, StorageError> {
    Ok(query(
        "select distinct c.transaction_id from transaction_citation c
              join source_record r on r.identity = c.record_identity
              left join transaction_placement p on p.transaction_id = c.transaction_id
              left join emitted_transfer_in e on e.transfer_in_id = c.transaction_id
              left join transaction_placement emitter on emitter.transaction_id = e.transfer_out_id
         where r.batch_id = ?
           and coalesce(p.derived_by_batch, emitter.derived_by_batch) is not ?
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
