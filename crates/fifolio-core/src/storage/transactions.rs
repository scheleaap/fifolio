//! The transaction repository: a header row per transaction, the fields of its variant in a
//! table of that variant's own, and its citations in a third.
//!
//! The variant tables are what keeps [DOM-010] true on disk: there is no nullable column
//! standing in for "not applicable here", so a row of one variant cannot be read as another.
//! Header, detail and citations are written in one SQLite transaction, so a transaction is
//! never half stored.

use chrono::NaiveDate;
use sqlx::sqlite::{SqliteConnection, SqlitePool, SqliteRow};
use sqlx::{Row, query};
use vec1::Vec1;

use crate::entities::{Account, Isin, Order, RecordIdentity};
use crate::ordering::{BatchAge, Leg, OrderKey, RecordPosition};
use crate::storage::codec::{
    at_scale, buy_origin, buy_origin_code, date_provenance, date_provenance_code, money_pair,
    pair_at_scale, price_pair, quantity as read_quantity, rate, rate_source, rate_source_code,
    transfer_in_source, transfer_in_source_code,
};
use crate::storage::manual_entries::ratio;
use crate::storage::{AttributionId, BatchId, RecordHandle, StorageError, row_id};
use crate::transaction::{
    Buy, Closing, Derivation, Expiration, Opening, Sell, Split, Transaction, TransferIn,
    TransferOut,
};
use crate::valuation::{Conversion, Currency};

row_id!(
    /// A transaction's key. Surrogate: the natural key would be the source record it consumes,
    /// and which record that is, as against one it only cites, is DOM-101's.
    TransactionId
);

/// The stored code of each variant, and the detail table that holds its fields.
const BUY: &str = "buy";
const TRANSFER_IN: &str = "transfer_in";
const SELL: &str = "sell";
const EXPIRATION: &str = "expiration";
const TRANSFER_OUT: &str = "transfer_out";
const SPLIT: &str = "split";

/// Whether a stored kind is one of the three that close parcels [DOM-081].
///
/// The invariants DOM-066 and DOM-068 are stated over closings, so the attribution repository
/// asks this before it stores or orders anything; the codes live here, with the inserts that
/// write them.
pub(super) fn is_closing(kind: &str) -> bool {
    matches!(kind, SELL | EXPIRATION | TRANSFER_OUT)
}

/// Where a transaction belongs: the account and security it is a transaction of, and the import
/// that derived it.
///
/// A parameter of the insert rather than a field of [`Transaction`]: these are the transaction's
/// relations to account and security [DOM-013], and like a source record's relation to its batch
/// they are held by storage rather than by the value. The relation to source records is the
/// transaction's citations. What is stored here is the pair DOM-066 and DOM-068 key on, and the
/// batch DOM-119 asks about; DOM-072 reads the records instead. Which of its records a
/// transaction consumes rather than cites is DOM-101's (FIF-058).
#[derive(Debug, Clone)]
pub struct Placement {
    account: Account,
    security: Isin,
    derived_by: Option<BatchId>,
}

impl Placement {
    /// A transaction an import derived.
    #[must_use]
    pub fn derived(account: Account, security: Isin, batch: BatchId) -> Self {
        Self {
            account,
            security,
            derived_by: Some(batch),
        }
    }

    /// A transaction no import derived: a `transfer_in` emitted on approval comes from no row at
    /// all [DOM-090], so it belongs to no batch and no batch deletion reaches it.
    #[must_use]
    pub fn emitted(account: Account, security: Isin) -> Self {
        Self {
            account,
            security,
            derived_by: None,
        }
    }
}

pub struct TransactionRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> TransactionRepository<'a> {
    pub(super) fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Stores `transaction` at `placement`, header, detail, citations and placement in one
    /// SQLite transaction, so a stored transaction is never half stored and never unplaced.
    ///
    /// Refused with [`StorageError::UnknownRecord`] when a citation names no record this
    /// database holds [DOM-047].
    pub async fn insert(
        &self,
        placement: &Placement,
        transaction: &Transaction,
    ) -> Result<TransactionId, StorageError> {
        let mut tx = self.pool.begin().await?;

        let kind = match transaction {
            Transaction::Opening(Opening::Buy(_)) => BUY,
            Transaction::Opening(Opening::TransferIn(_)) => TRANSFER_IN,
            Transaction::Closing(Closing::Sell(_)) => SELL,
            Transaction::Closing(Closing::Expiration(_)) => EXPIRATION,
            Transaction::Closing(Closing::TransferOut(_)) => TRANSFER_OUT,
            Transaction::Split(_) => SPLIT,
        };

        let key = transaction.order_key();
        let header = query(
            "insert into transaction_record (kind, trade_date, ordering, batch_age, leg)
             values (?, ?, ?, ?, ?)",
        )
        .bind(kind)
        .bind(key.trade_date())
        .bind(i64::from(key.position().order().get()))
        .bind(key.position().batch_age().get())
        .bind(leg_rank(key.leg()))
        .execute(&mut *tx)
        .await?;
        let id = header.last_insert_rowid();

        match transaction {
            Transaction::Opening(Opening::Buy(buy)) => {
                let unit_price = pair_at_scale("unit_price", buy.unit_price())?;
                let gross = pair_at_scale("gross", buy.gross())?;
                let fees = pair_at_scale("fees", buy.fees())?;
                query(
                    "insert into transaction_buy
                         (transaction_id, quantity, unit_price_native, unit_price_eur,
                          gross_native, gross_eur, fees_native, fees_eur, origin,
                          conversion_currency, conversion_rate, conversion_source,
                          conversion_rate_date)
                     values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(id)
                .bind(at_scale("quantity", buy.quantity())?)
                .bind(unit_price.0)
                .bind(unit_price.1)
                .bind(gross.0)
                .bind(gross.1)
                .bind(fees.0)
                .bind(fees.1)
                .bind(buy_origin_code(buy.origin()))
                .bind_conversion(buy.conversion())?
                .execute(&mut *tx)
                .await?;
            }
            Transaction::Opening(Opening::TransferIn(transfer_in)) => {
                let cost_basis = pair_at_scale("cost_basis", transfer_in.cost_basis())?;
                let fees = pair_at_scale("fees", transfer_in.fees())?;
                query(
                    "insert into transaction_transfer_in
                         (transaction_id, quantity, cost_basis_native, cost_basis_eur,
                          fees_native, fees_eur, acquisition_date, date_provenance, source,
                          conversion_currency, conversion_rate, conversion_source,
                          conversion_rate_date)
                     values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(id)
                .bind(at_scale("quantity", transfer_in.quantity())?)
                .bind(cost_basis.0)
                .bind(cost_basis.1)
                .bind(fees.0)
                .bind(fees.1)
                .bind(transfer_in.acquisition_date())
                .bind(date_provenance_code(transfer_in.date_provenance()))
                .bind(transfer_in_source_code(transfer_in.source()))
                .bind_conversion(transfer_in.conversion())?
                .execute(&mut *tx)
                .await?;
            }
            Transaction::Closing(Closing::Sell(sell)) => {
                let unit_price = pair_at_scale("unit_price", sell.unit_price())?;
                let gross = pair_at_scale("gross", sell.gross())?;
                let fees = pair_at_scale("fees", sell.fees())?;
                query(
                    "insert into transaction_sell
                         (transaction_id, quantity, unit_price_native, unit_price_eur,
                          gross_native, gross_eur, fees_native, fees_eur,
                          conversion_currency, conversion_rate, conversion_source,
                          conversion_rate_date)
                     values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(id)
                .bind(at_scale("quantity", sell.quantity())?)
                .bind(unit_price.0)
                .bind(unit_price.1)
                .bind(gross.0)
                .bind(gross.1)
                .bind(fees.0)
                .bind(fees.1)
                .bind_conversion(sell.conversion())?
                .execute(&mut *tx)
                .await?;
            }
            Transaction::Closing(Closing::Expiration(expiration)) => {
                let gross = pair_at_scale("gross", expiration.gross())?;
                let fees = pair_at_scale("fees", expiration.fees())?;
                query(
                    "insert into transaction_expiration
                         (transaction_id, gross_native, gross_eur, fees_native, fees_eur,
                          conversion_currency, conversion_rate, conversion_source,
                          conversion_rate_date)
                     values (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(id)
                .bind(gross.0)
                .bind(gross.1)
                .bind(fees.0)
                .bind(fees.1)
                .bind_conversion(expiration.conversion())?
                .execute(&mut *tx)
                .await?;
            }
            Transaction::Closing(Closing::TransferOut(transfer_out)) => {
                let fees = pair_at_scale("fees", transfer_out.fees())?;
                query(
                    "insert into transaction_transfer_out
                         (transaction_id, quantity, fees_native, fees_eur,
                          conversion_currency, conversion_rate, conversion_source,
                          conversion_rate_date)
                     values (?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(id)
                .bind(at_scale("quantity", transfer_out.quantity())?)
                .bind(fees.0)
                .bind(fees.1)
                .bind_conversion(transfer_out.conversion())?
                .execute(&mut *tx)
                .await?;
            }
            Transaction::Split(split) => {
                query(
                    "insert into transaction_split
                         (transaction_id, ratio_numerator, ratio_denominator)
                     values (?, ?, ?)",
                )
                .bind(id)
                .bind(i64::from(split.ratio().numerator().get()))
                .bind(i64::from(split.ratio().denominator().get()))
                .execute(&mut *tx)
                .await?;
            }
        }

        // A handle says its record was stored when the handle was issued; an import undo since,
        // or a handle from another database, leaves it naming nothing stored here. The citation
        // is therefore written only from a stored record, in this SQLite transaction, so a
        // derivation is never created from a record that is gone [DOM-047]. The column stays a
        // value rather than a foreign key because the record may go afterwards [DOM-099].
        for (ordinal, identity) in (0i64..).zip(transaction.cites()) {
            let written = query(
                "insert into transaction_citation (transaction_id, ordinal, record_identity)
                 select ?, ?, identity from source_record where identity = ?",
            )
            .bind(id)
            .bind(ordinal)
            .bind(identity.as_str())
            .execute(&mut *tx)
            .await?;
            if written.rows_affected() == 0 {
                return Err(StorageError::UnknownRecord {
                    identity: identity.as_str().to_owned(),
                });
            }
        }

        query(
            "insert into transaction_placement
                 (transaction_id, account_broker, account_id, security_isin, derived_by_batch)
             values (?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(placement.account.broker())
        .bind(placement.account.id())
        .bind(placement.security.as_str())
        .bind(placement.derived_by.map(BatchId::get))
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(TransactionId::new(id))
    }

    /// The account and security `id` is a transaction of [DOM-013], or `None` if no such
    /// transaction is stored. Attribution reads it to hold every allocated opening to its
    /// closing's pair [DOM-019], inside the SQLite transaction that writes the attribution.
    pub(crate) async fn account_and_security_in(
        connection: &mut SqliteConnection,
        id: TransactionId,
    ) -> Result<Option<(Account, Isin)>, StorageError> {
        Ok(query(
            "select account_broker, account_id, security_isin from transaction_placement
             where transaction_id = ?",
        )
        .bind(id.get())
        .fetch_optional(connection)
        .await?
        .map(|row| {
            (
                Account::new(
                    row.get::<String, _>("account_broker"),
                    row.get::<String, _>("account_id"),
                ),
                Isin::new(row.get::<String, _>("security_isin")),
            )
        }))
    }

    pub async fn find(&self, id: TransactionId) -> Result<Option<Transaction>, StorageError> {
        Self::find_in(&mut *self.pool.acquire().await?, id).await
    }

    /// [`Self::find`] on `connection`, so a caller can read inside its own SQLite transaction.
    pub(crate) async fn find_in(
        connection: &mut SqliteConnection,
        id: TransactionId,
    ) -> Result<Option<Transaction>, StorageError> {
        let Some(header) = query(
            "select kind, trade_date, ordering, batch_age, leg from transaction_record where id = ?",
        )
        .bind(id.get())
        .fetch_optional(&mut *connection)
        .await?
        else {
            return Ok(None);
        };

        let cites = query(
            "select record_identity from transaction_citation
             where transaction_id = ? order by ordinal",
        )
        .bind(id.get())
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .map(|row| RecordIdentity::new(row.get::<String, _>("record_identity")))
        .collect();
        // `insert` writes every citation a derivation holds, and a derivation holds at least one
        // [DOM-047], so a header without citations was not written by this code; it is refused
        // rather than read back as a transaction derived from nothing. The citations it does
        // hold were written from handles, which is why they are handles again here.
        let cites = Vec1::try_from_vec(cites).map_err(|_| StorageError::CorruptValue {
            field: "transaction_citation",
            value: id.to_string(),
        })?;

        let key = order_key(&header)?;
        // Each handle carries the transaction's own stored position: the records themselves may
        // be gone [DOM-099], and `Derivation::stored` reads the key, not the handles, for it.
        let cites = cites.mapped(|identity| RecordHandle::new(identity, key.position()));
        let derivation = Derivation::stored(key.trade_date(), cites, key);
        let kind = header.get::<String, _>("kind");

        let transaction = match kind.as_str() {
            BUY => {
                let row = Self::detail(
                    &mut *connection,
                    id,
                    "select * from transaction_buy where transaction_id = ?",
                )
                .await?;
                Transaction::from(Opening::from(Buy::new(
                    derivation,
                    read_quantity("quantity", &text(&row, "quantity"))?,
                    price_pair(
                        "unit_price",
                        &text(&row, "unit_price_native"),
                        &text(&row, "unit_price_eur"),
                    )?,
                    money_pair(
                        "gross",
                        &text(&row, "gross_native"),
                        &text(&row, "gross_eur"),
                    )?,
                    money_pair("fees", &text(&row, "fees_native"), &text(&row, "fees_eur"))?,
                    buy_origin(&text(&row, "origin"))?,
                    conversion(&row)?,
                )))
            }
            TRANSFER_IN => {
                let row = Self::detail(
                    &mut *connection,
                    id,
                    "select * from transaction_transfer_in where transaction_id = ?",
                )
                .await?;
                Transaction::from(Opening::from(TransferIn::new(
                    derivation,
                    read_quantity("quantity", &text(&row, "quantity"))?,
                    money_pair(
                        "cost_basis",
                        &text(&row, "cost_basis_native"),
                        &text(&row, "cost_basis_eur"),
                    )?,
                    money_pair("fees", &text(&row, "fees_native"), &text(&row, "fees_eur"))?,
                    row.get::<NaiveDate, _>("acquisition_date"),
                    date_provenance(&text(&row, "date_provenance"))?,
                    transfer_in_source(&text(&row, "source"))?,
                    conversion(&row)?,
                )))
            }
            SELL => {
                let row = Self::detail(
                    &mut *connection,
                    id,
                    "select * from transaction_sell where transaction_id = ?",
                )
                .await?;
                Transaction::from(Closing::from(Sell::new(
                    derivation,
                    read_quantity("quantity", &text(&row, "quantity"))?,
                    price_pair(
                        "unit_price",
                        &text(&row, "unit_price_native"),
                        &text(&row, "unit_price_eur"),
                    )?,
                    money_pair(
                        "gross",
                        &text(&row, "gross_native"),
                        &text(&row, "gross_eur"),
                    )?,
                    money_pair("fees", &text(&row, "fees_native"), &text(&row, "fees_eur"))?,
                    conversion(&row)?,
                )))
            }
            EXPIRATION => {
                let row = Self::detail(
                    &mut *connection,
                    id,
                    "select * from transaction_expiration where transaction_id = ?",
                )
                .await?;
                Transaction::from(Closing::from(Expiration::new(
                    derivation,
                    money_pair(
                        "gross",
                        &text(&row, "gross_native"),
                        &text(&row, "gross_eur"),
                    )?,
                    money_pair("fees", &text(&row, "fees_native"), &text(&row, "fees_eur"))?,
                    conversion(&row)?,
                )))
            }
            TRANSFER_OUT => {
                let row = Self::detail(
                    &mut *connection,
                    id,
                    "select * from transaction_transfer_out where transaction_id = ?",
                )
                .await?;
                Transaction::from(Closing::from(TransferOut::new(
                    derivation,
                    read_quantity("quantity", &text(&row, "quantity"))?,
                    money_pair("fees", &text(&row, "fees_native"), &text(&row, "fees_eur"))?,
                    conversion(&row)?,
                )))
            }
            SPLIT => {
                let row = Self::detail(
                    &mut *connection,
                    id,
                    "select * from transaction_split where transaction_id = ?",
                )
                .await?;
                Transaction::from(Split::new(derivation, ratio(&row)?))
            }
            other => {
                return Err(StorageError::CorruptValue {
                    field: "kind",
                    value: other.to_owned(),
                });
            }
        };

        Ok(Some(transaction))
    }

    /// Records that `transfer_out` emitted `transfer_in` on approval, one such record per parcel
    /// consumed [DOM-090]. The link is what DOM-094 refuses to let a deletion break.
    ///
    /// Both kinds are checked first: the link makes its `transfer_in` deletable only through its
    /// emitter, so recording it over a pair of any other kinds would freeze a transaction behind
    /// one it has nothing to do with. Check and write share one SQLite transaction.
    pub async fn record_emission(
        &self,
        transfer_out: TransactionId,
        transfer_in: TransactionId,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;

        for (transaction, expected) in [(transfer_out, TRANSFER_OUT), (transfer_in, TRANSFER_IN)] {
            let kind = kind_of(&mut tx, transaction).await?;
            if kind != expected {
                return Err(StorageError::NotOfKind {
                    transaction,
                    expected,
                    kind,
                });
            }
        }

        query("insert into emitted_transfer_in (transfer_in_id, transfer_out_id) values (?, ?)")
            .bind(transfer_in.get())
            .bind(transfer_out.get())
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Deletes a transaction, refusing while it participates in an attribution [DOM-069] and
    /// refusing a `transfer_in` that a `transfer_out` emitted [DOM-094].
    ///
    /// Deleting the `transfer_out` takes its emitted records with it, which is what "not
    /// independently of it" leaves allowed.
    pub async fn delete(&self, transaction: TransactionId) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;

        if let Some(transfer_out) = emitting_transfer_out(&mut tx, transaction).await? {
            return Err(StorageError::EmittedTransferIn {
                transfer_in: transaction,
                transfer_out,
            });
        }

        delete_transactions(&mut tx, &[transaction]).await?;

        tx.commit().await?;
        Ok(())
    }

    /// The variant's own row. Its absence means a header without its detail, which the insert
    /// above cannot produce.
    async fn detail(
        connection: &mut SqliteConnection,
        id: TransactionId,
        sql: &'static str,
    ) -> Result<SqliteRow, StorageError> {
        // The statement is a literal per variant rather than a table name interpolated into
        // one: SQLite binds values, never identifiers.
        query(sql)
            .bind(id.get())
            .fetch_optional(connection)
            .await?
            .ok_or(StorageError::CorruptValue {
                field: "transaction_id",
                value: id.get().to_string(),
            })
    }
}

/// The stored kind of `transaction`, or a refusal if no such transaction is stored.
async fn kind_of(
    connection: &mut SqliteConnection,
    transaction: TransactionId,
) -> Result<String, StorageError> {
    Ok(query("select kind from transaction_record where id = ?")
        .bind(transaction.get())
        .fetch_optional(connection)
        .await?
        .ok_or(StorageError::UnknownTransaction { transaction })?
        .get::<String, _>("kind"))
}

/// The attribution a transaction takes part in, as closing or as the opening of an allocation
/// [DOM-069].
pub(super) async fn participating_attribution(
    connection: &mut SqliteConnection,
    transaction: TransactionId,
) -> Result<Option<AttributionId>, StorageError> {
    Ok(query(
        "select a.id from attribution a
         where a.closing_transaction_id = ?
            or exists (select 1 from attribution_allocation al
                       where al.attribution_id = a.id and al.opening_transaction_id = ?)
         order by a.id
         limit 1",
    )
    .bind(transaction.get())
    .bind(transaction.get())
    .fetch_optional(connection)
    .await?
    .map(|row| AttributionId::new(row.get("id"))))
}

/// The `transfer_out` that emitted `transaction`, if it is an emitted record [DOM-094].
async fn emitting_transfer_out(
    connection: &mut SqliteConnection,
    transaction: TransactionId,
) -> Result<Option<TransactionId>, StorageError> {
    Ok(
        query("select transfer_out_id from emitted_transfer_in where transfer_in_id = ?")
            .bind(transaction.get())
            .fetch_optional(connection)
            .await?
            .map(|row| TransactionId::new(row.get("transfer_out_id"))),
    )
}

/// The `transfer_in` records a `transfer_out` emitted [DOM-090].
async fn emitted_by(
    connection: &mut SqliteConnection,
    transfer_out: TransactionId,
) -> Result<Vec<TransactionId>, StorageError> {
    Ok(
        query("select transfer_in_id from emitted_transfer_in where transfer_out_id = ?")
            .bind(transfer_out.get())
            .fetch_all(connection)
            .await?
            .iter()
            .map(|row| TransactionId::new(row.get("transfer_in_id")))
            .collect(),
    )
}

/// Deletes each transaction together with the records it emitted, refusing the whole group if any
/// member participates in an attribution [DOM-069], [DOM-094].
///
/// Shared with the batch deletion, so an import undo cannot reach round the invariant a single
/// deletion meets. The caller owns the SQLite transaction, so a refusal rolls the group back.
pub(super) async fn delete_transactions(
    connection: &mut SqliteConnection,
    transactions: &[TransactionId],
) -> Result<(), StorageError> {
    for &transaction in transactions {
        let mut group = vec![transaction];
        group.extend(emitted_by(connection, transaction).await?);

        for &member in &group {
            if let Some(attribution) = participating_attribution(connection, member).await? {
                return Err(StorageError::TransactionAttributed {
                    transaction: member,
                    attribution,
                });
            }
        }

        for member in group {
            // The detail row, the citations, the placement and the emission link all cascade.
            query("delete from transaction_record where id = ?")
                .bind(member.get())
                .execute(&mut *connection)
                .await?;
        }
    }
    Ok(())
}

/// The stored rank of a leg: an integer, so that the row comparison the attribution invariants
/// order by reads a lead before its trailing leg (DEC-090).
fn leg_rank(leg: Leg) -> i64 {
    match leg {
        Leg::Lead => 0,
        Leg::Trailing => 1,
    }
}

/// A transaction's stored place in the canonical order [DOM-011], [DOM-111].
fn order_key(header: &SqliteRow) -> Result<OrderKey, StorageError> {
    let ordering = header.get::<i64, _>("ordering");
    let order = u32::try_from(ordering).map_err(|_| StorageError::CorruptValue {
        field: "ordering",
        value: ordering.to_string(),
    })?;
    let leg = match header.get::<i64, _>("leg") {
        0 => Leg::Lead,
        1 => Leg::Trailing,
        other => {
            return Err(StorageError::CorruptValue {
                field: "leg",
                value: other.to_string(),
            });
        }
    };
    Ok(OrderKey::new(
        header.get::<NaiveDate, _>("trade_date"),
        RecordPosition::new(Order::new(order), BatchAge::new(header.get("batch_age"))),
        leg,
    ))
}

fn text(row: &SqliteRow, column: &str) -> String {
    row.get::<String, _>(column)
}

fn conversion(row: &SqliteRow) -> Result<Conversion, StorageError> {
    Ok(Conversion::new(
        Currency::new(text(row, "conversion_currency")),
        rate("conversion_rate", &text(row, "conversion_rate"))?,
        rate_source(&text(row, "conversion_source"))?,
        row.get::<NaiveDate, _>("conversion_rate_date"),
    ))
}

/// Binds a [`Conversion`]'s four columns in one step, so no insert can bind a rate without the
/// currency, source and date that make it auditable [DOM-028].
trait BindConversion: Sized {
    fn bind_conversion(self, conversion: &Conversion) -> Result<Self, StorageError>;
}

impl<'q> BindConversion for sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    fn bind_conversion(self, conversion: &Conversion) -> Result<Self, StorageError> {
        Ok(self
            .bind(conversion.currency().code().to_owned())
            .bind(at_scale("conversion_rate", conversion.rate())?)
            .bind(rate_source_code(conversion.source()))
            .bind(conversion.rate_date()))
    }
}
