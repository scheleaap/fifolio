//! The transaction repository: a header row per transaction, the fields of its variant in a
//! table of that variant's own, and its citations in a third.
//!
//! The variant tables are what keeps [DOM-010] true on disk: there is no nullable column
//! standing in for "not applicable here", so a row of one variant cannot be read as another.
//! Header, detail and citations are written in one SQLite transaction, so a transaction is
//! never half stored.

use chrono::NaiveDate;
use sqlx::sqlite::{SqlitePool, SqliteRow};
use sqlx::{Row, query};

use crate::entities::RecordIdentity;
use crate::storage::codec::{
    at_scale, buy_origin, buy_origin_code, date_provenance, date_provenance_code, money_pair,
    pair_at_scale, price_pair, quantity as read_quantity, rate, rate_source, rate_source_code,
    transfer_in_source, transfer_in_source_code,
};
use crate::storage::{StorageError, row_id};
use crate::transaction::{
    Buy, Closing, Derivation, Expiration, Opening, Sell, Split, Transaction, TransferIn,
    TransferOut,
};
use crate::valuation::{Conversion, Currency};

row_id!(
    /// A transaction's key. Surrogate: the natural key would be the source record it consumes,
    /// which is DOM-011 and undecided.
    TransactionId
);

/// The stored code of each variant, and the detail table that holds its fields.
const BUY: &str = "buy";
const TRANSFER_IN: &str = "transfer_in";
const SELL: &str = "sell";
const EXPIRATION: &str = "expiration";
const TRANSFER_OUT: &str = "transfer_out";
const SPLIT: &str = "split";

pub struct TransactionRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> TransactionRepository<'a> {
    pub(super) fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, transaction: &Transaction) -> Result<TransactionId, StorageError> {
        let mut tx = self.pool.begin().await?;

        let kind = match transaction {
            Transaction::Opening(Opening::Buy(_)) => BUY,
            Transaction::Opening(Opening::TransferIn(_)) => TRANSFER_IN,
            Transaction::Closing(Closing::Sell(_)) => SELL,
            Transaction::Closing(Closing::Expiration(_)) => EXPIRATION,
            Transaction::Closing(Closing::TransferOut(_)) => TRANSFER_OUT,
            Transaction::Split(_) => SPLIT,
        };

        let header = query("insert into transaction_record (kind, trade_date) values (?, ?)")
            .bind(kind)
            .bind(transaction.trade_date())
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
            // A split has no fields of its own yet [DOM-113, FIF-061], so the header row is the
            // whole of it.
            Transaction::Split(_) => {}
        }

        for (ordinal, identity) in (0i64..).zip(transaction.cites()) {
            query(
                "insert into transaction_citation (transaction_id, ordinal, record_identity)
                 values (?, ?, ?)",
            )
            .bind(id)
            .bind(ordinal)
            .bind(identity.as_str())
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(TransactionId::new(id))
    }

    pub async fn find(&self, id: TransactionId) -> Result<Option<Transaction>, StorageError> {
        let Some(header) = query("select kind, trade_date from transaction_record where id = ?")
            .bind(id.get())
            .fetch_optional(self.pool)
            .await?
        else {
            return Ok(None);
        };

        let cites = query(
            "select record_identity from transaction_citation
             where transaction_id = ? order by ordinal",
        )
        .bind(id.get())
        .fetch_all(self.pool)
        .await?
        .into_iter()
        .map(|row| RecordIdentity::new(row.get::<String, _>("record_identity")));

        let derivation = Derivation::new(header.get::<NaiveDate, _>("trade_date"), cites);
        let kind = header.get::<String, _>("kind");

        let transaction = match kind.as_str() {
            BUY => {
                let row = self
                    .detail(id, "select * from transaction_buy where transaction_id = ?")
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
                let row = self
                    .detail(
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
                let row = self
                    .detail(
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
                let row = self
                    .detail(
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
                let row = self
                    .detail(
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
            SPLIT => Transaction::from(Split::new(derivation)),
            other => {
                return Err(StorageError::CorruptValue {
                    field: "kind",
                    value: other.to_owned(),
                });
            }
        };

        Ok(Some(transaction))
    }

    /// The variant's own row. Its absence means a header without its detail, which the insert
    /// above cannot produce.
    async fn detail(
        &self,
        id: TransactionId,
        sql: &'static str,
    ) -> Result<SqliteRow, StorageError> {
        // The statement is a literal per variant rather than a table name interpolated into
        // one: SQLite binds values, never identifiers.
        query(sql)
            .bind(id.get())
            .fetch_optional(self.pool)
            .await?
            .ok_or(StorageError::CorruptValue {
                field: "transaction_id",
                value: id.get().to_string(),
            })
    }
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
