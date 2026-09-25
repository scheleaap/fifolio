//! Create, read, update, delete and list securities [SRV-007].
//!
//! A security is addressed by its ISIN, normalized as [`Isin::new`] normalizes it, so a path in
//! lowercase names the same security.
//!
//! The wire enums are the server's own rather than serde derives on the core types, because
//! they are a contract a client depends on and carry an OpenAPI schema the core has no reason
//! to know about. Their spellings match the codes storage writes.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use fifolio_core::entities::{Isin, Quotation, Security, SecurityType};
use fifolio_core::storage::{Database, StorageError};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::problem::Problem;

/// What kind of instrument a security is [DOM-004].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SecurityTypeBody {
    Stock,
    Bond,
    Etf,
    Fund,
    Derivative,
    Other,
}

impl From<SecurityType> for SecurityTypeBody {
    fn from(value: SecurityType) -> Self {
        match value {
            SecurityType::Stock => Self::Stock,
            SecurityType::Bond => Self::Bond,
            SecurityType::Etf => Self::Etf,
            SecurityType::Fund => Self::Fund,
            SecurityType::Derivative => Self::Derivative,
            SecurityType::Other => Self::Other,
        }
    }
}

impl From<SecurityTypeBody> for SecurityType {
    fn from(value: SecurityTypeBody) -> Self {
        match value {
            SecurityTypeBody::Stock => Self::Stock,
            SecurityTypeBody::Bond => Self::Bond,
            SecurityTypeBody::Etf => Self::Etf,
            SecurityTypeBody::Fund => Self::Fund,
            SecurityTypeBody::Derivative => Self::Derivative,
            SecurityTypeBody::Other => Self::Other,
        }
    }
}

/// How a security's price is expressed [DOM-005].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuotationBody {
    PerUnit,
    PercentOfPar,
}

impl From<Quotation> for QuotationBody {
    fn from(value: Quotation) -> Self {
        match value {
            Quotation::PerUnit => Self::PerUnit,
            Quotation::PercentOfPar => Self::PercentOfPar,
        }
    }
}

impl From<QuotationBody> for Quotation {
    fn from(value: QuotationBody) -> Self {
        match value {
            QuotationBody::PerUnit => Self::PerUnit,
            QuotationBody::PercentOfPar => Self::PercentOfPar,
        }
    }
}

/// A security as returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SecurityBody {
    #[schema(example = "NL0000009538")]
    pub isin: String,
    pub name: String,
    pub security_type: SecurityTypeBody,
    pub quotation: QuotationBody,
    /// Whether an import created the security rather than the user; kept through edits
    /// [DOM-006].
    pub auto_created: bool,
    /// Whether the user has yet to mark the security reviewed. Set on import, cleared only by
    /// `POST /securities/{isin}/reviewed`, and left alone by an edit [DOM-126, SRV-057].
    pub needs_review: bool,
}

impl From<&Security> for SecurityBody {
    fn from(security: &Security) -> Self {
        Self {
            isin: security.isin().as_str().to_owned(),
            name: security.name().to_owned(),
            security_type: security.security_type().into(),
            quotation: security.quotation().into(),
            auto_created: security.is_auto_created(),
            needs_review: security.needs_review(),
        }
    }
}

/// A security the user enters, its quotation stated rather than defaulted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NewSecurity {
    #[schema(example = "NL0000009538")]
    pub isin: String,
    pub name: String,
    pub security_type: SecurityTypeBody,
    pub quotation: QuotationBody,
}

/// Everything about a security but its ISIN, which is its key and not editable, and its two
/// flags: provenance never changes [DOM-006], and needs review is cleared only by marking the
/// security reviewed [DOM-126].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SecurityEdit {
    pub name: String,
    pub security_type: SecurityTypeBody,
    pub quotation: QuotationBody,
}

pub(crate) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_securities, create_security))
        .routes(routes!(get_security, update_security, delete_security))
        .routes(routes!(mark_security_reviewed))
}

/// Every security, ordered by ISIN [SRV-007].
#[utoipa::path(
    get,
    path = "/securities",
    tag = "securities",
    responses((status = 200, description = "Every security", body = [SecurityBody]))
)]
async fn list_securities(
    State(database): State<Database>,
) -> Result<Json<Vec<SecurityBody>>, Problem> {
    let securities = database.securities().list().await?;
    Ok(Json(securities.iter().map(SecurityBody::from).collect()))
}

/// Stores a security the user entered [SRV-007]; an ISIN already stored is a conflict
/// [SRV-010].
#[utoipa::path(
    post,
    path = "/securities",
    tag = "securities",
    request_body = NewSecurity,
    responses(
        (status = 201, description = "The security, as stored", body = SecurityBody),
        (status = 409, description = "A security with this ISIN exists", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn create_security(
    State(database): State<Database>,
    Json(body): Json<NewSecurity>,
) -> Result<(StatusCode, Json<SecurityBody>), Problem> {
    let security = Security::new(
        Isin::new(body.isin),
        body.name,
        body.security_type.into(),
        body.quotation.into(),
    );
    database.securities().insert(&security).await?;
    Ok((StatusCode::CREATED, Json(SecurityBody::from(&security))))
}

/// One security [SRV-007].
#[utoipa::path(
    get,
    path = "/securities/{isin}",
    tag = "securities",
    params(("isin" = String, Path, description = "The ISIN")),
    responses(
        (status = 200, description = "The security", body = SecurityBody),
        (status = 404, description = "No such security", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn get_security(
    State(database): State<Database>,
    Path(isin): Path<String>,
) -> Result<Json<SecurityBody>, Problem> {
    let isin = Isin::new(isin);
    database
        .securities()
        .find(&isin)
        .await?
        .map(|security| Json(SecurityBody::from(&security)))
        .ok_or_else(|| {
            StorageError::UnknownSecurity {
                isin: isin.as_str().to_owned(),
            }
            .into()
        })
}

/// Edits a security's name, type and quotation, which is how an auto-created security is
/// corrected [SRV-011].
#[utoipa::path(
    put,
    path = "/securities/{isin}",
    tag = "securities",
    params(("isin" = String, Path, description = "The ISIN")),
    request_body = SecurityEdit,
    responses(
        (status = 200, description = "The security, as stored", body = SecurityBody),
        (status = 404, description = "No such security", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn update_security(
    State(database): State<Database>,
    Path(isin): Path<String>,
    Json(body): Json<SecurityEdit>,
) -> Result<Json<SecurityBody>, Problem> {
    let security = database
        .securities()
        .update(
            &Isin::new(isin),
            &body.name,
            body.security_type.into(),
            body.quotation.into(),
        )
        .await?;
    Ok(Json(SecurityBody::from(&security)))
}

/// Marks a security reviewed, clearing needs review and nothing else [SRV-057, DOM-126].
///
/// An action rather than a field of the edit, because an edit must leave needs review set.
/// Marking a security that is already reviewed succeeds and changes nothing.
#[utoipa::path(
    post,
    path = "/securities/{isin}/reviewed",
    tag = "securities",
    params(("isin" = String, Path, description = "The ISIN")),
    responses(
        (status = 200, description = "The security, as stored", body = SecurityBody),
        (status = 404, description = "No such security", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn mark_security_reviewed(
    State(database): State<Database>,
    Path(isin): Path<String>,
) -> Result<Json<SecurityBody>, Problem> {
    let security = database
        .securities()
        .mark_reviewed(&Isin::new(isin))
        .await?;
    Ok(Json(SecurityBody::from(&security)))
}

/// Deletes a security, refused while any source record references it [SRV-009].
#[utoipa::path(
    delete,
    path = "/securities/{isin}",
    tag = "securities",
    params(("isin" = String, Path, description = "The ISIN")),
    responses(
        (status = 204, description = "The security is deleted"),
        (status = 404, description = "No such security", body = Problem,
         content_type = "application/problem+json"),
        (status = 409, description = "The security is referenced", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn delete_security(
    State(database): State<Database>,
    Path(isin): Path<String>,
) -> Result<StatusCode, Problem> {
    database.securities().delete(&Isin::new(isin)).await?;
    Ok(StatusCode::NO_CONTENT)
}
