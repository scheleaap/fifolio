//! The attribution repository: what the user approved, and the two invariants that order it.
//!
//! An attribution is one closing together with the allocations against the openings it consumes
//! [DOM-054]. Only the opening, the closing and the quantity are stored [DOM-058]; every monetary
//! figure is derived from the parent transactions on demand, so nothing here can drift away from
//! them, and deriving it is FIF-014's.
//!
//! # The order two of the invariants read
//!
//! DOM-066 and DOM-068 are stated over "earlier" and "later" closings of the same account and
//! security, which is the canonical order — (trade date, `order`, batch age) [DOM-111]. That
//! order is undecided (OQ-007), and the `order` a transaction takes from the record it consumes
//! is undecided too (OQ-001), so neither is available to read. The stand-in used here is
//! **(trade date, row id)**: it agrees with the canonical order on the trade date, which is the
//! key both invariants are about, and settles a tie by the order the rows were written rather
//! than by a rule this item is not entitled to invent. When FIF-076 lands, this comparison is the
//! one place that changes.

use chrono::NaiveDate;
use sqlx::sqlite::SqliteRow;
use sqlx::sqlite::{SqliteArguments, SqliteConnection, SqlitePool};
use sqlx::{Row, Sqlite, query};

use crate::decimal::Quantity;
use crate::storage::codec::{at_scale, quantity as read_quantity};
use crate::storage::transactions::{TransactionId, is_closing};
use crate::storage::{StorageError, row_id};

row_id!(
    /// An attribution's key.
    AttributionId
);

/// One opening and the quantity of it the closing consumed [DOM-058].
///
/// The figures the allocation implies — cost, buy fee, proceeds, sell fee, gain — are computed
/// from the parent transactions [DOM-059] and are deliberately not fields here: a stored figure
/// is one that can disagree with its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allocation {
    opening: TransactionId,
    quantity: Quantity,
}

impl Allocation {
    #[must_use]
    pub fn new(opening: TransactionId, quantity: Quantity) -> Self {
        Self { opening, quantity }
    }

    #[must_use]
    pub fn opening(&self) -> TransactionId {
        self.opening
    }

    #[must_use]
    pub fn quantity(&self) -> Quantity {
        self.quantity
    }
}

/// An approved attribution as it was stored: the closing, and its allocations in the order they
/// were approved in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribution {
    closing: TransactionId,
    allocations: Vec<Allocation>,
}

impl Attribution {
    #[must_use]
    pub fn closing(&self) -> TransactionId {
        self.closing
    }

    #[must_use]
    pub fn allocations(&self) -> &[Allocation] {
        &self.allocations
    }
}

/// A transaction's position: the pair the invariants key on, its kind, and where it sits in the
/// order.
struct Position {
    broker: String,
    account: String,
    isin: String,
    kind: String,
    trade_date: NaiveDate,
    id: i64,
}

impl Position {
    /// Reads the position of `transaction`, or refuses if no such transaction is stored.
    async fn of(
        connection: &mut SqliteConnection,
        transaction: TransactionId,
    ) -> Result<Self, StorageError> {
        let row = query(
            "select p.account_broker, p.account_id, p.security_isin, t.kind, t.trade_date
             from transaction_placement p
                  join transaction_record t on t.id = p.transaction_id
             where p.transaction_id = ?",
        )
        .bind(transaction.get())
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(StorageError::UnknownTransaction { transaction })?;

        Ok(Self {
            broker: row.get("account_broker"),
            account: row.get("account_id"),
            isin: row.get("security_isin"),
            kind: row.get("kind"),
            trade_date: row.get("trade_date"),
            id: transaction.get(),
        })
    }
}

pub struct AttributionRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> AttributionRepository<'a> {
    pub(super) fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Stores the attribution of `closing` to `allocations`, refusing it while any earlier
    /// closing of the same account and security is unattributed [DOM-066].
    ///
    /// Refused too when `closing` is not a closing at all, or already carries an attribution:
    /// both invariants are stated over closings, and an attributed opening would be counted as
    /// a "later attribution" by a lawful DOM-068 deletion and freeze the opening under DOM-069.
    ///
    /// Check and write share one SQLite transaction, so an earlier closing cannot slip in
    /// between them.
    pub async fn approve(
        &self,
        closing: TransactionId,
        allocations: &[Allocation],
    ) -> Result<AttributionId, StorageError> {
        // Every quantity is scaled before anything is written, so a refusal at the last
        // allocation leaves no half-stored attribution [ARC-010].
        let quantities = allocations
            .iter()
            .map(|allocation| at_scale("quantity", allocation.quantity()))
            .collect::<Result<Vec<_>, _>>()?;

        let mut tx = self.pool.begin().await?;
        let position = Position::of(&mut tx, closing).await?;

        if !is_closing(&position.kind) {
            return Err(StorageError::NotAClosing {
                transaction: closing,
                kind: position.kind,
            });
        }

        if let Some(existing) = attribution_of(&mut tx, closing).await? {
            return Err(StorageError::ClosingAlreadyAttributed {
                closing,
                attribution: existing,
            });
        }

        if let Some(earlier) = earlier_unattributed_closing(&mut tx, &position).await? {
            return Err(StorageError::EarlierClosingUnattributed { closing, earlier });
        }

        let inserted = query("insert into attribution (closing_transaction_id) values (?)")
            .bind(closing.get())
            .execute(&mut *tx)
            .await?;
        let id = inserted.last_insert_rowid();

        for (ordinal, (allocation, quantity)) in (0i64..).zip(allocations.iter().zip(quantities)) {
            query(
                "insert into attribution_allocation
                     (attribution_id, ordinal, opening_transaction_id, quantity)
                 values (?, ?, ?, ?)",
            )
            .bind(id)
            .bind(ordinal)
            .bind(allocation.opening().get())
            .bind(quantity)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(AttributionId::new(id))
    }

    /// Deletes an attribution, refusing while a later attribution exists for the same account and
    /// security [DOM-068]: deleting out of order would leave a later closing attributed to parcels
    /// an earlier one had not yet been offered.
    pub async fn delete(&self, attribution: AttributionId) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;

        let Some(closing) = closing_of(&mut tx, attribution).await? else {
            // Nothing stored under that id: deleting it is already true.
            return Ok(());
        };
        let position = Position::of(&mut tx, closing).await?;

        if let Some(later) = later_attribution(&mut tx, &position).await? {
            return Err(StorageError::LaterAttributionExists { attribution, later });
        }

        query("delete from attribution where id = ?")
            .bind(attribution.get())
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn find(
        &self,
        attribution: AttributionId,
    ) -> Result<Option<Attribution>, StorageError> {
        let mut connection = self.pool.acquire().await?;
        let Some(closing) = closing_of(&mut connection, attribution).await? else {
            return Ok(None);
        };

        let allocations = query(
            "select opening_transaction_id, quantity from attribution_allocation
             where attribution_id = ? order by ordinal",
        )
        .bind(attribution.get())
        .fetch_all(&mut *connection)
        .await?
        .iter()
        .map(allocation_from_row)
        .collect::<Result<Vec<_>, _>>()?;

        Ok(Some(Attribution {
            closing,
            allocations,
        }))
    }
}

fn allocation_from_row(row: &SqliteRow) -> Result<Allocation, StorageError> {
    Ok(Allocation::new(
        TransactionId::new(row.get("opening_transaction_id")),
        read_quantity("quantity", &row.get::<String, _>("quantity"))?,
    ))
}

async fn closing_of(
    connection: &mut SqliteConnection,
    attribution: AttributionId,
) -> Result<Option<TransactionId>, StorageError> {
    Ok(
        query("select closing_transaction_id from attribution where id = ?")
            .bind(attribution.get())
            .fetch_optional(connection)
            .await?
            .map(|row| TransactionId::new(row.get("closing_transaction_id"))),
    )
}

/// The attribution of `closing`, if it already carries one [DOM-066].
async fn attribution_of(
    connection: &mut SqliteConnection,
    closing: TransactionId,
) -> Result<Option<AttributionId>, StorageError> {
    Ok(
        query("select id from attribution where closing_transaction_id = ?")
            .bind(closing.get())
            .fetch_optional(connection)
            .await?
            .map(|row| AttributionId::new(row.get("id"))),
    )
}

/// The first closing of the same account and security that sits before `position` and carries no
/// attribution [DOM-066].
async fn earlier_unattributed_closing(
    connection: &mut SqliteConnection,
    position: &Position,
) -> Result<Option<TransactionId>, StorageError> {
    // The three closing codes are written out rather than interpolated: sqlx takes a literal
    // statement, and the codes are the ones `transactions.rs` stores [DOM-081].
    let sql = "select t.id from transaction_record t
                    join transaction_placement p on p.transaction_id = t.id
                    left join attribution a on a.closing_transaction_id = t.id
               where p.account_broker = ? and p.account_id = ? and p.security_isin = ?
                 and t.kind in ('sell', 'expiration', 'transfer_out')
                 and (t.trade_date < ? or (t.trade_date = ? and t.id < ?))
                 and a.id is null
               order by t.trade_date, t.id
               limit 1";

    Ok(position
        .bind_to(query(sql))
        .fetch_optional(connection)
        .await?
        .map(|row| TransactionId::new(row.get("id"))))
}

/// The first attribution of the same account and security whose closing sits after `position`
/// [DOM-068].
///
/// The kind filter matches `earlier_unattributed_closing` above: both invariants are stated over
/// closings. It is unreachable through `approve`, which refuses an opening outright, and is kept
/// so that the two halves of the ordering cannot drift apart.
async fn later_attribution(
    connection: &mut SqliteConnection,
    position: &Position,
) -> Result<Option<AttributionId>, StorageError> {
    let sql = "select a.id from attribution a
                    join transaction_record t on t.id = a.closing_transaction_id
                    join transaction_placement p on p.transaction_id = t.id
               where p.account_broker = ? and p.account_id = ? and p.security_isin = ?
                 and t.kind in ('sell', 'expiration', 'transfer_out')
                 and (t.trade_date > ? or (t.trade_date = ? and t.id > ?))
               order by t.trade_date, t.id
               limit 1";

    Ok(position
        .bind_to(query(sql))
        .fetch_optional(connection)
        .await?
        .map(|row| AttributionId::new(row.get("id"))))
}

impl Position {
    /// Binds the six parameters both comparisons above take, in their one order.
    fn bind_to<'q>(
        &'q self,
        query: sqlx::query::Query<'q, Sqlite, SqliteArguments>,
    ) -> sqlx::query::Query<'q, Sqlite, SqliteArguments> {
        query
            .bind(&self.broker)
            .bind(&self.account)
            .bind(&self.isin)
            .bind(self.trade_date)
            .bind(self.trade_date)
            .bind(self.id)
    }
}
