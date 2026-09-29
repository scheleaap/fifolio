//! Creating, listing and deleting manual entries [SRV-025], [SRV-026], [SRV-053].
//!
//! Posting an entry is the only way information no export contains enters the system
//! [SRV-025], and it is idempotent on the entry's content and the identities it cites, so
//! replaying an exported file duplicates nothing [SRV-048]: a new entry answers 201, one already
//! stored answers 200 with the stored entry (DEC-121, provisional). A new entry must cite at least
//! one record, each stored (DEC-122, provisional).
//!
//! There is no route that edits an entry: a mistake is corrected by deleting it and supplying a
//! new one [SRV-027]. Deleting is the only removal of an entry [SRV-053]; undoing an import
//! never removes one [SRV-021].
//!
//! Quantities travel as decimal strings, never JSON numbers, which a client could read through
//! floating point [ARC-006].

use std::num::NonZeroU32;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use fifolio_core::decimal::{Quantity, Scaled};
use fifolio_core::entities::{Account, Isin, RecordIdentity};
use fifolio_core::manual_entry::{Election, ManualEntry, Ratio, Supplied};
use fifolio_core::storage::{Creation, Database, ManualEntryId, StorageError, WaitingEntry};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::accounts::AccountBody;
use crate::problem::Problem;

/// A ratio as an exact integer pair: `numerator` new units for every `denominator` held
/// [DOM-113].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RatioBody {
    #[schema(value_type = u32, minimum = 1)]
    pub numerator: NonZeroU32,
    #[schema(value_type = u32, minimum = 1)]
    pub denominator: NonZeroU32,
}

impl From<Ratio> for RatioBody {
    fn from(ratio: Ratio) -> Self {
        Self {
            numerator: ratio.numerator(),
            denominator: ratio.denominator(),
        }
    }
}

impl From<RatioBody> for Ratio {
    fn from(body: RatioBody) -> Self {
        Ratio::new(body.numerator, body.denominator)
    }
}

/// What the user supplied, one shape per completion-queue case that asks for something
/// [DOM-097].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SuppliedBody {
    /// A dividend taken in stock, and how many shares, as a decimal string.
    ElectionStock {
        #[schema(example = "12")]
        shares: String,
    },
    /// A dividend taken in cash.
    ElectionCash,
    /// The ratio a split applied.
    Split { ratio: RatioBody },
    /// The security a holding was exchanged into, and at what ratio.
    Exchange { target: String, ratio: RatioBody },
    /// How much a cash merger, tender or partial buyback disposed of, as a decimal string, and
    /// the security received in return where there was one.
    Disposal {
        #[schema(example = "100")]
        quantity: String,
        target: Option<String>,
    },
}

impl From<&Supplied> for SuppliedBody {
    fn from(supplied: &Supplied) -> Self {
        match supplied {
            Supplied::Election(Election::Stock { shares }) => Self::ElectionStock {
                shares: shares.get().to_string(),
            },
            Supplied::Election(Election::Cash) => Self::ElectionCash,
            Supplied::Split(ratio) => Self::Split {
                ratio: (*ratio).into(),
            },
            Supplied::Exchange { target, ratio } => Self::Exchange {
                target: target.as_str().to_owned(),
                ratio: (*ratio).into(),
            },
            Supplied::Disposal { quantity, target } => Self::Disposal {
                quantity: quantity.get().to_string(),
                target: target.as_ref().map(|isin| isin.as_str().to_owned()),
            },
        }
    }
}

impl SuppliedBody {
    /// The supplied value, or a 422 naming a quantity that is not a decimal above zero.
    fn supplied(self) -> Result<Supplied, Problem> {
        Ok(match self {
            Self::ElectionStock { shares } => Supplied::Election(Election::Stock {
                shares: quantity("shares", &shares)?,
            }),
            Self::ElectionCash => Supplied::Election(Election::Cash),
            Self::Split { ratio } => Supplied::Split(ratio.into()),
            Self::Exchange { target, ratio } => Supplied::Exchange {
                target: Isin::new(target),
                ratio: ratio.into(),
            },
            Self::Disposal {
                quantity: stated,
                target,
            } => Supplied::Disposal {
                quantity: quantity("quantity", &stated)?,
                target: target.map(Isin::new),
            },
        })
    }
}

/// A share count or disposed quantity: a decimal above zero, since a count of nothing or less
/// would derive a buy or disposal no holding could produce (DEC-123, provisional).
fn quantity(field: &str, stated: &str) -> Result<Quantity, Problem> {
    let refused = |reason: &str| {
        Problem::status_only(
            StatusCode::UNPROCESSABLE_ENTITY,
            Some(format!("{field} {stated:?} {reason}")),
        )
    };
    // Exact: `from_str` rounds digits past the 96-bit mantissa away, which would store another
    // number than the one stated instead of refusing it [ARC-010].
    let value = Decimal::from_str_exact(stated).map_err(|_| refused("is not a decimal"))?;
    if value <= Decimal::ZERO {
        return Err(refused("is not above zero"));
    }
    Ok(Quantity::new(value))
}

/// A manual entry to create: everything an export records of one [CLI-009].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NewManualEntryBody {
    pub account: AccountBody,
    /// The ISIN of the security the entry is about.
    #[schema(example = "NL0000009538")]
    pub security: String,
    pub supplied: SuppliedBody,
    /// The identities of the source records the entry answers, in the order the completion
    /// queue showed them [DOM-098].
    pub answers: Vec<String>,
}

/// A stored manual entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ManualEntryBody {
    /// The id a deletion addresses the entry by.
    pub id: i64,
    pub account: AccountBody,
    #[schema(example = "NL0000009538")]
    pub security: String,
    pub supplied: SuppliedBody,
    /// The identities of the source records the entry answers, in order [DOM-098].
    pub answers: Vec<String>,
}

impl ManualEntryBody {
    fn new(id: ManualEntryId, entry: &ManualEntry) -> Self {
        Self {
            id: id.get(),
            account: AccountBody::from(entry.account()),
            security: entry.security().as_str().to_owned(),
            supplied: entry.supplied().into(),
            answers: identities(entry.answers()),
        }
    }
}

/// An entry whose source records are absent, and what it is waiting for [SRV-026], [DOM-109].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WaitingEntryBody {
    pub entry: ManualEntryBody,
    /// The identities the entry names that no stored source record carries, in its order.
    pub missing: Vec<String>,
}

impl From<&WaitingEntry> for WaitingEntryBody {
    fn from(waiting: &WaitingEntry) -> Self {
        Self {
            entry: ManualEntryBody::new(waiting.id(), waiting.entry()),
            missing: identities(waiting.missing()),
        }
    }
}

fn identities(identities: &[RecordIdentity]) -> Vec<String> {
    identities
        .iter()
        .map(|identity| identity.as_str().to_owned())
        .collect()
}

pub(crate) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_manual_entries, create_manual_entry))
        .routes(routes!(list_waiting_manual_entries))
        .routes(routes!(delete_manual_entry))
}

/// Every manual entry, oldest first, for export [SRV-026].
#[utoipa::path(
    get,
    path = "/manual-entries",
    tag = "manual-entries",
    responses((status = 200, description = "Every manual entry", body = [ManualEntryBody]))
)]
async fn list_manual_entries(
    State(database): State<Database>,
) -> Result<Json<Vec<ManualEntryBody>>, Problem> {
    let entries = database.manual_entries().list().await?;
    Ok(Json(
        entries
            .iter()
            .map(|(id, entry)| ManualEntryBody::new(*id, entry))
            .collect(),
    ))
}

/// Stores a manual entry, or answers the identical one already stored [SRV-025], [SRV-048].
#[utoipa::path(
    post,
    path = "/manual-entries",
    tag = "manual-entries",
    request_body = NewManualEntryBody,
    responses(
        (status = 201, description = "The entry, as stored", body = ManualEntryBody),
        (status = 200, description = "An identical entry was already stored; it is answered and \
         nothing is stored", body = ManualEntryBody),
        (status = 404, description = "The account, or a cited source record, is not stored",
         body = Problem, content_type = "application/problem+json"),
        (status = 422, description = "The entry cites no record or one twice, or a quantity is \
         not a decimal, is not above zero or carries more than 8 decimals", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn create_manual_entry(
    State(database): State<Database>,
    Json(body): Json<NewManualEntryBody>,
) -> Result<(StatusCode, Json<ManualEntryBody>), Problem> {
    let repository = database.manual_entries();
    let creation = repository
        .create(
            &Account::from(body.account),
            &Isin::new(body.security),
            &body.supplied.supplied()?,
            &body.answers,
        )
        .await?;
    let status = match creation {
        Creation::Created(_) => StatusCode::CREATED,
        Creation::Existing(_) => StatusCode::OK,
    };
    // Read back rather than echoed: an existing entry equals the posted one as a value, but its
    // quantities keep the digits they were first written with.
    let id = creation.id();
    let stored = repository
        .find(id)
        .await?
        .ok_or(StorageError::UnknownManualEntry { entry: id })?;
    Ok((status, Json(ManualEntryBody::new(id, &stored))))
}

/// The manual entries whose source records are absent, each with the identities it is waiting
/// for [SRV-026], [DOM-109].
#[utoipa::path(
    get,
    path = "/manual-entries/waiting",
    tag = "manual-entries",
    responses((status = 200, description = "Every waiting manual entry", body = [WaitingEntryBody]))
)]
async fn list_waiting_manual_entries(
    State(database): State<Database>,
) -> Result<Json<Vec<WaitingEntryBody>>, Problem> {
    let waiting = database.manual_entries().waiting().await?;
    Ok(Json(waiting.iter().map(WaitingEntryBody::from).collect()))
}

/// Deletes a manual entry, the only thing that removes one [SRV-053].
#[utoipa::path(
    delete,
    path = "/manual-entries/{id}",
    tag = "manual-entries",
    params(("id" = i64, Path, description = "The entry's id")),
    responses(
        (status = 204, description = "The entry is deleted"),
        (status = 404, description = "No such manual entry", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn delete_manual_entry(
    State(database): State<Database>,
    Path(id): Path<i64>,
) -> Result<StatusCode, Problem> {
    database
        .manual_entries()
        .delete(ManualEntryId::new(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
