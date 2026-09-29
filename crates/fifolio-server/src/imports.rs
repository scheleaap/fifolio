//! Importing a broker file into an account [SRV-012].
//!
//! The file is the request body, sent as it is; the target account, the format and the filename
//! the batch records are query parameters. The caller names the account explicitly because
//! source files rarely identify it reliably [SRV-012], and names the format rather than having
//! it sniffed.
//!
//! Of the two formats [SRV-013], only Trade Republic DE CSV imports today. A Saxo NL XLSX file is
//! refused as a format not supported yet: its rows cannot be classified until the Saxo importer
//! can (FIF-023), and an import that stored them unclassified would be a guess.
//!
//! What the import stores and what posting a file twice does is
//! [`fifolio_core::import_service`]'s. A refusal names every ground and every failed row
//! [SRV-058], [SRV-059]; a sell exceeding the holdings is not a refusal [SRV-018].

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use chrono::Utc;
use fifolio_core::entities::Account;
use fifolio_core::import::Importer;
use fifolio_core::import::trade_republic::TradeRepublic;
use fifolio_core::import_service::import_file;
use fifolio_core::storage::Database;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::problem::{Problem, ProblemType};

/// A broker export format [SRV-013], spelled as storage records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FormatBody {
    SaxoNlXlsx,
    TradeRepublicDeCsv,
}

/// Where the file goes and what it is [SRV-012].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ImportTarget {
    /// The target account's broker.
    #[param(example = "Trade Republic")]
    pub broker: String,
    /// The target account's id.
    #[param(example = "DE0001")]
    pub account: String,
    /// The file's format.
    #[param(inline)]
    pub format: FormatBody,
    /// The file's name, which the import batch records [SRV-020].
    #[param(example = "transactions_2024-01-01_2024-12-31.csv")]
    pub filename: String,
}

/// A row type the importer did not recognize, and how many rows of the file carried it
/// [SRV-049].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UnrecognizedTypeBody {
    /// The format's own name for the type.
    #[schema(example = "CASH/NEW_TYPE")]
    pub row_type: String,
    pub rows: u32,
}

/// What an import did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ImportedBody {
    /// The import batch this import created [SRV-019].
    pub batch: i64,
    /// Every unrecognized row type the file carried, ordered by type, so that a new broker type
    /// is visible on its first appearance [SRV-049]. Those rows are not stored.
    pub unrecognized_types: Vec<UnrecognizedTypeBody>,
}

pub(crate) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(import))
}

/// The importer for `format`, or the refusal of a format that cannot be imported yet.
fn importer(format: FormatBody) -> Result<&'static (dyn Importer + Sync), Problem> {
    match format {
        FormatBody::TradeRepublicDeCsv => Ok(&TradeRepublic),
        FormatBody::SaxoNlXlsx => Err(Problem::new(
            ProblemType::FormatNotSupported,
            "Saxo NL XLSX files cannot be imported until the Saxo importer classifies their rows \
             (FIF-023)",
        )),
    }
}

/// Imports the file in the body into the account named, or refuses it whole [SRV-012],
/// [SRV-058].
#[utoipa::path(
    post,
    path = "/imports",
    tag = "imports",
    params(ImportTarget),
    request_body(content = Vec<u8>, content_type = "application/octet-stream",
                 description = "The broker file, as exported"),
    responses(
        (status = 201, description = "The file is imported", body = ImportedBody),
        (status = 404, description = "No such account", body = Problem,
         content_type = "application/problem+json"),
        (status = 422, description = "The file is unreadable, refused, or of a format not \
         supported yet; the detail names every ground and every failed row", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn import(
    State(database): State<Database>,
    Query(target): Query<ImportTarget>,
    file: Bytes,
) -> Result<(StatusCode, Json<ImportedBody>), Problem> {
    let importer = importer(target.format)?;
    let account = Account::new(target.broker, target.account);
    let imported = import_file(
        &database,
        importer,
        &account,
        &target.filename,
        &file,
        Utc::now(),
    )
    .await?;
    let unrecognized_types = imported
        .import()
        .unrecognized_types()
        .iter()
        .map(|(row_type, &rows)| UnrecognizedTypeBody {
            row_type: row_type.clone(),
            rows,
        })
        .collect();
    Ok((
        StatusCode::CREATED,
        Json(ImportedBody {
            batch: imported.batch().get(),
            unrecognized_types,
        }),
    ))
}
