//! RFC 9457 problem details: the one shape every error response takes [ARC-020].
//!
//! Two routes lead here. A handler returns a [`Problem`] built from a core error, whose `type`
//! is the [`ProblemType`] that error's variant maps to [ARC-021]. Everything else — an unrouted
//! path, a method a route does not serve, an extractor's rejection — is a response axum writes
//! on its own, and [`problem_for_bare_errors`] rewrites it on the way out, so no error response
//! leaves the server in another shape whoever produced it.
//!
//! # The mapping
//!
//! Every mapping below is an exhaustive `match` with no wildcard arm, so a variant added to a
//! core error does not compile until it is given a type: each variant maps to exactly one type
//! because the compiler checks it. A wrapper that only forwards another error
//! ([`IngestError`]'s two arms) is not a class of its own and takes the type of what it wraps.
//!
//! That guarantee covers the core errors a handler can hold. A format's own error
//! (`SaxoError`, `TradeRepublicError`) is not among them and has no type here: the server
//! imports through `fifolio_core::import::import`, whose [`Importer`] trait answers only
//! `RowError` and [`ImportError`], so a format error reaches a handler as an [`ImportError`]
//! or not at all. The signature is what enforces it.
//!
//! [`Importer`]: fifolio_core::import::Importer
//!
//! # The `type` URIs
//!
//! `urn:fifolio:problem:<slug>`. RFC 9457 §3.1.1 asks for an absolute URI and does not require
//! it to resolve; this server is local and has no host a dereferenceable URL could name, so the
//! URI is an identifier only. The slugs are written out rather than derived from the variant
//! names, because they are a wire contract the CLI maps to translated messages [CLI-036], and a
//! Rust rename must not change them. A response carrying nothing beyond its status is typed
//! `about:blank`, which RFC 9457 §4.2.1 reserves for exactly that.

use axum::Json;
use axum::body::to_bytes;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use fifolio_core::ecb::{FeedError, IngestError};
use fifolio_core::fx::RateError;
use fifolio_core::import::ImportError;
use fifolio_core::storage::StorageError;
use serde::{Serialize, Serializer};
use utoipa::ToSchema;

/// The media type of every error response [ARC-020].
pub const CONTENT_TYPE: &str = "application/problem+json";

const TYPE_PREFIX: &str = "urn:fifolio:problem:";

/// RFC 9457 §4.2.1: the type of a problem with no semantics beyond its status code.
pub const ABOUT_BLANK: &str = "about:blank";

/// A bare error body is carried into `detail`; one larger than this is not a message a
/// framework rejection writes, and is dropped rather than buffered.
const DETAIL_LIMIT: usize = 64 * 1024;

/// A class of error, stable on the wire [ARC-021].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum ProblemType {
    DuplicateIsin,
    DuplicateAccount,
    UnknownAccount,
    UnknownSecurity,
    AccountReferenced,
    SecurityReferenced,
    UnscaledValue,
    CorruptValue,
    UnknownTransaction,
    NotAClosing,
    WrongTransactionKind,
    ClosingAlreadyAttributed,
    EarlierClosingUnattributed,
    LaterAttributionExists,
    TransactionAttributed,
    EmittedTransferIn,
    BatchTransactionAttributed,
    BatchRecordsCited,
    StorageFailure,
    UnreadableFile,
    UnorderableRow,
    MultipleCalendarYears,
    ImporterDefect,
    RateBeforeSeries,
    RateUnavailable,
    RateStale,
    RateFeedUnreadable,
    RateFeedMalformed,
}

impl ProblemType {
    /// The slug, the status and the title, which RFC 9457 §3.1.3 says does not vary between
    /// occurrences; what varies goes in `detail`.
    ///
    /// Statuses: 409 for a refusal that depends on what is stored, 422 for a request whose
    /// content cannot be acted on, 404 for a row that is not there, 502 for the ECB feed
    /// failing us, and 500 for what no request could have caused.
    fn spec(self) -> (&'static str, StatusCode, &'static str) {
        use StatusCode as S;
        match self {
            Self::DuplicateIsin => (
                "duplicate-isin",
                S::CONFLICT,
                "A security with this ISIN already exists",
            ),
            Self::DuplicateAccount => (
                "duplicate-account",
                S::CONFLICT,
                "An account with this broker and id already exists",
            ),
            Self::UnknownAccount => ("unknown-account", S::NOT_FOUND, "No such account"),
            Self::UnknownSecurity => ("unknown-security", S::NOT_FOUND, "No such security"),
            Self::AccountReferenced => (
                "account-referenced",
                S::CONFLICT,
                "The account is referenced by stored records",
            ),
            Self::SecurityReferenced => (
                "security-referenced",
                S::CONFLICT,
                "The security is referenced by stored records",
            ),
            Self::UnscaledValue => (
                "unscaled-value",
                S::UNPROCESSABLE_ENTITY,
                "A value carries more decimals than it is stored at",
            ),
            Self::CorruptValue => (
                "corrupt-value",
                S::INTERNAL_SERVER_ERROR,
                "A stored value cannot be read back",
            ),
            Self::UnknownTransaction => {
                ("unknown-transaction", S::NOT_FOUND, "No such transaction")
            }
            Self::NotAClosing => (
                "not-a-closing",
                S::UNPROCESSABLE_ENTITY,
                "The transaction is not a closing",
            ),
            Self::WrongTransactionKind => (
                "wrong-transaction-kind",
                S::UNPROCESSABLE_ENTITY,
                "The transaction is of the wrong kind",
            ),
            Self::ClosingAlreadyAttributed => (
                "closing-already-attributed",
                S::CONFLICT,
                "The closing is already attributed",
            ),
            Self::EarlierClosingUnattributed => (
                "earlier-closing-unattributed",
                S::CONFLICT,
                "An earlier closing is unattributed",
            ),
            Self::LaterAttributionExists => (
                "later-attribution-exists",
                S::CONFLICT,
                "A later attribution exists",
            ),
            Self::TransactionAttributed => (
                "transaction-attributed",
                S::CONFLICT,
                "The transaction participates in an attribution",
            ),
            Self::EmittedTransferIn => (
                "emitted-transfer-in",
                S::CONFLICT,
                "The transfer_in was emitted by a transfer_out",
            ),
            Self::BatchTransactionAttributed => (
                "batch-transaction-attributed",
                S::CONFLICT,
                "The batch derived attributed transactions",
            ),
            Self::BatchRecordsCited => (
                "batch-records-cited",
                S::CONFLICT,
                "The batch owns records other transactions cite",
            ),
            Self::StorageFailure => (
                "storage-failure",
                S::INTERNAL_SERVER_ERROR,
                "The database failed",
            ),
            Self::UnreadableFile => (
                "unreadable-file",
                S::UNPROCESSABLE_ENTITY,
                "The file could not be read",
            ),
            Self::UnorderableRow => (
                "unorderable-row",
                S::UNPROCESSABLE_ENTITY,
                "A row cannot be ordered",
            ),
            Self::MultipleCalendarYears => (
                "multiple-calendar-years",
                S::UNPROCESSABLE_ENTITY,
                "The file spans more than one calendar year",
            ),
            Self::ImporterDefect => (
                "importer-defect",
                S::INTERNAL_SERVER_ERROR,
                "The importer lost track of the file's rows",
            ),
            Self::RateBeforeSeries => (
                "rate-before-series",
                S::UNPROCESSABLE_ENTITY,
                "No rate exists before the ECB series begins",
            ),
            Self::RateUnavailable => (
                "rate-unavailable",
                S::UNPROCESSABLE_ENTITY,
                "No rate is available",
            ),
            Self::RateStale => (
                "rate-stale",
                S::UNPROCESSABLE_ENTITY,
                "The nearest rate is too stale to substitute",
            ),
            Self::RateFeedUnreadable => (
                "rate-feed-unreadable",
                S::BAD_GATEWAY,
                "The ECB feed could not be read",
            ),
            Self::RateFeedMalformed => (
                "rate-feed-malformed",
                S::BAD_GATEWAY,
                "The ECB feed returned a malformed document",
            ),
        }
    }

    /// The `type` member.
    #[must_use]
    pub fn uri(self) -> String {
        format!("{TYPE_PREFIX}{}", self.spec().0)
    }

    #[must_use]
    pub fn status(self) -> StatusCode {
        self.spec().1
    }

    #[must_use]
    pub fn title(self) -> &'static str {
        self.spec().2
    }
}

impl From<&StorageError> for ProblemType {
    fn from(error: &StorageError) -> Self {
        match error {
            StorageError::DuplicateIsin { .. } => Self::DuplicateIsin,
            StorageError::DuplicateAccount { .. } => Self::DuplicateAccount,
            StorageError::UnknownAccount { .. } => Self::UnknownAccount,
            StorageError::UnknownSecurity { .. } => Self::UnknownSecurity,
            StorageError::AccountReferenced { .. } => Self::AccountReferenced,
            StorageError::SecurityReferenced { .. } => Self::SecurityReferenced,
            StorageError::UnscaledValue { .. } => Self::UnscaledValue,
            StorageError::CorruptValue { .. } => Self::CorruptValue,
            StorageError::UnknownTransaction { .. } => Self::UnknownTransaction,
            StorageError::NotAClosing { .. } => Self::NotAClosing,
            StorageError::NotOfKind { .. } => Self::WrongTransactionKind,
            StorageError::ClosingAlreadyAttributed { .. } => Self::ClosingAlreadyAttributed,
            StorageError::EarlierClosingUnattributed { .. } => Self::EarlierClosingUnattributed,
            StorageError::LaterAttributionExists { .. } => Self::LaterAttributionExists,
            StorageError::TransactionAttributed { .. } => Self::TransactionAttributed,
            StorageError::EmittedTransferIn { .. } => Self::EmittedTransferIn,
            StorageError::BatchTransactionAttributed { .. } => Self::BatchTransactionAttributed,
            StorageError::BatchRecordsCited { .. } => Self::BatchRecordsCited,
            // Neither is something a caller can act on: the driver failed, or the schema could
            // not be brought current, which only happens at startup.
            StorageError::Database(_) | StorageError::Migration(_) => Self::StorageFailure,
        }
    }
}

impl From<&ImportError> for ProblemType {
    fn from(error: &ImportError) -> Self {
        match error {
            ImportError::Read(_) => Self::UnreadableFile,
            ImportError::Unorderable { .. } => Self::UnorderableRow,
            ImportError::MultipleCalendarYears { .. } => Self::MultipleCalendarYears,
            // The importer answered the wrong number of rows: a defect here, not in the file.
            ImportError::ClassificationCount { .. } => Self::ImporterDefect,
        }
    }
}

impl From<&RateError> for ProblemType {
    fn from(error: &RateError) -> Self {
        match error {
            RateError::BeforeSeries { .. } => Self::RateBeforeSeries,
            RateError::Unavailable { .. } => Self::RateUnavailable,
            RateError::StaleSubstitute { .. } => Self::RateStale,
        }
    }
}

impl From<&FeedError> for ProblemType {
    fn from(error: &FeedError) -> Self {
        match error {
            FeedError::Unreadable { .. } => Self::RateFeedUnreadable,
            FeedError::Malformed { .. } => Self::RateFeedMalformed,
        }
    }
}

impl From<&IngestError> for ProblemType {
    fn from(error: &IngestError) -> Self {
        match error {
            IngestError::Feed(error) => error.into(),
            IngestError::Storage(error) => error.into(),
        }
    }
}

/// An RFC 9457 problem details object [ARC-020].
///
/// `instance` is not carried: nothing here identifies an occurrence beyond the request itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Problem {
    /// `urn:fifolio:problem:<slug>`, or `about:blank` for a problem that is only its status.
    #[serde(rename = "type")]
    #[schema(example = "urn:fifolio:problem:duplicate-isin")]
    problem_type: String,
    /// Fixed per `type`.
    title: String,
    /// The HTTP status, repeated in the body as RFC 9457 §3.1.2 allows.
    #[serde(serialize_with = "status_code")]
    #[schema(value_type = u16, minimum = 400, maximum = 599)]
    status: StatusCode,
    /// This occurrence, in English.
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

fn status_code<S: Serializer>(status: &StatusCode, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u16(status.as_u16())
}

impl Problem {
    #[must_use]
    pub fn new(problem_type: ProblemType, detail: impl Into<String>) -> Self {
        Self {
            problem_type: problem_type.uri(),
            title: problem_type.title().to_owned(),
            status: problem_type.status(),
            detail: Some(detail.into()),
        }
    }

    /// A problem that is its status and nothing more, titled with the status phrase as RFC
    /// 9457 §4.2.1 asks of `about:blank`.
    #[must_use]
    pub fn status_only(status: StatusCode, detail: Option<String>) -> Self {
        Self {
            problem_type: ABOUT_BLANK.to_owned(),
            title: status.canonical_reason().unwrap_or("Error").to_owned(),
            status,
            detail,
        }
    }
}

/// A core error as a problem: its variant decides the type, its message is the detail.
macro_rules! problem_from {
    ($($error:ty),* $(,)?) => {$(
        impl From<$error> for Problem {
            fn from(error: $error) -> Self {
                Self::new(ProblemType::from(&error), error.to_string())
            }
        }
    )*};
}

problem_from!(StorageError, ImportError, RateError, FeedError, IngestError);

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let status = self.status;
        // The header array is applied after `Json` has set its own content type, and replaces it.
        (
            status,
            [(header::CONTENT_TYPE, HeaderValue::from_static(CONTENT_TYPE))],
            Json(self),
        )
            .into_response()
    }
}

/// Rewrites an error response no handler wrote as a problem, so ARC-020 holds for what axum
/// answers on its own: an unrouted path, a method the route does not serve, a rejected body.
///
/// The original body, a plain-text rejection message where there is one, becomes `detail`. The
/// original headers are kept apart from the two that describe the replaced body, since some are
/// required of the status: a 405 must carry `Allow` (RFC 9110 §15.5.6).
pub async fn problem_for_bare_errors(response: Response) -> Response {
    let status = response.status();
    let is_problem = response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|value| value.as_bytes().starts_with(CONTENT_TYPE.as_bytes()));
    if !(status.is_client_error() || status.is_server_error()) || is_problem {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let detail = to_bytes(body, DETAIL_LIMIT)
        .await
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned())
        .filter(|detail| !detail.is_empty());
    parts.headers.remove(header::CONTENT_TYPE);
    parts.headers.remove(header::CONTENT_LENGTH);

    let mut problem = Problem::status_only(status, detail).into_response();
    problem.headers_mut().extend(parts.headers);
    problem
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::middleware::map_response;
    use axum::routing::post;
    use chrono::NaiveDate;
    use fifolio_core::import::reader::ReadError;
    use fifolio_core::storage::{AttributionId, BatchId, TransactionId};
    use fifolio_core::valuation::Currency;
    use http_body_util::BodyExt;
    use strum::IntoEnumIterator;
    use tower::ServiceExt;

    use super::*;

    /// The wire contract, written out: a type that changes breaks every client that maps it, so
    /// renaming a variant or re-slugging one must fail here [ARC-021].
    const PINNED: &[(ProblemType, &str, u16)] = &[
        (
            ProblemType::DuplicateIsin,
            "urn:fifolio:problem:duplicate-isin",
            409,
        ),
        (
            ProblemType::DuplicateAccount,
            "urn:fifolio:problem:duplicate-account",
            409,
        ),
        (
            ProblemType::UnknownAccount,
            "urn:fifolio:problem:unknown-account",
            404,
        ),
        (
            ProblemType::UnknownSecurity,
            "urn:fifolio:problem:unknown-security",
            404,
        ),
        (
            ProblemType::AccountReferenced,
            "urn:fifolio:problem:account-referenced",
            409,
        ),
        (
            ProblemType::SecurityReferenced,
            "urn:fifolio:problem:security-referenced",
            409,
        ),
        (
            ProblemType::UnscaledValue,
            "urn:fifolio:problem:unscaled-value",
            422,
        ),
        (
            ProblemType::CorruptValue,
            "urn:fifolio:problem:corrupt-value",
            500,
        ),
        (
            ProblemType::UnknownTransaction,
            "urn:fifolio:problem:unknown-transaction",
            404,
        ),
        (
            ProblemType::NotAClosing,
            "urn:fifolio:problem:not-a-closing",
            422,
        ),
        (
            ProblemType::WrongTransactionKind,
            "urn:fifolio:problem:wrong-transaction-kind",
            422,
        ),
        (
            ProblemType::ClosingAlreadyAttributed,
            "urn:fifolio:problem:closing-already-attributed",
            409,
        ),
        (
            ProblemType::EarlierClosingUnattributed,
            "urn:fifolio:problem:earlier-closing-unattributed",
            409,
        ),
        (
            ProblemType::LaterAttributionExists,
            "urn:fifolio:problem:later-attribution-exists",
            409,
        ),
        (
            ProblemType::TransactionAttributed,
            "urn:fifolio:problem:transaction-attributed",
            409,
        ),
        (
            ProblemType::EmittedTransferIn,
            "urn:fifolio:problem:emitted-transfer-in",
            409,
        ),
        (
            ProblemType::BatchTransactionAttributed,
            "urn:fifolio:problem:batch-transaction-attributed",
            409,
        ),
        (
            ProblemType::BatchRecordsCited,
            "urn:fifolio:problem:batch-records-cited",
            409,
        ),
        (
            ProblemType::StorageFailure,
            "urn:fifolio:problem:storage-failure",
            500,
        ),
        (
            ProblemType::UnreadableFile,
            "urn:fifolio:problem:unreadable-file",
            422,
        ),
        (
            ProblemType::UnorderableRow,
            "urn:fifolio:problem:unorderable-row",
            422,
        ),
        (
            ProblemType::MultipleCalendarYears,
            "urn:fifolio:problem:multiple-calendar-years",
            422,
        ),
        (
            ProblemType::ImporterDefect,
            "urn:fifolio:problem:importer-defect",
            500,
        ),
        (
            ProblemType::RateBeforeSeries,
            "urn:fifolio:problem:rate-before-series",
            422,
        ),
        (
            ProblemType::RateUnavailable,
            "urn:fifolio:problem:rate-unavailable",
            422,
        ),
        (
            ProblemType::RateStale,
            "urn:fifolio:problem:rate-stale",
            422,
        ),
        (
            ProblemType::RateFeedUnreadable,
            "urn:fifolio:problem:rate-feed-unreadable",
            502,
        ),
        (
            ProblemType::RateFeedMalformed,
            "urn:fifolio:problem:rate-feed-malformed",
            502,
        ),
    ];

    /// Every type is pinned, and each to a distinct URI [ARC-021].
    #[test]
    fn every_type_has_its_pinned_uri_and_status() {
        let pinned: HashSet<ProblemType> = PINNED.iter().map(|(kind, ..)| *kind).collect();
        assert_eq!(pinned, ProblemType::iter().collect::<HashSet<_>>());
        assert_eq!(pinned.len(), PINNED.len(), "a type is pinned twice");

        for (kind, uri, status) in PINNED {
            assert_eq!(
                (kind.uri().as_str(), kind.status().as_u16()),
                (*uri, *status),
                "{kind:?}"
            );
            assert!(!kind.title().is_empty());
        }
        let uris: HashSet<&str> = PINNED.iter().map(|(_, uri, _)| *uri).collect();
        assert_eq!(uris.len(), PINNED.len(), "two types share a URI");
    }

    fn transaction(id: i64) -> TransactionId {
        TransactionId::new(id)
    }

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 3, 1).expect("a date")
    }

    /// One value of every core error variant and the type it answers with. The compiler proves
    /// each variant has exactly one type; this pins which [ARC-021].
    #[test]
    fn each_core_error_variant_maps_to_its_type() {
        let attribution = AttributionId::new(7);
        let storage = [
            (
                StorageError::DuplicateIsin {
                    isin: "NL0000009538".into(),
                },
                ProblemType::DuplicateIsin,
            ),
            (
                StorageError::DuplicateAccount {
                    broker: "Saxo".into(),
                    id: "1".into(),
                },
                ProblemType::DuplicateAccount,
            ),
            (
                StorageError::UnknownAccount {
                    broker: "Saxo".into(),
                    id: "1".into(),
                },
                ProblemType::UnknownAccount,
            ),
            (
                StorageError::UnknownSecurity {
                    isin: "NL0000009538".into(),
                },
                ProblemType::UnknownSecurity,
            ),
            (
                StorageError::AccountReferenced {
                    broker: "Saxo".into(),
                    id: "1".into(),
                    source_records: 1,
                    batches: 1,
                    manual_entries: 0,
                    transactions: 0,
                },
                ProblemType::AccountReferenced,
            ),
            (
                StorageError::SecurityReferenced {
                    isin: "NL0000009538".into(),
                    source_records: 1,
                    transactions: 0,
                },
                ProblemType::SecurityReferenced,
            ),
            (
                StorageError::UnscaledValue {
                    field: "quantity",
                    value: "1.123456789".into(),
                    scale: 8,
                },
                ProblemType::UnscaledValue,
            ),
            (
                StorageError::CorruptValue {
                    field: "kind",
                    value: "?".into(),
                },
                ProblemType::CorruptValue,
            ),
            (
                StorageError::UnknownTransaction {
                    transaction: transaction(1),
                },
                ProblemType::UnknownTransaction,
            ),
            (
                StorageError::NotAClosing {
                    transaction: transaction(1),
                    kind: "buy".into(),
                },
                ProblemType::NotAClosing,
            ),
            (
                StorageError::NotOfKind {
                    transaction: transaction(1),
                    expected: "transfer_out",
                    kind: "buy".into(),
                },
                ProblemType::WrongTransactionKind,
            ),
            (
                StorageError::ClosingAlreadyAttributed {
                    closing: transaction(1),
                    attribution,
                },
                ProblemType::ClosingAlreadyAttributed,
            ),
            (
                StorageError::EarlierClosingUnattributed {
                    closing: transaction(2),
                    earlier: transaction(1),
                },
                ProblemType::EarlierClosingUnattributed,
            ),
            (
                StorageError::LaterAttributionExists {
                    attribution,
                    later: AttributionId::new(8),
                },
                ProblemType::LaterAttributionExists,
            ),
            (
                StorageError::TransactionAttributed {
                    transaction: transaction(1),
                    attribution,
                },
                ProblemType::TransactionAttributed,
            ),
            (
                StorageError::EmittedTransferIn {
                    transfer_in: transaction(2),
                    transfer_out: transaction(1),
                },
                ProblemType::EmittedTransferIn,
            ),
            (
                StorageError::BatchTransactionAttributed {
                    batch: BatchId::new(1),
                    transactions: vec![transaction(1)],
                },
                ProblemType::BatchTransactionAttributed,
            ),
            (
                StorageError::BatchRecordsCited {
                    batch: BatchId::new(1),
                    transactions: vec![transaction(1)],
                },
                ProblemType::BatchRecordsCited,
            ),
            (
                StorageError::Database(sqlx::Error::RowNotFound),
                ProblemType::StorageFailure,
            ),
            (
                StorageError::Migration(sqlx::migrate::MigrateError::VersionMissing(1)),
                ProblemType::StorageFailure,
            ),
        ];
        for (error, expected) in &storage {
            assert_eq!(ProblemType::from(error), *expected, "{error:?}");
        }

        let import = [
            (
                ImportError::Read(ReadError::NoHeaderRow),
                ProblemType::UnreadableFile,
            ),
            (
                ImportError::Unorderable {
                    position: 3,
                    reason: "no date".into(),
                },
                ProblemType::UnorderableRow,
            ),
            (
                ImportError::MultipleCalendarYears {
                    years: vec![2023, 2024],
                },
                ProblemType::MultipleCalendarYears,
            ),
            (
                ImportError::ClassificationCount {
                    rows: 2,
                    classified: 1,
                },
                ProblemType::ImporterDefect,
            ),
        ];
        for (error, expected) in &import {
            assert_eq!(ProblemType::from(error), *expected, "{error:?}");
        }

        let usd = Currency::new("USD");
        let rate = [
            (
                RateError::BeforeSeries {
                    currency: usd.clone(),
                    date: date(),
                },
                ProblemType::RateBeforeSeries,
            ),
            (
                RateError::Unavailable {
                    currency: usd.clone(),
                    date: date(),
                },
                ProblemType::RateUnavailable,
            ),
            (
                RateError::StaleSubstitute {
                    currency: usd,
                    date: date(),
                    rate_date: date(),
                },
                ProblemType::RateStale,
            ),
        ];
        for (error, expected) in &rate {
            assert_eq!(ProblemType::from(error), *expected, "{error:?}");
        }

        let unreadable = FeedError::Unreadable {
            reason: "offline".into(),
        };
        let malformed = FeedError::Malformed {
            reason: "not XML".into(),
        };
        assert_eq!(
            ProblemType::from(&unreadable),
            ProblemType::RateFeedUnreadable
        );
        assert_eq!(
            ProblemType::from(&malformed),
            ProblemType::RateFeedMalformed
        );

        // A forwarding wrapper takes the type of what it forwards.
        assert_eq!(
            ProblemType::from(&IngestError::Feed(malformed)),
            ProblemType::RateFeedMalformed
        );
        assert_eq!(
            ProblemType::from(&IngestError::Storage(StorageError::UnscaledValue {
                field: "rate",
                value: "1.1234567".into(),
                scale: 6
            })),
            ProblemType::UnscaledValue
        );
    }

    /// A problem built from a core error carries that error's message as its detail [ARC-020].
    #[test]
    fn a_core_error_becomes_a_problem_with_its_message_as_detail() {
        let problem = Problem::from(StorageError::DuplicateIsin {
            isin: "NL0000009538".into(),
        });
        assert_eq!(
            serde_json::to_value(&problem).expect("serializes"),
            serde_json::json!({
                "type": "urn:fifolio:problem:duplicate-isin",
                "title": "A security with this ISIN already exists",
                "status": 409,
                "detail": "a security with ISIN NL0000009538 already exists",
            })
        );
    }

    /// `about:blank` is titled with the status phrase, and an absent detail is omitted rather
    /// than sent as null [ARC-020].
    #[test]
    fn a_status_only_problem_is_about_blank() {
        assert_eq!(
            serde_json::to_value(Problem::status_only(StatusCode::NOT_FOUND, None))
                .expect("serializes"),
            serde_json::json!({"type": "about:blank", "title": "Not Found", "status": 404})
        );
    }

    async fn post_to(router: Router, content_type: &str, body: &'static str) -> Response {
        router
            .oneshot(
                Request::post("/")
                    .header(header::CONTENT_TYPE, content_type)
                    .body(Body::from(body))
                    .expect("build the request"),
            )
            .await
            .expect("the router answers")
    }

    async fn json_body(response: Response) -> serde_json::Value {
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect the body")
            .to_bytes();
        serde_json::from_slice(&bytes).expect("the body is JSON")
    }

    /// An extractor's plain-text rejection leaves as a problem, its message kept as the detail
    /// [ARC-020].
    #[tokio::test]
    async fn a_rejected_body_becomes_a_problem() {
        let router = Router::new()
            .route("/", post(|_: Json<serde_json::Value>| async {}))
            .layer(map_response(problem_for_bare_errors));

        let response = post_to(router, "application/json", "{not json").await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CONTENT_TYPE], CONTENT_TYPE);
        // The rejection's own Content-Length must not survive onto the replacement body.
        let content_length = response
            .headers()
            .get(header::CONTENT_LENGTH)
            .map(|value| value.to_str().expect("text").to_owned());
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect the body")
            .to_bytes();
        assert!(
            content_length
                .as_deref()
                .is_none_or(|length| length == bytes.len().to_string()),
            "Content-Length {content_length:?} for a body of {} bytes",
            bytes.len()
        );
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("the body is JSON");
        assert_eq!(body["type"], ABOUT_BLANK);
        assert_eq!(body["title"], "Bad Request");
        assert_eq!(body["status"], 400);
        assert!(
            body["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("JSON")),
            "{body}"
        );
    }

    /// A handler's own problem passes through untouched, and so does a success [ARC-020].
    #[tokio::test]
    async fn a_problem_or_a_success_is_left_alone() {
        let router = Router::new()
            .route(
                "/",
                post(|body: String| async move {
                    if body == "fail" {
                        Err(Problem::from(RateError::Unavailable {
                            currency: Currency::new("USD"),
                            date: date(),
                        }))
                    } else {
                        Ok("fine")
                    }
                }),
            )
            .layer(map_response(problem_for_bare_errors));

        let failed = post_to(router.clone(), "text/plain", "fail").await;
        assert_eq!(failed.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            json_body(failed).await["type"],
            "urn:fifolio:problem:rate-unavailable"
        );

        let fine = post_to(router, "text/plain", "ok").await;
        assert_eq!(fine.status(), StatusCode::OK);
        let bytes = fine
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        assert_eq!(&bytes[..], b"fine");
    }

    /// The rewrite bound is the error range: a bare 5xx is rewritten as a 4xx is, and a 3xx,
    /// which is not an error, passes through as written [ARC-020].
    #[tokio::test]
    async fn a_bare_server_error_is_rewritten_and_a_redirect_is_not() {
        let router = Router::new()
            .route(
                "/",
                post(|body: String| async move {
                    if body == "fail" {
                        (StatusCode::INTERNAL_SERVER_ERROR, "it broke")
                    } else {
                        (StatusCode::SEE_OTHER, "elsewhere")
                    }
                }),
            )
            .layer(map_response(problem_for_bare_errors));

        let failed = post_to(router.clone(), "text/plain", "fail").await;
        assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(failed.headers()[header::CONTENT_TYPE], CONTENT_TYPE);
        assert_eq!(
            json_body(failed).await,
            serde_json::json!({
                "type": ABOUT_BLANK,
                "title": "Internal Server Error",
                "status": 500,
                "detail": "it broke",
            })
        );

        let redirect = post_to(router, "text/plain", "redirect").await;
        assert_eq!(redirect.status(), StatusCode::SEE_OTHER);
        assert!(
            redirect.headers()[header::CONTENT_TYPE]
                .to_str()
                .expect("text")
                .starts_with("text/plain")
        );
        let bytes = redirect
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        assert_eq!(&bytes[..], b"elsewhere");
    }

    async fn rewritten_detail(body: String) -> Option<serde_json::Value> {
        let response = Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from(body))
            .expect("build the response");
        json_body(problem_for_bare_errors(response).await)
            .await
            .get("detail")
            .cloned()
    }

    /// A bare body up to `DETAIL_LIMIT` bytes is carried as the detail, one byte more is
    /// dropped, and a body of only whitespace gives no detail rather than an empty one
    /// [ARC-020].
    #[tokio::test]
    async fn the_detail_is_kept_up_to_the_limit_and_dropped_past_it() {
        let at_limit = "x".repeat(DETAIL_LIMIT);
        assert_eq!(
            rewritten_detail(at_limit.clone()).await,
            Some(serde_json::Value::String(at_limit))
        );
        assert_eq!(rewritten_detail("x".repeat(DETAIL_LIMIT + 1)).await, None);
        assert_eq!(rewritten_detail(" \n\t ".to_owned()).await, None);
    }

    /// A bare response stating its own Content-Length does not lend it to the problem that
    /// replaces its body, which would misframe the reply on the wire [ARC-020]. axum's own
    /// rejections leave the length to hyper, so this states one explicitly.
    #[tokio::test]
    async fn the_replaced_body_takes_its_content_length_with_it() {
        let response = Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header(header::CONTENT_LENGTH, "8")
            .body(Body::from("it broke"))
            .expect("build the response");

        let problem = problem_for_bare_errors(response).await;

        let content_length = problem
            .headers()
            .get(header::CONTENT_LENGTH)
            .map(|value| value.to_str().expect("text").to_owned());
        let bytes = problem
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        assert!(
            content_length
                .as_deref()
                .is_none_or(|length| length == bytes.len().to_string()),
            "Content-Length {content_length:?} for a body of {} bytes",
            bytes.len()
        );
    }
}
