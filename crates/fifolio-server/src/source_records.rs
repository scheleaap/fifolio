//! Reading and listing source records [SRV-023].
//!
//! The list with `status=pending` is the completion queue, the main thing the interactive client
//! works through [SRV-024]. What consumed and pending mean before a consumption relation exists
//! is DEC-119, and what each filter reaches is DEC-120 (both provisional).
//!
//! There is no route that edits a record: source records are never edited [SRV-027]. A mistake
//! is corrected by deleting the derived transaction and the manual entry, then supplying a new
//! one. A record is addressed by its identity, which carries its account's id; a Saxo id's slash
//! is percent-encoded as `%2F` in the path, as for accounts.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use fifolio_core::entities::{Account, Isin};
use fifolio_core::storage::{
    BatchId, Database, RecordFilter, RecordStatus, StorageError, StoredSourceRecord,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::accounts::AccountBody;
use crate::problem::Problem;

/// Whether a transaction has answered the record, or it still waits for the user [DOM-045].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordStatusBody {
    Pending,
    Consumed,
}

impl From<RecordStatus> for RecordStatusBody {
    fn from(status: RecordStatus) -> Self {
        match status {
            RecordStatus::Pending => Self::Pending,
            RecordStatus::Consumed => Self::Consumed,
        }
    }
}

impl From<RecordStatusBody> for RecordStatus {
    fn from(status: RecordStatusBody) -> Self {
        match status {
            RecordStatusBody::Pending => Self::Pending,
            RecordStatusBody::Consumed => Self::Consumed,
        }
    }
}

/// One row of a broker export as it was imported [DOM-007].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SourceRecordBody {
    /// The record's identity, scoped to its account [DOM-024].
    pub identity: String,
    pub account: AccountBody,
    /// The import batch that owns the record: the newest that supplied it [SRV-052].
    pub batch: i64,
    /// The record's position in its file's canonical order [DOM-111].
    pub order: u32,
    /// The row exactly as the file held it.
    pub raw: String,
    /// The parsed fields, under the format's own column names.
    pub fields: BTreeMap<String, String>,
    #[schema(inline)]
    pub status: RecordStatusBody,
}

impl From<&StoredSourceRecord> for SourceRecordBody {
    fn from(stored: &StoredSourceRecord) -> Self {
        let record = stored.record();
        Self {
            identity: record.identity().as_str().to_owned(),
            account: AccountBody::from(stored.account()),
            batch: stored.owner().get(),
            order: record.order().get(),
            raw: record.raw().to_owned(),
            fields: record.parsed().clone(),
            status: stored.status().into(),
        }
    }
}

/// Which records to list [SRV-023]. Every filter is optional; those given must all hold.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SourceRecordQuery {
    /// The account's broker; given together with `account`.
    #[param(example = "Trade Republic")]
    pub broker: Option<String>,
    /// The account's id; given together with `broker`.
    #[param(example = "DE0001")]
    pub account: Option<String>,
    /// The import batch owning the record.
    pub batch: Option<i64>,
    /// An ISIN one of the record's parsed values names.
    #[param(example = "NL0000009538")]
    pub security: Option<String>,
    /// `pending` for the completion queue [SRV-024].
    #[param(inline)]
    pub status: Option<RecordStatusBody>,
}

impl SourceRecordQuery {
    /// The filter this query states, or a 400 for an account named by half: a broker alone or an
    /// id alone names no account, and guessing the other half would list the wrong records.
    fn filter(self) -> Result<RecordFilter, Problem> {
        let account = match (self.broker, self.account) {
            (Some(broker), Some(id)) => Some(Account::new(broker, id)),
            (None, None) => None,
            _ => {
                return Err(Problem::status_only(
                    StatusCode::BAD_REQUEST,
                    Some("broker and account filter together, naming one account".to_owned()),
                ));
            }
        };
        Ok(RecordFilter {
            account,
            batch: self.batch.map(BatchId::new),
            security: self.security.map(Isin::new),
            status: self.status.map(RecordStatus::from),
        })
    }
}

pub(crate) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_source_records))
        .routes(routes!(get_source_record))
}

/// Every source record the filters admit, in import order and then file order [SRV-023]; with
/// `status=pending`, the completion queue [SRV-024].
#[utoipa::path(
    get,
    path = "/source-records",
    tag = "source-records",
    params(SourceRecordQuery),
    responses(
        (status = 200, description = "The records the filters admit", body = [SourceRecordBody]),
        (status = 400, description = "A broker without an account id, or the reverse",
         body = Problem, content_type = "application/problem+json"),
        (status = 404, description = "The account, batch or security filtered on is not stored",
         body = Problem, content_type = "application/problem+json"),
    )
)]
async fn list_source_records(
    State(database): State<Database>,
    Query(query): Query<SourceRecordQuery>,
) -> Result<Json<Vec<SourceRecordBody>>, Problem> {
    let records = database.source_records().list(&query.filter()?).await?;
    Ok(Json(records.iter().map(SourceRecordBody::from).collect()))
}

/// One source record [SRV-023].
#[utoipa::path(
    get,
    path = "/source-records/{identity}",
    tag = "source-records",
    params(("identity" = String, Path, description = "The record's identity, percent-encoded")),
    responses(
        (status = 200, description = "The source record", body = SourceRecordBody),
        (status = 404, description = "No such source record", body = Problem,
         content_type = "application/problem+json"),
    )
)]
async fn get_source_record(
    State(database): State<Database>,
    Path(identity): Path<String>,
) -> Result<Json<SourceRecordBody>, Problem> {
    database
        .source_records()
        .read(&identity)
        .await?
        .map(|stored| Json(SourceRecordBody::from(&stored)))
        .ok_or_else(|| StorageError::UnknownRecord { identity }.into())
}
