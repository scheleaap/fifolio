//! Reading, listing and deleting transactions [SRV-028], [SRV-029], [SRV-033].
//!
//! The list filters on account, security, type and trade-date range, and with `unattributed`
//! answers the closings awaiting attribution [SRV-029]; what each filter reaches is DEC-126
//! (provisional). Deleting a derived transaction returns the source records it cited to pending,
//! which the source record list's `status` filter then shows (DEC-119, provisional); a deletion
//! DOM-069 or DOM-094 forbids is refused with the problem type of that rule.
//!
//! There is no route that edits a transaction [SRV-054]: a transferred parcel's acquisition
//! date in particular is fixed at import and never corrected, and an attributed transaction is
//! immutable [DOM-069]. A mistake is corrected by deleting the derived transaction and the manual
//! entry, then supplying a new one [SRV-027].
//!
//! Figures travel as decimal strings, never JSON numbers, which a client could read through
//! floating point [ARC-006].

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::NaiveDate;
use fifolio_core::decimal::Scaled;
use fifolio_core::entities::{Account, Isin};
use fifolio_core::storage::{
    Database, StorageError, StoredTransaction, TransactionFilter, TransactionId, TransactionKind,
};
use fifolio_core::transaction::{
    BuyOrigin, Closing, DateProvenance, Opening, Transaction, TransferInSource,
};
use fifolio_core::valuation::{Conversion, RateSource, Valued};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::accounts::AccountBody;
use crate::manual_entries::RatioBody;
use crate::problem::Problem;

/// A transaction's variant [DOM-010], spelled as storage records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransactionKindBody {
    Buy,
    TransferIn,
    Sell,
    Expiration,
    TransferOut,
    Split,
}

impl From<TransactionKindBody> for TransactionKind {
    fn from(kind: TransactionKindBody) -> Self {
        match kind {
            TransactionKindBody::Buy => Self::Buy,
            TransactionKindBody::TransferIn => Self::TransferIn,
            TransactionKindBody::Sell => Self::Sell,
            TransactionKindBody::Expiration => Self::Expiration,
            TransactionKindBody::TransferOut => Self::TransferOut,
            TransactionKindBody::Split => Self::Split,
        }
    }
}

/// A figure in the transaction's own currency and in EUR [DOM-028], each a decimal string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ValuedBody {
    #[schema(example = "230.00")]
    pub native: String,
    #[schema(example = "214.05")]
    pub eur: String,
}

impl<T: Scaled> From<Valued<T>> for ValuedBody {
    fn from(valued: Valued<T>) -> Self {
        Self {
            native: valued.native().get().to_string(),
            eur: valued.eur().get().to_string(),
        }
    }
}

/// Where a rate came from [DOM-030].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RateSourceBody {
    Broker,
    Ecb,
    Native,
}

impl From<RateSource> for RateSourceBody {
    fn from(source: RateSource) -> Self {
        match source {
            RateSource::Broker => Self::Broker,
            RateSource::Ecb => Self::Ecb,
            RateSource::Native => Self::Native,
        }
    }
}

/// The conversion the EUR figures were obtained under: currency, rate, source and the rate's
/// date [DOM-028].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ConversionBody {
    #[schema(example = "USD")]
    pub currency: String,
    /// Foreign units per EUR, as a decimal string.
    #[schema(example = "1.074500")]
    pub rate: String,
    #[schema(inline)]
    pub source: RateSourceBody,
    pub rate_date: NaiveDate,
}

impl From<&Conversion> for ConversionBody {
    fn from(conversion: &Conversion) -> Self {
        Self {
            currency: conversion.currency().code().to_owned(),
            rate: conversion.rate().get().to_string(),
            source: conversion.source().into(),
            rate_date: conversion.rate_date(),
        }
    }
}

/// How a buy arose [DOM-082].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BuyOriginBody {
    Purchase,
    StockDividend,
}

/// Where a `transfer_in`'s units came from [DOM-083].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransferInSourceBody {
    Broker,
    CorporateAction,
}

/// What a `transfer_in`'s acquisition date is worth [DOM-083].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DateProvenanceBody {
    TransferDate,
    Inherited,
}

/// The fields of one variant, and only those [DOM-010].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransactionDetailBody {
    Buy {
        quantity: String,
        unit_price: ValuedBody,
        gross: ValuedBody,
        fees: ValuedBody,
        #[schema(inline)]
        origin: BuyOriginBody,
        conversion: ConversionBody,
    },
    TransferIn {
        quantity: String,
        cost_basis: ValuedBody,
        fees: ValuedBody,
        acquisition_date: NaiveDate,
        #[schema(inline)]
        date_provenance: DateProvenanceBody,
        #[schema(inline)]
        source: TransferInSourceBody,
        conversion: ConversionBody,
    },
    Sell {
        quantity: String,
        unit_price: ValuedBody,
        gross: ValuedBody,
        fees: ValuedBody,
        conversion: ConversionBody,
    },
    Expiration {
        gross: ValuedBody,
        fees: ValuedBody,
        conversion: ConversionBody,
    },
    TransferOut {
        quantity: String,
        fees: ValuedBody,
        ratio: RatioBody,
        /// The ISIN of the security the emitted `transfer_in` records open parcels of.
        target: String,
        conversion: ConversionBody,
    },
    Split {
        ratio: RatioBody,
    },
}

impl From<&Transaction> for TransactionDetailBody {
    fn from(transaction: &Transaction) -> Self {
        match transaction {
            Transaction::Opening(Opening::Buy(buy)) => Self::Buy {
                quantity: buy.quantity().get().to_string(),
                unit_price: buy.unit_price().into(),
                gross: buy.gross().into(),
                fees: buy.fees().into(),
                origin: match buy.origin() {
                    BuyOrigin::Purchase => BuyOriginBody::Purchase,
                    BuyOrigin::StockDividend => BuyOriginBody::StockDividend,
                },
                conversion: buy.conversion().into(),
            },
            Transaction::Opening(Opening::TransferIn(transfer_in)) => Self::TransferIn {
                quantity: transfer_in.quantity().get().to_string(),
                cost_basis: transfer_in.cost_basis().into(),
                fees: transfer_in.fees().into(),
                acquisition_date: transfer_in.acquisition_date(),
                date_provenance: match transfer_in.date_provenance() {
                    DateProvenance::TransferDate => DateProvenanceBody::TransferDate,
                    DateProvenance::Inherited => DateProvenanceBody::Inherited,
                },
                source: match transfer_in.source() {
                    TransferInSource::Broker => TransferInSourceBody::Broker,
                    TransferInSource::CorporateAction => TransferInSourceBody::CorporateAction,
                },
                conversion: transfer_in.conversion().into(),
            },
            Transaction::Closing(Closing::Sell(sell)) => Self::Sell {
                quantity: sell.quantity().get().to_string(),
                unit_price: sell.unit_price().into(),
                gross: sell.gross().into(),
                fees: sell.fees().into(),
                conversion: sell.conversion().into(),
            },
            Transaction::Closing(Closing::Expiration(expiration)) => Self::Expiration {
                gross: expiration.gross().into(),
                fees: expiration.fees().into(),
                conversion: expiration.conversion().into(),
            },
            Transaction::Closing(Closing::TransferOut(transfer_out)) => Self::TransferOut {
                quantity: transfer_out.quantity().get().to_string(),
                fees: transfer_out.fees().into(),
                ratio: transfer_out.ratio().into(),
                target: transfer_out.target().as_str().to_owned(),
                conversion: transfer_out.conversion().into(),
            },
            Transaction::Split(split) => Self::Split {
                ratio: split.ratio().into(),
            },
        }
    }
}

/// A stored transaction with where it belongs [DOM-013].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TransactionBody {
    /// The id a read or a deletion addresses the transaction by.
    pub id: i64,
    pub account: AccountBody,
    /// The ISIN of the security the transaction is of.
    #[schema(example = "NL0000009538")]
    pub security: String,
    /// The import batch that derived the transaction; `null` for a `transfer_in` emitted on
    /// approval, which no import derived [DOM-090].
    pub derived_by: Option<i64>,
    pub trade_date: NaiveDate,
    /// The identities of the source records the transaction was derived from, in order
    /// [DOM-047].
    pub cites: Vec<String>,
    pub detail: TransactionDetailBody,
}

impl From<&StoredTransaction> for TransactionBody {
    fn from(stored: &StoredTransaction) -> Self {
        let transaction = stored.transaction();
        Self {
            id: stored.id().get(),
            account: AccountBody::from(stored.account()),
            security: stored.security().as_str().to_owned(),
            derived_by: stored.derived_by().map(|batch| batch.get()),
            trade_date: transaction.trade_date(),
            cites: transaction
                .cites()
                .iter()
                .map(|identity| identity.as_str().to_owned())
                .collect(),
            detail: transaction.into(),
        }
    }
}

/// Which transactions to list [SRV-028], [SRV-029]. Every filter is optional; those given must
/// all hold.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TransactionQuery {
    /// The account's broker; given together with `account`.
    #[param(example = "Saxo")]
    pub broker: Option<String>,
    /// The account's id; given together with `broker`.
    #[param(example = "69900/1000000")]
    pub account: Option<String>,
    /// The ISIN of the security the transaction is of.
    #[param(example = "NL0000009538")]
    pub security: Option<String>,
    /// The transaction's variant.
    #[serde(rename = "type")]
    #[param(inline)]
    pub kind: Option<TransactionKindBody>,
    /// The earliest trade date, inclusive.
    pub from: Option<NaiveDate>,
    /// The latest trade date, inclusive; not before `from`.
    pub to: Option<NaiveDate>,
    /// `true` for only the closings no attribution closes [SRV-029].
    pub unattributed: Option<bool>,
}

impl TransactionQuery {
    /// The filter this query states, or a 400 for an account named by half or a date range that
    /// ends before it starts: either names nothing, and answering an empty list would read as
    /// "nothing to attribute" (DEC-126, provisional).
    fn filter(self) -> Result<TransactionFilter, Problem> {
        let bad_request =
            |detail: &str| Problem::status_only(StatusCode::BAD_REQUEST, Some(detail.to_owned()));
        let account = match (self.broker, self.account) {
            (Some(broker), Some(id)) => Some(Account::new(broker, id)),
            (None, None) => None,
            _ => {
                return Err(bad_request(
                    "broker and account filter together, naming one account",
                ));
            }
        };
        if let (Some(from), Some(to)) = (self.from, self.to)
            && to < from
        {
            return Err(bad_request("the date range ends before it starts"));
        }
        Ok(TransactionFilter {
            account,
            security: self.security.map(Isin::new),
            kind: self.kind.map(TransactionKind::from),
            from: self.from,
            to: self.to,
            unattributed_closings: self.unattributed.unwrap_or(false),
        })
    }
}

pub(crate) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_transactions))
        .routes(routes!(get_transaction, delete_transaction))
}

/// Every transaction the filters admit, in canonical order [SRV-028]; with `unattributed=true`,
/// the closings awaiting attribution [SRV-029].
#[utoipa::path(
    get,
    path = "/transactions",
    tag = "transactions",
    params(TransactionQuery),
    responses(
        (status = 200, description = "The transactions the filters admit",
         body = [TransactionBody]),
        (status = 400, description = "A broker without an account id or the reverse, a date \
         range ending before it starts, or a value that does not parse", body = Problem,
         content_type = "application/problem+json"),
        (status = 404, description = "The account or security filtered on is not stored",
         body = Problem, content_type = "application/problem+json"),
    )
)]
async fn list_transactions(
    State(database): State<Database>,
    Query(query): Query<TransactionQuery>,
) -> Result<Json<Vec<TransactionBody>>, Problem> {
    let transactions = database.transactions().list(&query.filter()?).await?;
    Ok(Json(
        transactions.iter().map(TransactionBody::from).collect(),
    ))
}

/// One transaction [SRV-028].
#[utoipa::path(
    get,
    path = "/transactions/{id}",
    tag = "transactions",
    params(("id" = i64, Path, description = "The transaction's id")),
    responses(
        (status = 200, description = "The transaction", body = TransactionBody),
        (status = 404, description = "No such transaction", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn get_transaction(
    State(database): State<Database>,
    Path(id): Path<i64>,
) -> Result<Json<TransactionBody>, Problem> {
    let transaction = TransactionId::new(id);
    database
        .transactions()
        .read(transaction)
        .await?
        .map(|stored| Json(TransactionBody::from(&stored)))
        .ok_or_else(|| StorageError::UnknownTransaction { transaction }.into())
}

/// Deletes a derived transaction, returning the source records it cited to pending [SRV-033].
/// Refused while it participates in an attribution [DOM-069], and for a `transfer_in` a
/// `transfer_out` emitted, which goes only with its emitter [DOM-094].
#[utoipa::path(
    delete,
    path = "/transactions/{id}",
    tag = "transactions",
    params(("id" = i64, Path, description = "The transaction's id")),
    responses(
        (status = 204, description = "The transaction is deleted"),
        (status = 404, description = "No such transaction", body = Problem,
         content_type = "application/problem+json"),
        (status = 409, description = "The transaction participates in an attribution, or is a \
         transfer_in a transfer_out emitted", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn delete_transaction(
    State(database): State<Database>,
    Path(id): Path<i64>,
) -> Result<StatusCode, Problem> {
    let transaction = TransactionId::new(id);
    // Storage deletes an absent transaction as already true, which its tests pin; the route
    // answers 404 instead, as deleting an absent batch or manual entry does, so a mistyped id
    // does not read as a correction made (DEC-126, provisional). Storage answers the absence
    // from inside the deleting transaction, so a concurrent deletion cannot make both 204.
    if !database.transactions().delete(transaction).await? {
        return Err(StorageError::UnknownTransaction { transaction }.into());
    }
    Ok(StatusCode::NO_CONTENT)
}
