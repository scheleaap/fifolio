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
//!
//! Every import creates a batch [SRV-019], and `/imports` is also where batches are read and
//! listed [SRV-020]. Deleting one answers SRV-022's two refusals; a deletion neither refuses is
//! answered as not supported yet, because removing what the batch owns is FIF-086's and needs
//! the supplier relation FIF-071 stores (DEC-115, provisional).

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use fifolio_core::entities::{Account, ImportBatch, ImportCounts, Isin, SourceFormat};
use fifolio_core::import::Importer;
use fifolio_core::import::trade_republic::TradeRepublic;
use fifolio_core::import_service::import_file;
use fifolio_core::storage::{BatchId, Database, StorageError};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::accounts::AccountBody;
use crate::problem::{Problem, ProblemType};

/// A broker export format [SRV-013], spelled as storage records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FormatBody {
    SaxoNlXlsx,
    TradeRepublicDeCsv,
}

impl From<SourceFormat> for FormatBody {
    fn from(format: SourceFormat) -> Self {
        match format {
            SourceFormat::SaxoNlXlsx => Self::SaxoNlXlsx,
            SourceFormat::TradeRepublicDeCsv => Self::TradeRepublicDeCsv,
        }
    }
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

/// How the file's rows were classified, and how many securities the import created [SRV-017].
///
/// The three row counts add up to the file's rows: there is no failed count, because a file
/// with a failed row is refused rather than imported [SRV-058] (DEC-074).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SummaryBody {
    /// Rows that became a transaction directly.
    pub derived: u32,
    /// Rows that affect holdings but need something the file does not carry.
    pub pending: u32,
    /// Rows recognized as carrying no position effect, and so not stored [SRV-016]; the
    /// unrecognized types' rows among them.
    pub non_position: u32,
    /// Securities this import created, auto-created and needing review [SRV-014]. One already
    /// stored is not created again (DEC-110), so a file posted again counts none.
    pub securities_auto_created: u32,
}

impl SummaryBody {
    fn new(counts: ImportCounts, created: &[Isin]) -> Self {
        Self {
            derived: counts.derived,
            pending: counts.pending,
            non_position: counts.non_position,
            // Each created security is named by a stored row, so this is at most the row count,
            // itself a u32.
            securities_auto_created: u32::try_from(created.len())
                .expect("no more securities than rows"),
        }
    }
}

/// What an import did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ImportedBody {
    /// The import batch this import created [SRV-019].
    pub batch: i64,
    pub summary: SummaryBody,
    /// Every unrecognized row type the file carried, ordered by type, so that a new broker type
    /// is visible on its first appearance [SRV-049]. Those rows are not stored.
    pub unrecognized_types: Vec<UnrecognizedTypeBody>,
}

/// How a batch's file was classified: the three row counts of SRV-017's summary. The
/// securities auto-created are the import response's alone, not the batch's (DEC-112,
/// provisional).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BatchCountsBody {
    pub derived: u32,
    pub pending: u32,
    pub non_position: u32,
}

/// One import of one file into one account [DOM-017], [SRV-020].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BatchBody {
    /// The id the import answered with [SRV-019].
    pub id: i64,
    pub account: AccountBody,
    pub filename: String,
    #[schema(inline)]
    pub format: FormatBody,
    /// When the file was imported.
    pub imported_at: DateTime<Utc>,
    pub counts: BatchCountsBody,
}

impl BatchBody {
    fn new(id: BatchId, batch: &ImportBatch) -> Self {
        let counts = batch.counts();
        Self {
            id: id.get(),
            account: AccountBody::from(batch.account()),
            filename: batch.filename().to_owned(),
            format: batch.format().into(),
            imported_at: batch.imported_at(),
            counts: BatchCountsBody {
                derived: counts.derived,
                pending: counts.pending,
                non_position: counts.non_position,
            },
        }
    }
}

pub(crate) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(import, list_batches))
        .routes(routes!(get_batch, delete_batch))
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
    let summary = SummaryBody::new(imported.import().counts(), imported.created());
    Ok((
        StatusCode::CREATED,
        Json(ImportedBody {
            batch: imported.batch().get(),
            summary,
            unrecognized_types,
        }),
    ))
}

/// Every import batch, in the order the files were imported [SRV-020].
#[utoipa::path(
    get,
    path = "/imports",
    tag = "imports",
    responses((status = 200, description = "Every import batch", body = [BatchBody]))
)]
async fn list_batches(State(database): State<Database>) -> Result<Json<Vec<BatchBody>>, Problem> {
    let batches = database.import_batches().list().await?;
    Ok(Json(
        batches
            .iter()
            .map(|(id, batch)| BatchBody::new(*id, batch))
            .collect(),
    ))
}

/// One import batch [SRV-020].
#[utoipa::path(
    get,
    path = "/imports/{batch}",
    tag = "imports",
    params(("batch" = i64, Path, description = "The batch id an import answered with")),
    responses(
        (status = 200, description = "The import batch", body = BatchBody),
        (status = 404, description = "No such import batch", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn get_batch(
    State(database): State<Database>,
    Path(batch): Path<i64>,
) -> Result<Json<BatchBody>, Problem> {
    let batch = BatchId::new(batch);
    database
        .import_batches()
        .find(batch)
        .await?
        .map(|stored| Json(BatchBody::new(batch, &stored)))
        .ok_or_else(|| StorageError::UnknownBatch { batch }.into())
}

/// Deletes an import batch, refused while a transaction derived from it participates in an
/// attribution or while a record it owns is cited by a transaction it did not derive, the
/// refusal naming those transactions [SRV-022].
///
/// A deletion neither ground refuses is not carried out: removing what the batch owns is
/// SRV-021's (FIF-086), and removing it before ownership can return to a remaining supplier
/// (FIF-071) would delete records a re-import also supplied (DEC-115, provisional).
#[utoipa::path(
    delete,
    path = "/imports/{batch}",
    tag = "imports",
    params(("batch" = i64, Path, description = "The batch id an import answered with")),
    responses(
        (status = 404, description = "No such import batch", body = Problem,
         content_type = "application/problem+json"),
        (status = 409, description = "The batch derived attributed transactions, or owns \
         records transactions it did not derive cite; the detail names them", body = Problem,
         content_type = "application/problem+json"),
        (status = 501, description = "Nothing refuses the deletion, but removing a batch is \
         not supported yet", body = Problem, content_type = "application/problem+json"),
    )
)]
async fn delete_batch(State(database): State<Database>, Path(batch): Path<i64>) -> Problem {
    let batch = BatchId::new(batch);
    database
        .import_batches()
        .check_deletable(batch)
        .await
        .map_or_else(Problem::from, |()| {
            Problem::new(
                ProblemType::BatchRemovalNotSupported,
                format!(
                    "nothing refuses deleting batch {batch}, but removing its records is not \
                     supported until FIF-086"
                ),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each count reaches its own field: the endpoint's Trade Republic files yield no pending
    /// row, so only distinct values here tell the fields apart [SRV-017].
    #[test]
    fn the_summary_carries_each_count_in_its_own_field() {
        let counts = ImportCounts {
            derived: 3,
            pending: 2,
            non_position: 5,
        };
        let created = [Isin::new("XF0000000079")];

        assert_eq!(
            SummaryBody::new(counts, &created),
            SummaryBody {
                derived: 3,
                pending: 2,
                non_position: 5,
                securities_auto_created: 1,
            }
        );
    }
}
