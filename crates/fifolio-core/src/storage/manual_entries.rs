//! The manual entry repository.
//!
//! The entry's account and security are columns; the records it answers are their broker
//! identities in a side table, never foreign keys, so an import undo that removes those records
//! leaves the entry intact [DOM-099], [DOM-100].

use std::num::NonZeroU32;

use sqlx::sqlite::{SqlitePool, SqliteRow};
use sqlx::{Row, query};

use crate::entities::{Isin, RecordIdentity};
use crate::manual_entry::{Election, ManualEntry, Ratio, Supplied};
use crate::storage::codec::{at_scale, quantity as read_quantity};
use crate::storage::{StorageError, row_id};

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
