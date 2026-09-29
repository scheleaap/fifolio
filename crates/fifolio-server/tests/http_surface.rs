//! Integration layer: the real router over a real temporary database, in process [TST-003,
//! TST-005].
//!
//! The router is exercised through `tower`'s `oneshot`, so a request travels the real routing,
//! extraction and response path without a socket. The socket is the end-to-end layer's
//! business.
//!
//! [`Harness`] is where a server item adds its cases: build one, [`Harness::send`] a request,
//! and assert on the [`Reply`]. An error reply is checked with [`Reply::assert_problem`], which
//! holds every error to the one problem+json shape [ARC-020].

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use chrono::DateTime;
use fifolio_core::decimal::{Money, Quantity, QuotedPrice};
use fifolio_core::entities::{
    Account, ImportBatch, ImportCounts, Isin, Order, Quotation, Security, SecurityType,
    SourceFormat, SourceRecord,
};
use fifolio_core::identity::{IdentitySource, identify};
use fifolio_core::manual_entry::{Election, ManualEntry, Ratio, Supplied};
use fifolio_core::storage::{
    Allocation, BatchId, Database, Placement, RecordHandle, TransactionId,
};
use fifolio_core::transaction::{
    Buy, BuyOrigin, DateProvenance, Derivation, Expiration, Sell, Split, Transaction, TransferIn,
    TransferInSource, TransferOut,
};
use fifolio_core::valuation::Conversion;
use fifolio_server::problem::{ABOUT_BLANK, CONTENT_TYPE};
use fifolio_test_support::TempDb;
use http_body_util::BodyExt;
use rust_decimal_macros::dec;
use serde_json::{Value, json};
use tower::ServiceExt;
use vec1::vec1;

/// The server as a request sees it, over a database of its own.
struct Harness {
    /// Held so the directory outlives the router; dropping it removes the database.
    dir: TempDb,
    database: Database,
    router: Router,
}

impl Harness {
    /// Opens a fresh temporary database the way `serve` does — created and migrated — and
    /// builds the router over it.
    async fn new() -> Self {
        let dir = TempDb::new();
        let database = Database::open(dir.path())
            .await
            .expect("open the temporary database");
        let router = fifolio_server::router(database.clone());
        Self {
            dir,
            database,
            router,
        }
    }

    async fn send(&self, request: Request<Body>) -> Reply {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect the body")
            .to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("the body is JSON")
        };
        Reply {
            status,
            headers,
            body,
        }
    }

    async fn request(&self, method: Method, uri: &str) -> Reply {
        self.send(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("build the request"),
        )
        .await
    }
}

impl Harness {
    async fn json(&self, method: Method, uri: &str, body: &Value) -> Reply {
        self.send(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("build the request"),
        )
        .await
    }

    /// Posts `file` to `/imports` with `query` as its parameters, the way a client sends a file.
    async fn post_file(&self, query: &str, file: Vec<u8>) -> Reply {
        self.send(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/imports?{query}"))
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .body(Body::from(file))
                .expect("build the request"),
        )
        .await
    }

    /// How many rows `table` holds; the names are this file's own constants, never input.
    async fn count(&self, table: &'static str) -> i64 {
        let pool = sqlx::SqlitePool::connect(&self.dir.url())
            .await
            .expect("connect");
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!("select count(*) from {table}")))
            .fetch_one(&pool)
            .await
            .expect("count")
    }

    /// One batch of `account` owning one source record whose parsed fields are `fields`, the
    /// way an import leaves them.
    async fn import_record(&self, account: &Account, reference: &str, fields: &[(&str, &str)]) {
        let batch = self
            .database
            .import_batches()
            .insert(&ImportBatch::new(
                account.clone(),
                "Transactions_2024.xlsx",
                SourceFormat::SaxoNlXlsx,
                DateTime::from_timestamp(1_714_608_000, 0).expect("a timestamp"),
                ImportCounts::default(),
            ))
            .await
            .expect("insert the batch");
        let record = SourceRecord::new(
            identify(account, &IdentitySource::BrokerReference(reference)),
            Order::new(1),
            "raw",
            fields
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect::<BTreeMap<_, _>>(),
        );
        self.database
            .source_records()
            .insert(batch, &record)
            .await
            .expect("insert the record");
    }
}

/// A response, its body parsed as JSON.
struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Reply {
    /// The reply is an RFC 9457 problem of `problem_type` at `status` [ARC-020]: served as
    /// `application/problem+json`, carrying `type`, `title` and `status` with the status
    /// matching the response's own, an optional string `detail`, and no other member. Returns
    /// the detail for the case to check what it names.
    fn assert_problem(&self, status: StatusCode, problem_type: &str) -> Option<&str> {
        assert_eq!(self.status, status, "{}", self.body);
        assert_eq!(self.headers[header::CONTENT_TYPE], CONTENT_TYPE);
        let members = self.body.as_object().expect("a problem is a JSON object");
        assert!(
            members
                .keys()
                .all(|key| ["type", "title", "status", "detail"].contains(&key.as_str())),
            "unexpected members in {}",
            self.body
        );
        assert_eq!(self.body["type"], problem_type);
        assert!(
            self.body["title"]
                .as_str()
                .is_some_and(|title| !title.is_empty()),
            "a problem carries a title: {}",
            self.body
        );
        assert_eq!(self.body["status"], status.as_u16());
        match &self.body.get("detail") {
            None => None,
            Some(detail) => Some(detail.as_str().expect("detail is a string")),
        }
    }
}

/// A path no route serves answers 404 as a problem, not as an empty body [ARC-020, TST-005].
#[tokio::test]
async fn an_unrouted_path_is_a_404_problem() {
    let harness = Harness::new().await;

    let reply = harness.request(Method::GET, "/nothing-here").await;

    assert_eq!(
        reply.assert_problem(StatusCode::NOT_FOUND, ABOUT_BLANK),
        None
    );
}

/// A method a route does not serve answers 405 as a problem, still naming the methods it does
/// serve, as a 405 must [ARC-020, TST-005].
#[tokio::test]
async fn an_unserved_method_is_a_405_problem_that_keeps_allow() {
    let harness = Harness::new().await;

    let reply = harness.request(Method::DELETE, "/openapi.json").await;

    reply.assert_problem(StatusCode::METHOD_NOT_ALLOWED, ABOUT_BLANK);
    let allow = reply.headers[header::ALLOW]
        .to_str()
        .expect("Allow is text");
    assert!(allow.contains("GET"), "Allow: {allow}");
}

const SAXO: &str = "/accounts/Saxo/69900%2F1000000";

fn saxo() -> Value {
    json!({"broker": "Saxo", "id": "69900/1000000"})
}

fn philips() -> Value {
    json!({
        "isin": "NL0000009538",
        "name": "Philips",
        "security_type": "stock",
        "quotation": "per_unit",
    })
}

/// Accounts are created, read, listed, renamed and deleted, a slash in the broker's id
/// travelling percent-encoded in the path [SRV-007, TST-005].
#[tokio::test]
async fn accounts_have_create_read_update_delete_and_list() {
    let harness = Harness::new().await;

    let created = harness.json(Method::POST, "/accounts", &saxo()).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body, saxo());

    let tr = json!({"broker": "Trade Republic", "id": "main"});
    harness.json(Method::POST, "/accounts", &tr).await;
    let listed = harness.request(Method::GET, "/accounts").await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.body, json!([saxo(), tr]));

    let read = harness.request(Method::GET, SAXO).await;
    assert_eq!(read.status, StatusCode::OK);
    assert_eq!(read.body, saxo());

    let renamed_to = json!({"broker": "Saxo", "id": "69900/2000000"});
    let renamed = harness.json(Method::PUT, SAXO, &renamed_to).await;
    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.body);
    assert_eq!(renamed.body, renamed_to);
    harness
        .request(Method::GET, SAXO)
        .await
        .assert_problem(StatusCode::NOT_FOUND, "urn:fifolio:problem:unknown-account");

    let deleted = harness
        .request(Method::DELETE, "/accounts/Saxo/69900%2F2000000")
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert_eq!(deleted.body, Value::Null);
    assert_eq!(
        harness.request(Method::GET, "/accounts").await.body,
        json!([tr])
    );
}

/// Asking for, editing or deleting an account that is not stored is a 404 naming it, and
/// creating one twice or renaming onto a taken key is a conflict [SRV-007, ARC-020, TST-005].
#[tokio::test]
async fn an_absent_or_duplicate_account_is_refused() {
    let harness = Harness::new().await;

    for method in [Method::GET, Method::DELETE] {
        let reply = harness.request(method, SAXO).await;
        let detail =
            reply.assert_problem(StatusCode::NOT_FOUND, "urn:fifolio:problem:unknown-account");
        assert!(detail.is_some_and(|detail| detail.contains("69900/1000000")));
    }
    harness
        .json(Method::PUT, SAXO, &saxo())
        .await
        .assert_problem(StatusCode::NOT_FOUND, "urn:fifolio:problem:unknown-account");

    harness.json(Method::POST, "/accounts", &saxo()).await;
    harness
        .json(Method::POST, "/accounts", &saxo())
        .await
        .assert_problem(
            StatusCode::CONFLICT,
            "urn:fifolio:problem:duplicate-account",
        );

    let other = json!({"broker": "Saxo", "id": "other"});
    harness.json(Method::POST, "/accounts", &other).await;
    harness
        .json(Method::PUT, "/accounts/Saxo/other", &saxo())
        .await
        .assert_problem(
            StatusCode::CONFLICT,
            "urn:fifolio:problem:duplicate-account",
        );
}

/// An account a source record was imported into is neither deleted nor renamed; the refusal
/// says what holds it, and the account is still there afterwards [SRV-008, TST-005].
#[tokio::test]
async fn deleting_an_account_a_source_record_references_is_refused() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;
    harness
        .import_record(&Account::new("Saxo", "69900/1000000"), "r1", &[])
        .await;

    let refused = harness.request(Method::DELETE, SAXO).await;
    let detail = refused.assert_problem(
        StatusCode::CONFLICT,
        "urn:fifolio:problem:account-referenced",
    );
    assert!(
        detail.is_some_and(|detail| detail.contains("1 source records")),
        "{}",
        refused.body
    );

    harness
        .json(Method::PUT, SAXO, &json!({"broker": "Saxo", "id": "new"}))
        .await
        .assert_problem(
            StatusCode::CONFLICT,
            "urn:fifolio:problem:account-referenced",
        );
    assert_eq!(harness.request(Method::GET, SAXO).await.body, saxo());
}

/// A manual entry alone holds an account against deletion and a change of key, as a record
/// does; writing the account back under its own key is no change and succeeds [SRV-008,
/// DEC-089, TST-005].
#[tokio::test]
async fn an_account_a_manual_entry_names_keeps_its_key() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;
    let account = Account::new("Saxo", "69900/1000000");
    harness
        .database
        .manual_entries()
        .insert(&ManualEntry::new(
            account.clone(),
            Isin::new("NL0000009538"),
            Supplied::Election(Election::Cash),
            [identify(&account, &IdentitySource::BrokerReference("r1"))],
        ))
        .await
        .expect("insert the entry");

    let refused = harness.request(Method::DELETE, SAXO).await;
    let detail = refused.assert_problem(
        StatusCode::CONFLICT,
        "urn:fifolio:problem:account-referenced",
    );
    assert!(
        detail.is_some_and(|detail| detail.contains("1 manual entries")),
        "{}",
        refused.body
    );
    harness
        .json(
            Method::PUT,
            SAXO,
            &json!({"broker": "IBKR", "id": "69900/1000000"}),
        )
        .await
        .assert_problem(
            StatusCode::CONFLICT,
            "urn:fifolio:problem:account-referenced",
        );

    let unchanged = harness.json(Method::PUT, SAXO, &saxo()).await;
    assert_eq!(unchanged.status, StatusCode::OK, "{}", unchanged.body);
    assert_eq!(unchanged.body, saxo());
    assert_eq!(
        harness.request(Method::GET, "/accounts").await.body,
        json!([saxo()])
    );
}

/// Securities are created, read, listed, edited and deleted; a user-created security is
/// flagged neither as auto-created nor as needing review, and a lowercase ISIN in the path
/// names the same security [SRV-007, DOM-006, DOM-126, TST-005].
#[tokio::test]
async fn securities_have_create_read_update_delete_and_list() {
    let harness = Harness::new().await;

    let created = harness.json(Method::POST, "/securities", &philips()).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let mut stored = philips();
    stored["auto_created"] = json!(false);
    stored["needs_review"] = json!(false);
    assert_eq!(created.body, stored);

    let read = harness
        .request(Method::GET, "/securities/nl0000009538")
        .await;
    assert_eq!(read.status, StatusCode::OK);
    assert_eq!(read.body, stored);

    let edited = harness
        .json(
            Method::PUT,
            "/securities/NL0000009538",
            &json!({"name": "Koninklijke Philips", "security_type": "stock", "quotation": "per_unit"}),
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);
    assert_eq!(edited.body["name"], "Koninklijke Philips");

    let listed = harness.request(Method::GET, "/securities").await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.body, json!([edited.body]));

    let deleted = harness
        .request(Method::DELETE, "/securities/NL0000009538")
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    harness
        .request(Method::GET, "/securities/NL0000009538")
        .await
        .assert_problem(
            StatusCode::NOT_FOUND,
            "urn:fifolio:problem:unknown-security",
        );
    assert_eq!(
        harness.request(Method::GET, "/securities").await.body,
        json!([])
    );
}

/// Creating a security whose ISIN is stored is a conflict naming the ISIN, whatever else the
/// request says and however the ISIN is spelled, and the stored security is untouched
/// [SRV-010, ARC-021, TST-005].
#[tokio::test]
async fn creating_a_security_with_an_existing_isin_is_a_conflict() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/securities", &philips()).await;

    let second = harness
        .json(
            Method::POST,
            "/securities",
            &json!({
                "isin": " nl0000009538",
                "name": "Another name",
                "security_type": "etf",
                "quotation": "percent_of_par",
            }),
        )
        .await;

    let detail = second.assert_problem(StatusCode::CONFLICT, "urn:fifolio:problem:duplicate-isin");
    assert!(
        detail.is_some_and(|detail| detail.contains("NL0000009538")),
        "{}",
        second.body
    );
    assert_eq!(
        harness
            .request(Method::GET, "/securities/NL0000009538")
            .await
            .body["name"],
        "Philips"
    );
}

/// An auto-created security is corrected by editing its type and quotation, each on its own,
/// and it stays flagged as auto-created [SRV-011, DOM-006, DOM-037, TST-005].
#[tokio::test]
async fn an_auto_created_security_is_corrected_through_type_and_quotation() {
    let harness = Harness::new().await;
    harness
        .database
        .securities()
        .insert(&Security::auto_created(
            Isin::new("NL0000102077"),
            "NL 7.5% 2023",
            SecurityType::Other,
            Quotation::PerUnit,
        ))
        .await
        .expect("insert");
    let uri = "/securities/NL0000102077";

    let retyped = harness
        .json(
            Method::PUT,
            uri,
            &json!({"name": "NL 7.5% 2023", "security_type": "bond", "quotation": "per_unit"}),
        )
        .await;
    assert_eq!(retyped.status, StatusCode::OK, "{}", retyped.body);
    assert_eq!(
        (&retyped.body["security_type"], &retyped.body["quotation"]),
        (&json!("bond"), &json!("per_unit"))
    );

    let requoted = harness
        .json(
            Method::PUT,
            uri,
            &json!({"name": "NL 7.5% 2023", "security_type": "bond", "quotation": "percent_of_par"}),
        )
        .await;
    assert_eq!(
        requoted.body,
        json!({
            "isin": "NL0000102077",
            "name": "NL 7.5% 2023",
            "security_type": "bond",
            "quotation": "percent_of_par",
            "auto_created": true,
            "needs_review": true,
        })
    );
    assert_eq!(harness.request(Method::GET, uri).await.body, requoted.body);
}

/// An imported security needs review; an edit leaves it needing review even when the body
/// says otherwise, and marking it reviewed clears that and leaves auto-created and everything
/// else as it was, however often it is asked. Marking an absent security is a 404, and marking
/// a user-entered one changes nothing [SRV-057, DOM-126, DOM-006, TST-005].
#[tokio::test]
async fn only_marking_a_security_reviewed_clears_needs_review() {
    let harness = Harness::new().await;
    harness
        .database
        .securities()
        .insert(&Security::auto_created(
            Isin::new("IE00B4L5Y983"),
            "iShares Core MSCI World",
            SecurityType::Fund,
            Quotation::PerUnit,
        ))
        .await
        .expect("insert");
    let uri = "/securities/IE00B4L5Y983";
    assert_eq!(
        harness.request(Method::GET, uri).await.body["needs_review"],
        true
    );

    let edited = harness
        .json(
            Method::PUT,
            uri,
            &json!({
                "name": "iShares Core MSCI World",
                "security_type": "etf",
                "quotation": "per_unit",
                "needs_review": false,
            }),
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);
    assert_eq!(edited.body["needs_review"], true);

    let mut expected = edited.body.clone();
    expected["needs_review"] = json!(false);
    for _ in 0..2 {
        let reviewed = harness
            .request(Method::POST, "/securities/ie00b4l5y983/reviewed")
            .await;
        assert_eq!(reviewed.status, StatusCode::OK, "{}", reviewed.body);
        assert_eq!(reviewed.body, expected);
    }
    assert_eq!(harness.request(Method::GET, uri).await.body, expected);
    assert_eq!(expected["auto_created"], true);
    assert_eq!(expected["security_type"], "etf");

    let absent = harness
        .request(Method::POST, "/securities/NL0000009538/reviewed")
        .await;
    let detail = absent.assert_problem(
        StatusCode::NOT_FOUND,
        "urn:fifolio:problem:unknown-security",
    );
    assert!(detail.is_some_and(|detail| detail.contains("NL0000009538")));

    let by_user = harness.json(Method::POST, "/securities", &philips()).await;
    assert_eq!(by_user.status, StatusCode::CREATED, "{}", by_user.body);
    let reviewed = harness
        .request(Method::POST, "/securities/NL0000009538/reviewed")
        .await;
    assert_eq!(reviewed.status, StatusCode::OK, "{}", reviewed.body);
    assert_eq!(reviewed.body, by_user.body);
    assert_eq!(
        (
            &reviewed.body["auto_created"],
            &reviewed.body["needs_review"]
        ),
        (&json!(false), &json!(false))
    );
    assert_eq!(
        harness
            .request(Method::GET, "/securities/NL0000009538")
            .await
            .body,
        by_user.body
    );
}

/// Editing or deleting a security that is not stored is a 404, and a type outside the fixed set
/// is refused as a problem rather than stored [SRV-007, SRV-011, DOM-004, ARC-020, TST-005].
#[tokio::test]
async fn an_absent_security_or_an_unknown_type_is_refused() {
    let harness = Harness::new().await;
    let edit = json!({"name": "x", "security_type": "bond", "quotation": "per_unit"});

    harness
        .json(Method::PUT, "/securities/NL0000009538", &edit)
        .await
        .assert_problem(
            StatusCode::NOT_FOUND,
            "urn:fifolio:problem:unknown-security",
        );
    harness
        .request(Method::DELETE, "/securities/NL0000009538")
        .await
        .assert_problem(
            StatusCode::NOT_FOUND,
            "urn:fifolio:problem:unknown-security",
        );

    let mut unknown = philips();
    unknown["security_type"] = json!("crypto");
    harness
        .json(Method::POST, "/securities", &unknown)
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, ABOUT_BLANK);
    assert_eq!(
        harness.request(Method::GET, "/securities").await.body,
        json!([])
    );
}

/// A security a source record names is not deleted, and the refusal says what holds it
/// [SRV-009, TST-005].
#[tokio::test]
async fn deleting_a_security_a_source_record_references_is_refused() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;
    harness.json(Method::POST, "/securities", &philips()).await;
    harness
        .import_record(
            &Account::new("Saxo", "69900/1000000"),
            "r1",
            &[("Instrument ISIN", "NL0000009538")],
        )
        .await;

    let refused = harness
        .request(Method::DELETE, "/securities/NL0000009538")
        .await;

    let detail = refused.assert_problem(
        StatusCode::CONFLICT,
        "urn:fifolio:problem:security-referenced",
    );
    assert!(
        detail.is_some_and(
            |detail| detail.contains("NL0000009538") && detail.contains("1 source records")
        ),
        "{}",
        refused.body
    );
    assert_eq!(
        harness
            .request(Method::GET, "/securities/NL0000009538")
            .await
            .status,
        StatusCode::OK
    );
}

/// The endpoints are in the served spec, which is read from the same route list [SRV-006].
#[tokio::test]
async fn the_endpoints_are_documented() {
    let harness = Harness::new().await;

    let spec = harness.request(Method::GET, "/openapi.json").await.body;

    for path in [
        "/accounts",
        "/accounts/{broker}/{id}",
        "/securities",
        "/securities/{isin}",
        "/securities/{isin}/reviewed",
        "/imports",
        "/imports/{batch}",
        "/source-records",
        "/source-records/{identity}",
    ] {
        assert!(spec["paths"].get(path).is_some(), "{path} is undocumented");
    }
    assert_eq!(
        spec["components"]["schemas"]["SecurityTypeBody"]["enum"],
        json!(["stock", "bond", "etf", "fund", "derivative", "other"])
    );
}

/// The query naming the Trade Republic account every import case targets.
const TRADE_REPUBLIC: &str = "broker=Trade%20Republic&account=DE0001";

async fn with_trade_republic_account() -> Harness {
    let harness = Harness::new().await;
    let created = harness
        .json(
            Method::POST,
            "/accounts",
            &json!({"broker": "Trade Republic", "id": "DE0001"}),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    harness
}

fn trade_republic_fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/trade-republic")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|_| panic!("read the fixture {}", path.display()))
}

/// A Trade Republic export of the given rows, each naming only the columns it populates.
fn trade_republic_export(rows: &[&[(&str, &str)]]) -> Vec<u8> {
    use fifolio_core::import::trade_republic::HEADERS;
    let quoted = |values: Vec<&str>| format!("\"{}\"\n", values.join("\",\""));
    let body: String = rows
        .iter()
        .map(|row| {
            quoted(
                HEADERS
                    .iter()
                    .map(|header| {
                        row.iter()
                            .find(|(name, _)| name == header)
                            .map_or("", |(_, value)| *value)
                    })
                    .collect(),
            )
        })
        .collect();
    format!("{}{body}", quoted(HEADERS.to_vec())).into_bytes()
}

/// A Trade Republic cash row of `kind` naming no security.
fn cash_row<'a>(kind: &'a str, transaction_id: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        ("datetime", "2024-05-03T06:01:14.891Z"),
        ("date", "2024-05-03"),
        ("category", "CASH"),
        ("type", kind),
        ("amount", "12.50"),
        ("currency", "EUR"),
        ("transaction_id", transaction_id),
    ]
}

/// A Trade Republic file posted with its account, format and name imports end to end: its buys
/// are stored in a batch of their own, its cash rows are counted and not stored, and the ISINs it
/// names are created flagged auto-created and needing review [SRV-012], [SRV-013], [SRV-014],
/// [SRV-016], [TST-005].
#[tokio::test]
async fn a_trade_republic_file_imports_end_to_end() {
    let harness = with_trade_republic_account().await;

    let reply = harness
        .post_file(
            &format!(
                "{TRADE_REPUBLIC}&format=trade_republic_de_csv\
                 &filename=transactions_2022-01-01_2022-12-31.csv"
            ),
            trade_republic_fixture("transactions_2022-01-01_2022-12-31.csv"),
        )
        .await;

    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let batch = reply.body["batch"].as_i64().expect("the batch id");
    assert_eq!(reply.body["unrecognized_types"], json!([]));
    let stored = harness
        .database
        .import_batches()
        .find(fifolio_core::storage::BatchId::new(batch))
        .await
        .expect("the lookup")
        .expect("the batch is stored");
    assert_eq!(stored.filename(), "transactions_2022-01-01_2022-12-31.csv");
    assert_eq!(stored.format(), SourceFormat::TradeRepublicDeCsv);
    // The fixture's two buys, and none of its five cash rows.
    assert_eq!(harness.count("source_record").await, 2);

    let securities = harness.request(Method::GET, "/securities").await;
    let flags: Vec<(&str, &str, bool, bool)> = securities
        .body
        .as_array()
        .expect("a list")
        .iter()
        .map(|security| {
            (
                security["isin"].as_str().expect("an ISIN"),
                security["security_type"].as_str().expect("a type"),
                security["auto_created"] == true,
                security["needs_review"] == true,
            )
        })
        .collect();
    assert_eq!(
        flags,
        [
            ("XF0000000079", "fund", true, true),
            ("XF0000000152", "stock", true, true),
        ]
    );
}

/// Posting the same file again succeeds and changes no stored record or security
/// (DEC-111, provisional) [SRV-015], [TST-005].
#[tokio::test]
async fn posting_the_same_file_again_changes_nothing() {
    let harness = with_trade_republic_account().await;
    let query = format!("{TRADE_REPUBLIC}&format=trade_republic_de_csv&filename=a.csv");
    let file = trade_republic_fixture("transactions_2023-01-01_2023-12-31.csv");
    let first = harness.post_file(&query, file.clone()).await;
    assert_eq!(first.status, StatusCode::CREATED, "{}", first.body);
    let records = harness.count("source_record").await;
    let securities = harness.request(Method::GET, "/securities").await.body;

    let second = harness.post_file(&query, file).await;

    assert_eq!(second.status, StatusCode::CREATED, "{}", second.body);
    assert_eq!(harness.count("source_record").await, records);
    assert_eq!(
        harness.request(Method::GET, "/securities").await.body,
        securities
    );
}

/// The response summarizes the file in four counts: its rows, derived, pending and
/// non-position, add up to the fixture's rows with no failed count beside them, and the
/// securities it auto-created are those not stored before, so a file posted again creates none
/// (DEC-110, provisional) [SRV-017], [SRV-058], [SRV-014].
#[tokio::test]
async fn the_response_summarizes_the_files_rows_and_created_securities() {
    let harness = with_trade_republic_account().await;
    let query = format!("{TRADE_REPUBLIC}&format=trade_republic_de_csv&filename=a.csv");
    let file = trade_republic_fixture("transactions_2022-01-01_2022-12-31.csv");
    // One line per row after the header: the fixture quotes no line break inside a field.
    let rows = file
        .split(|&byte| byte == b'\n')
        .filter(|line| !line.is_empty())
        .count()
        - 1;

    let first = harness.post_file(&query, file.clone()).await;
    let second = harness.post_file(&query, file).await;

    assert_eq!(first.status, StatusCode::CREATED, "{}", first.body);
    // The fixture's two buys, and its five cash rows; the buys name two ISINs.
    assert_eq!(
        first.body["summary"],
        json!({"derived": 2, "pending": 0, "non_position": 5, "securities_auto_created": 2})
    );
    let summary = &first.body["summary"];
    let classified: u64 = ["derived", "pending", "non_position"]
        .iter()
        .map(|count| summary[count].as_u64().expect("a count"))
        .sum();
    assert_eq!(classified, u64::try_from(rows).expect("a row count"));
    assert_eq!(
        second.body["summary"],
        json!({"derived": 2, "pending": 0, "non_position": 5, "securities_auto_created": 0})
    );
}

/// The response names every unrecognized row type the file carried, with how many rows carried
/// it, ordered by type; none of those rows is stored [SRV-049], [SRV-016], [IMP-TR-014].
#[tokio::test]
async fn the_response_names_every_unrecognized_row_type_with_its_count() {
    let harness = with_trade_republic_account().await;
    let file = trade_republic_export(&[
        // SAVEBACK comes first in the file, so the reply's order is by type and not by first
        // appearance.
        &cash_row("SAVEBACK", "a"),
        &cash_row("CARD_TRANSACTION", "b"),
        &cash_row("CARD_TRANSACTION", "c"),
        &cash_row("INTEREST_PAYMENT", "d"),
    ]);

    let reply = harness
        .post_file(
            &format!("{TRADE_REPUBLIC}&format=trade_republic_de_csv&filename=a.csv"),
            file,
        )
        .await;

    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    assert_eq!(
        reply.body["unrecognized_types"],
        json!([
            {"row_type": "CASH/CARD_TRANSACTION", "rows": 2},
            {"row_type": "CASH/SAVEBACK", "rows": 1},
        ])
    );
    assert_eq!(harness.count("source_record").await, 0);
}

/// A file with failed rows is refused whole as a problem whose detail names every failed row,
/// and nothing is stored [SRV-058], [ARC-020], [TST-005].
#[tokio::test]
async fn a_file_with_failed_rows_is_refused_naming_every_one() {
    let harness = with_trade_republic_account().await;
    // BOND maps to no security type (DEC-077), so each bond buy is a failed row.
    let bond = |isin, transaction_id| {
        vec![
            ("datetime", "2024-05-02T06:01:14.891Z"),
            ("date", "2024-05-02"),
            ("category", "TRADING"),
            ("type", "BUY"),
            ("asset_class", "BOND"),
            ("name", "A bond"),
            ("symbol", isin),
            ("shares", "35.0000000000"),
            ("price", "75.090000"),
            ("amount", "-2628.150000"),
            ("fee", "-1.00"),
            ("currency", "EUR"),
            ("transaction_id", transaction_id),
        ]
    };
    let file = trade_republic_export(&[
        &bond("XF0000000301", "x"),
        &cash_row("CUSTOMER_INBOUND", "a"),
        &bond("XF0000000302", "y"),
    ]);

    let reply = harness
        .post_file(
            &format!("{TRADE_REPUBLIC}&format=trade_republic_de_csv&filename=a.csv"),
            file,
        )
        .await;

    let detail = reply
        .assert_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "urn:fifolio:problem:failed-rows",
        )
        .expect("a detail");
    assert!(detail.starts_with("2 rows could not be read"), "{detail}");
    for isin in ["XF0000000301", "XF0000000302"] {
        assert!(detail.contains(isin), "{detail}");
    }
    for table in ["import_batch", "source_record", "security"] {
        assert_eq!(harness.count(table).await, 0, "{table}");
    }
}

/// A Trade Republic cash row of `kind` traded on `date`, which is also its timestamp's day.
fn cash_row_on<'a>(
    kind: &'a str,
    transaction_id: &'a str,
    datetime: &'a str,
    date: &'a str,
) -> Vec<(&'a str, &'a str)> {
    let mut row = cash_row(kind, transaction_id);
    row[0] = ("datetime", datetime);
    row[1] = ("date", date);
    row
}

/// A file whose trade dates span two calendar years is refused at the endpoint with the years'
/// own problem type, naming both years, and not as an unreadable file; nothing is stored
/// [SRV-051], [ARC-021], [TST-005].
#[tokio::test]
async fn a_file_spanning_two_calendar_years_is_refused_with_its_own_type() {
    let harness = with_trade_republic_account().await;
    let file = trade_republic_export(&[
        &cash_row_on(
            "CUSTOMER_INBOUND",
            "a",
            "2023-12-31T06:01:14.891Z",
            "2023-12-31",
        ),
        &cash_row_on(
            "CUSTOMER_INBOUND",
            "b",
            "2024-01-02T06:01:14.891Z",
            "2024-01-02",
        ),
    ]);

    let reply = harness
        .post_file(
            &format!("{TRADE_REPUBLIC}&format=trade_republic_de_csv&filename=a.csv"),
            file,
        )
        .await;

    let detail = reply
        .assert_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "urn:fifolio:problem:multiple-calendar-years",
        )
        .expect("a detail");
    assert!(detail.contains("2023, 2024"), "{detail}");
    for table in ["import_batch", "source_record", "security"] {
        assert_eq!(harness.count(table).await, 0, "{table}");
    }
}

/// A file spanning two years that also has a failed row is refused once, naming both grounds:
/// the years and the failed row, so the years do not hide the row nor the row the years
/// (DEC-113, provisional) [SRV-051], [SRV-059], [ARC-021], [TST-005].
#[tokio::test]
async fn a_multi_year_file_with_a_failed_row_is_refused_naming_every_ground() {
    let harness = with_trade_republic_account().await;
    let file = trade_republic_export(&[
        &cash_row_on(
            "CUSTOMER_INBOUND",
            "a",
            "2023-12-31T06:01:14.891Z",
            "2023-12-31",
        ),
        // An unreadable trade date is a failed row [SRV-058], and contributes no year.
        &cash_row_on("CUSTOMER_INBOUND", "b", "not-a-date", "not-a-date"),
        &cash_row_on(
            "CUSTOMER_INBOUND",
            "c",
            "2024-01-02T06:01:14.891Z",
            "2024-01-02",
        ),
    ]);

    let reply = harness
        .post_file(
            &format!("{TRADE_REPUBLIC}&format=trade_republic_de_csv&filename=a.csv"),
            file,
        )
        .await;

    let detail = reply
        .assert_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "urn:fifolio:problem:several-grounds",
        )
        .expect("a detail");
    assert!(
        detail.contains("more than one calendar year: 2023, 2024"),
        "{detail}"
    );
    // Positions index the data rows from zero, so the middle row is row 1.
    assert!(
        detail.contains("1 rows could not be read: row 1: the column date holds \"not-a-date\""),
        "{detail}"
    );
    for table in ["import_batch", "source_record", "security"] {
        assert_eq!(harness.count(table).await, 0, "{table}");
    }
}

/// A Trade Republic file states no account, so it is not checked: the same file imports into
/// two accounts of different ids, and each keeps records of its own [SRV-056], [TST-005]
/// (DEC-075). The refusal of a file naming another account is not reachable here yet: the only
/// format stating one is Saxo, refused as not supported yet (DEC-114, provisional).
#[tokio::test]
async fn a_trade_republic_file_is_not_checked_against_the_account() {
    let harness = with_trade_republic_account().await;
    let other = harness
        .json(
            Method::POST,
            "/accounts",
            &json!({"broker": "Trade Republic", "id": "an unrelated id"}),
        )
        .await;
    assert_eq!(other.status, StatusCode::CREATED, "{}", other.body);
    let file = trade_republic_fixture("transactions_2022-01-01_2022-12-31.csv");

    for account in ["DE0001", "an%20unrelated%20id"] {
        let reply = harness
            .post_file(
                &format!(
                    "broker=Trade%20Republic&account={account}\
                     &format=trade_republic_de_csv&filename=a.csv"
                ),
                file.clone(),
            )
            .await;
        assert_eq!(
            reply.status,
            StatusCode::CREATED,
            "{account}: {}",
            reply.body
        );
    }
    // The fixture's two buys, once per account: identity is scoped to the account [DOM-023].
    assert_eq!(harness.count("source_record").await, 4);
}

/// A Saxo file is refused as a format not supported yet, naming the item that makes it
/// importable, until the Saxo importer can classify its rows [SRV-013], [ARC-020]. This is an
/// interim refusal: SRV-013's Saxo half is deferred to FIF-023 and is not covered here.
#[tokio::test]
async fn a_saxo_file_is_refused_as_not_supported_yet() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;

    let reply = harness
        .post_file(
            "broker=Saxo&account=69900%2F1000000&format=saxo_nl_xlsx&filename=a.xlsx",
            b"an XLSX file".to_vec(),
        )
        .await;

    let detail = reply
        .assert_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "urn:fifolio:problem:format-not-supported",
        )
        .expect("a detail");
    assert!(detail.contains("FIF-023"), "{detail}");
    assert_eq!(harness.count("import_batch").await, 0);
}

/// An import into an account that is not stored is a 404; one missing a parameter, or naming a
/// format that does not exist, is refused as a problem too: the account and format are never
/// inferred [SRV-012], [ARC-020].
#[tokio::test]
async fn an_unknown_account_or_an_incomplete_request_is_refused() {
    let harness = with_trade_republic_account().await;
    let file = trade_republic_fixture("transactions_2022-01-01_2022-12-31.csv");

    harness
        .post_file(
            "broker=Trade%20Republic&account=nobody&format=trade_republic_de_csv&filename=a.csv",
            file.clone(),
        )
        .await
        .assert_problem(StatusCode::NOT_FOUND, "urn:fifolio:problem:unknown-account");
    for incomplete in [
        format!("{TRADE_REPUBLIC}&format=trade_republic_de_csv"),
        "broker=Trade%20Republic&format=trade_republic_de_csv&filename=a.csv".to_owned(),
        "account=DE0001&format=trade_republic_de_csv&filename=a.csv".to_owned(),
        format!("{TRADE_REPUBLIC}&filename=a.csv"),
    ] {
        harness
            .post_file(&incomplete, file.clone())
            .await
            .assert_problem(StatusCode::BAD_REQUEST, ABOUT_BLANK);
    }
    harness
        .post_file(
            &format!("{TRADE_REPUBLIC}&format=comdirect_csv&filename=a.csv"),
            file,
        )
        .await
        .assert_problem(StatusCode::BAD_REQUEST, ABOUT_BLANK);
}

/// Every import creates a batch, and each batch is read and listed with its account, filename,
/// format, timestamp and three counts; a file posted again has a batch of its own (DEC-111,
/// DEC-112, provisional) [SRV-019], [SRV-020], [TST-005].
#[tokio::test]
async fn every_import_creates_a_batch_that_is_read_and_listed() {
    let harness = with_trade_republic_account().await;
    assert_eq!(
        harness.request(Method::GET, "/imports").await.body,
        json!([])
    );
    let query = format!("{TRADE_REPUBLIC}&format=trade_republic_de_csv&filename=a.csv");
    let file = trade_republic_fixture("transactions_2022-01-01_2022-12-31.csv");

    let first = harness.post_file(&query, file.clone()).await;
    let second = harness.post_file(&query, file).await;

    let listed = harness.request(Method::GET, "/imports").await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    let batches = listed.body.as_array().expect("a list");
    assert_eq!(
        batches
            .iter()
            .map(|batch| batch["id"].clone())
            .collect::<Vec<_>>(),
        [first.body["batch"].clone(), second.body["batch"].clone()],
        "one batch per import, in import order"
    );
    for batch in batches {
        let imported_at = batch["imported_at"].as_str().expect("a timestamp");
        assert!(
            DateTime::parse_from_rfc3339(imported_at).is_ok(),
            "{imported_at}"
        );
        assert_eq!(
            batch,
            &json!({
                "id": batch["id"],
                "account": {"broker": "Trade Republic", "id": "DE0001"},
                "filename": "a.csv",
                "format": "trade_republic_de_csv",
                "imported_at": imported_at,
                "counts": {"derived": 2, "pending": 0, "non_position": 5},
            })
        );
        let read = harness
            .request(Method::GET, &format!("/imports/{}", batch["id"]))
            .await;
        assert_eq!(read.status, StatusCode::OK, "{}", read.body);
        assert_eq!(&read.body, batch);
    }
}

/// A batch that is not stored is a 404 to read and to delete [SRV-020], [SRV-022], [ARC-020].
#[tokio::test]
async fn an_absent_batch_is_not_found() {
    let harness = Harness::new().await;

    for method in [Method::GET, Method::DELETE] {
        harness
            .request(method, "/imports/1")
            .await
            .assert_problem(StatusCode::NOT_FOUND, "urn:fifolio:problem:unknown-batch");
    }
}

/// The ISIN of [`philips`].
fn philips_isin() -> Isin {
    Isin::new("NL0000009538")
}

fn on(day: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2024, 5, day).expect("a valid date")
}

fn valued<T: fifolio_core::decimal::Scaled>(value: T) -> fifolio_core::valuation::Valued<T> {
    fifolio_core::valuation::Valued::new(value, value)
}

fn buy_of(record: RecordHandle) -> Transaction {
    Buy::new(
        Derivation::new(on(1), vec1![record]),
        Quantity::new(dec!(100.00000000)),
        valued(QuotedPrice::new(dec!(10.000000))),
        valued(Money::new(dec!(1000.00))),
        valued(Money::new(dec!(8.00))),
        BuyOrigin::Purchase,
        Conversion::native(on(1)),
    )
    .into()
}

fn sell_of(day: u32, record: RecordHandle) -> Transaction {
    Sell::new(
        Derivation::new(on(day), vec1![record]),
        Quantity::new(dec!(10.00000000)),
        valued(QuotedPrice::new(dec!(12.000000))),
        valued(Money::new(dec!(120.00))),
        valued(Money::new(dec!(8.00))),
        Conversion::native(on(day)),
    )
    .into()
}

impl Harness {
    /// A batch of the Saxo account with no records, as an import leaves one.
    async fn saxo_batch(&self, filename: &str) -> BatchId {
        self.database
            .import_batches()
            .insert(&ImportBatch::new(
                Account::new("Saxo", "69900/1000000"),
                filename,
                SourceFormat::SaxoNlXlsx,
                DateTime::from_timestamp(1_714_608_000, 0).expect("a timestamp"),
                ImportCounts::default(),
            ))
            .await
            .expect("insert the batch")
    }

    /// A record `batch` owns, and the handle a transaction is derived from.
    async fn owned_record(&self, batch: BatchId, reference: &str) -> RecordHandle {
        let record = SourceRecord::new(
            identify(
                &Account::new("Saxo", "69900/1000000"),
                &IdentitySource::BrokerReference(reference),
            ),
            Order::new(1),
            "raw",
            BTreeMap::new(),
        );
        self.database
            .source_records()
            .insert(batch, &record)
            .await
            .expect("insert the record")
    }

    async fn derive(&self, batch: BatchId, transaction: &Transaction) -> TransactionId {
        self.database
            .transactions()
            .insert(
                &Placement::derived(Account::new("Saxo", "69900/1000000"), philips_isin(), batch),
                transaction,
            )
            .await
            .expect("store the transaction")
    }
}

/// Deleting a batch is refused while a transaction derived from it takes part in an attribution,
/// then while a record it owns is cited by a transaction it did not derive, each refusal a 409
/// problem naming the transactions that hold it and removing nothing; once neither holds, the
/// deletion is a 204 that removes the batch, its records and what was derived from them [SRV-022],
/// [SRV-021], [DOM-072], [DOM-119], [ARC-020], [TST-005].
#[tokio::test]
async fn deleting_a_batch_is_refused_naming_what_holds_it() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;
    harness.json(Method::POST, "/securities", &philips()).await;
    let batch = harness.saxo_batch("2024.xlsx").await;
    let b1 = harness.owned_record(batch, "b1").await;
    let opening = harness.derive(batch, &buy_of(b1.clone())).await;
    let s1 = harness.owned_record(batch, "s1").await;
    let closing = harness.derive(batch, &sell_of(2, s1)).await;
    let attribution = harness
        .database
        .attributions()
        .approve(
            closing,
            &[Allocation::new(opening, Quantity::new(dec!(10.00000000)))],
        )
        .await
        .expect("approve");
    // Another import's transaction citing this batch's record: a multi-file event.
    let other = harness.saxo_batch("2025.xlsx").await;
    let citing = harness.derive(other, &sell_of(3, b1)).await;
    let uri = format!("/imports/{batch}");

    let detail = harness
        .request(Method::DELETE, &uri)
        .await
        .assert_problem(
            StatusCode::CONFLICT,
            "urn:fifolio:problem:batch-transaction-attributed",
        )
        .expect("a detail")
        .to_owned();
    assert!(
        detail.ends_with(&format!("{opening}, {closing}")),
        "the refusal names both attributed transactions: {detail}"
    );

    harness
        .database
        .attributions()
        .delete(attribution)
        .await
        .expect("delete the attribution");
    let detail = harness
        .request(Method::DELETE, &uri)
        .await
        .assert_problem(
            StatusCode::CONFLICT,
            "urn:fifolio:problem:batch-records-cited",
        )
        .expect("a detail")
        .to_owned();
    assert!(
        detail.ends_with(&format!(": {citing}")),
        "the refusal names the citing transaction and only it: {detail}"
    );

    harness
        .database
        .transactions()
        .delete(citing)
        .await
        .expect("delete the citing transaction");
    assert_eq!(
        harness.request(Method::GET, &uri).await.status,
        StatusCode::OK,
        "no refused deletion removes the batch"
    );
    assert_eq!(harness.count("source_record").await, 2);
    assert_eq!(harness.count("transaction_record").await, 2);

    let deleted = harness.request(Method::DELETE, &uri).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    harness
        .request(Method::GET, &uri)
        .await
        .assert_problem(StatusCode::NOT_FOUND, "urn:fifolio:problem:unknown-batch");
    assert_eq!(harness.count("source_record").await, 0);
    assert_eq!(harness.count("transaction_record").await, 0);
}

/// Deleting the batch of a file posted again undoes that import only: every record the file
/// stated survives, back with the first import, which is left as the one batch [SRV-021],
/// (DEC-092, provisional), [SRV-052], [TST-005].
#[tokio::test]
async fn deleting_a_re_import_leaves_the_first_import_standing() {
    let harness = with_trade_republic_account().await;
    let query = format!("{TRADE_REPUBLIC}&format=trade_republic_de_csv&filename=a.csv");
    let file = trade_republic_fixture("transactions_2022-01-01_2022-12-31.csv");
    let first = harness.post_file(&query, file.clone()).await;
    let records = harness.count("source_record").await;
    let second = harness.post_file(&query, file).await;

    let deleted = harness
        .request(
            Method::DELETE,
            &format!("/imports/{}", second.body["batch"]),
        )
        .await;

    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    assert!(records > 0, "the fixture stores records");
    assert_eq!(harness.count("source_record").await, records);
    let listed = harness.request(Method::GET, "/imports").await.body;
    assert_eq!(
        listed
            .as_array()
            .expect("a list")
            .iter()
            .map(|batch| batch["id"].clone())
            .collect::<Vec<_>>(),
        [first.body["batch"].clone()]
    );
}

/// The path of the Saxo account's record `reference`, its identity percent-encoded.
fn saxo_record_uri(reference: &str) -> String {
    let identity = identify(
        &Account::new("Saxo", "69900/1000000"),
        &IdentitySource::BrokerReference(reference),
    );
    format!("/source-records/{}", identity.as_str().replace('/', "%2F"))
}

/// The identities a list reply carries, in its order.
fn listed_identities(reply: &Reply) -> Vec<String> {
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply
        .body
        .as_array()
        .expect("a list")
        .iter()
        .map(|record| record["identity"].as_str().expect("an identity").to_owned())
        .collect()
}

/// Source records are read one at a time and listed, filtered on account, batch, security and
/// consumed or pending, each record with its account, owning batch and status; the pending list
/// is the completion queue, which a record leaves while a transaction cites it and returns to
/// once that transaction is deleted, the way a mistake is corrected (DEC-119, DEC-120,
/// provisional) [SRV-023], [SRV-024], [SRV-027], [TST-005].
#[tokio::test]
async fn source_records_are_read_and_listed_with_filters() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;
    harness.json(Method::POST, "/securities", &philips()).await;
    let saxo_account = Account::new("Saxo", "69900/1000000");
    harness
        .import_record(&saxo_account, "r1", &[("Instrument ISIN", "NL0000009538")])
        .await;
    harness
        .import_record(&saxo_account, "r2", &[("Acties", "Storting")])
        .await;
    let batch = harness.saxo_batch("2024.xlsx").await;
    let s1 = harness.owned_record(batch, "s1").await;
    let buy = harness.derive(batch, &buy_of(s1)).await;
    let id = |reference: &str| {
        identify(&saxo_account, &IdentitySource::BrokerReference(reference))
            .as_str()
            .to_owned()
    };
    let list = |query: &'static str| {
        let harness = &harness;
        async move {
            listed_identities(
                &harness
                    .request(Method::GET, &format!("/source-records{query}"))
                    .await,
            )
        }
    };

    let all = harness.request(Method::GET, "/source-records").await;
    assert_eq!(listed_identities(&all), [id("r1"), id("r2"), id("s1")]);
    assert_eq!(
        all.body[0],
        json!({
            "identity": id("r1"),
            "account": {"broker": "Saxo", "id": "69900/1000000"},
            "batch": 1,
            "order": 1,
            "raw": "raw",
            "fields": {"Instrument ISIN": "NL0000009538"},
            "status": "pending",
        })
    );
    assert_eq!(all.body[2]["status"], "consumed");
    assert_eq!(all.body[2]["batch"], batch.get());
    assert_eq!(
        list("?broker=Saxo&account=69900%2F1000000").await,
        [id("r1"), id("r2"), id("s1")]
    );
    assert_eq!(list("?batch=2").await, [id("r2")]);
    assert_eq!(list("?security=NL0000009538").await, [id("r1")]);
    assert_eq!(list("?status=consumed").await, [id("s1")]);
    assert_eq!(
        list("?status=pending&security=NL0000009538").await,
        [id("r1")]
    );

    let read = harness.request(Method::GET, &saxo_record_uri("r1")).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    assert_eq!(read.body, all.body[0]);

    assert_eq!(list("?status=pending").await, [id("r1"), id("r2")]);
    harness
        .database
        .transactions()
        .delete(buy)
        .await
        .expect("delete the derived transaction");
    assert_eq!(
        list("?status=pending").await,
        [id("r1"), id("r2"), id("s1")],
        "deleting the transaction returns its record to the queue"
    );
}

/// An absent record is a 404; a filter naming half an account or an unknown status is a 400,
/// and one naming an account, batch or security that is not stored is a 404 rather than an
/// empty queue (DEC-120, provisional) [SRV-023], [ARC-020], [TST-005].
#[tokio::test]
async fn an_absent_record_or_filter_is_refused() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;

    harness
        .request(Method::GET, &saxo_record_uri("absent"))
        .await
        .assert_problem(StatusCode::NOT_FOUND, "urn:fifolio:problem:unknown-record");
    for (query, status, problem_type) in [
        ("broker=Saxo", StatusCode::BAD_REQUEST, ABOUT_BLANK),
        (
            "account=69900%2F1000000",
            StatusCode::BAD_REQUEST,
            ABOUT_BLANK,
        ),
        ("status=answered", StatusCode::BAD_REQUEST, ABOUT_BLANK),
        (
            "broker=Saxo&account=nobody",
            StatusCode::NOT_FOUND,
            "urn:fifolio:problem:unknown-account",
        ),
        (
            "batch=7",
            StatusCode::NOT_FOUND,
            "urn:fifolio:problem:unknown-batch",
        ),
        (
            "security=US0378331005",
            StatusCode::NOT_FOUND,
            "urn:fifolio:problem:unknown-security",
        ),
    ] {
        harness
            .request(Method::GET, &format!("/source-records?{query}"))
            .await
            .assert_problem(status, problem_type);
    }
}

/// Source records are never edited: the routing table serves only reads under
/// `/source-records`, asserted over the table itself rather than by convention, and a write
/// method there is a 405 [SRV-027], [TST-005].
#[tokio::test]
async fn no_route_edits_a_source_record() {
    let spec = serde_json::to_value(fifolio_server::openapi()).expect("the spec serializes");
    let paths = spec["paths"].as_object().expect("paths");
    let record_paths: Vec<&String> = paths
        .keys()
        .filter(|path| path.starts_with("/source-records"))
        .collect();
    assert_eq!(
        record_paths,
        ["/source-records", "/source-records/{identity}"]
    );
    for path in record_paths {
        let methods: Vec<&String> = paths[path]
            .as_object()
            .expect("a path item")
            .keys()
            .filter(|key| {
                [
                    "get", "put", "post", "delete", "options", "head", "patch", "trace",
                ]
                .contains(&key.as_str())
            })
            .collect();
        assert_eq!(methods, ["get"], "{path} serves only reads");
    }

    let harness = Harness::new().await;
    for uri in ["/source-records".to_owned(), saxo_record_uri("r1")] {
        for method in [Method::PUT, Method::PATCH, Method::POST, Method::DELETE] {
            harness
                .request(method.clone(), &uri)
                .await
                .assert_problem(StatusCode::METHOD_NOT_ALLOWED, ABOUT_BLANK);
        }
    }
}

/// The identity text of the Saxo account's record `reference`, as a client sends it.
fn saxo_identity(reference: &str) -> String {
    identify(
        &Account::new("Saxo", "69900/1000000"),
        &IdentitySource::BrokerReference(reference),
    )
    .as_str()
    .to_owned()
}

/// A stock election of `shares` against Philips, answering the Saxo records `references`.
fn stock_entry(shares: &str, references: &[&str]) -> Value {
    json!({
        "account": saxo(),
        "security": "NL0000009538",
        "supplied": {"kind": "election_stock", "shares": shares},
        "answers": references.iter().map(|reference| saxo_identity(reference)).collect::<Vec<_>>(),
    })
}

/// A manual entry is created with 201 and listed for export as stored; posting the same entry
/// again, as a replayed export does, answers 200 with the stored entry and stores nothing, while
/// an entry differing in what was supplied is another entry [SRV-025], [SRV-026], [SRV-048]
/// (DEC-121, provisional), [TST-005].
#[tokio::test]
async fn a_manual_entry_is_created_once_however_often_it_is_posted() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;
    let batch = harness.saxo_batch("2024.xlsx").await;
    harness.owned_record(batch, "r1").await;
    harness.owned_record(batch, "r2").await;

    let created = harness
        .json(
            Method::POST,
            "/manual-entries",
            &stock_entry("12.5", &["r1", "r2"]),
        )
        .await;
    let replayed = harness
        .json(
            Method::POST,
            "/manual-entries",
            &stock_entry("12.50", &["r1", "r2"]),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let mut expected = stock_entry("12.5", &["r1", "r2"]);
    expected["id"] = created.body["id"].clone();
    assert!(expected["id"].is_i64(), "{}", created.body);
    assert_eq!(created.body, expected);
    assert_eq!(replayed.status, StatusCode::OK, "{}", replayed.body);
    assert_eq!(
        replayed.body, expected,
        "the stored entry, as first written"
    );
    assert_eq!(
        harness.request(Method::GET, "/manual-entries").await.body,
        json!([expected])
    );
    // Nine decimals would be refused as a new entry, but the match comes before the scale
    // check, so a replay stating the stored number with more digits is still recognized.
    let longer = harness
        .json(
            Method::POST,
            "/manual-entries",
            &stock_entry("12.500000000", &["r1", "r2"]),
        )
        .await;
    assert_eq!(longer.status, StatusCode::OK, "{}", longer.body);
    assert_eq!(longer.body, expected);
    assert_eq!(harness.count("manual_entry").await, 1);

    let other = harness
        .json(
            Method::POST,
            "/manual-entries",
            &stock_entry("13", &["r1", "r2"]),
        )
        .await;
    assert_eq!(other.status, StatusCode::CREATED, "{}", other.body);
    assert_eq!(harness.count("manual_entry").await, 2);
}

/// Every supplied shape travels as its own `kind`, quantities as decimal strings, and reads
/// back as posted [SRV-025], [SRV-026], [DOM-097], [TST-005].
#[tokio::test]
async fn every_supplied_shape_is_created_and_listed() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;
    let batch = harness.saxo_batch("2024.xlsx").await;
    harness.owned_record(batch, "r1").await;
    let shapes = [
        json!({"kind": "election_stock", "shares": "3"}),
        json!({"kind": "election_cash"}),
        json!({"kind": "split", "ratio": {"numerator": 1, "denominator": 3}}),
        json!({"kind": "exchange", "target": "CA8934631091",
               "ratio": {"numerator": 2, "denominator": 1}}),
        json!({"kind": "disposal", "quantity": "100", "target": "CA8934631091"}),
        json!({"kind": "disposal", "quantity": "0.5", "target": null}),
    ];

    for supplied in &shapes {
        let body = json!({
            "account": saxo(),
            "security": "NL0000009538",
            "supplied": supplied,
            "answers": [saxo_identity("r1")],
        });
        let created = harness.json(Method::POST, "/manual-entries", &body).await;
        assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
        assert_eq!(&created.body["supplied"], supplied);
    }

    let listed = harness.request(Method::GET, "/manual-entries").await.body;
    assert_eq!(
        listed
            .as_array()
            .expect("a list")
            .iter()
            .map(|entry| entry["supplied"].clone())
            .collect::<Vec<_>>(),
        shapes
    );
}

/// A manual entry naming an unstored account or record, naming none or one twice, or carrying a
/// quantity that is not a decimal above zero at its scale, is refused as a problem and nothing is
/// stored (DEC-122, DEC-123, DEC-125, provisional), [SRV-025], [ARC-010], [ARC-020], [TST-005].
#[tokio::test]
async fn a_manual_entry_that_cannot_be_stored_is_refused() {
    let harness = Harness::new().await;

    // The quantity is refused on reading the body, before the account is looked up (DEC-123).
    let detail = harness
        .json(Method::POST, "/manual-entries", &stock_entry("0", &["r1"]))
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, ABOUT_BLANK)
        .map(str::to_owned);
    assert!(
        detail.is_some_and(|detail| detail.contains("above zero")),
        "a zero count is refused ahead of the unknown account"
    );
    harness
        .json(Method::POST, "/manual-entries", &stock_entry("3", &["r1"]))
        .await
        .assert_problem(StatusCode::NOT_FOUND, "urn:fifolio:problem:unknown-account");
    harness.json(Method::POST, "/accounts", &saxo()).await;
    let batch = harness.saxo_batch("2024.xlsx").await;
    harness.owned_record(batch, "r1").await;
    let detail = harness
        .json(
            Method::POST,
            "/manual-entries",
            &stock_entry("3", &["r1", "absent"]),
        )
        .await
        .assert_problem(StatusCode::NOT_FOUND, "urn:fifolio:problem:unknown-record")
        .map(str::to_owned);
    assert!(
        detail.is_some_and(|detail| detail.contains(&saxo_identity("absent"))),
        "the refusal names the absent record"
    );
    harness
        .json(Method::POST, "/manual-entries", &stock_entry("3", &[]))
        .await
        .assert_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "urn:fifolio:problem:manual-entry-answers-nothing",
        );
    let detail = harness
        .json(
            Method::POST,
            "/manual-entries",
            &stock_entry("3", &["r1", "r1"]),
        )
        .await
        .assert_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "urn:fifolio:problem:manual-entry-answers-repeated",
        )
        .map(str::to_owned);
    assert!(
        detail.is_some_and(|detail| detail.contains(&saxo_identity("r1"))),
        "the refusal names the repeated record"
    );
    harness
        .json(
            Method::POST,
            "/manual-entries",
            &stock_entry("3.000000005", &["r1"]),
        )
        .await
        .assert_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "urn:fifolio:problem:unscaled-value",
        );
    let detail = harness
        .json(
            Method::POST,
            "/manual-entries",
            &stock_entry("3,5", &["r1"]),
        )
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, ABOUT_BLANK)
        .map(str::to_owned);
    assert!(
        detail.is_some_and(|detail| detail.contains("shares")),
        "the refusal names the field"
    );
    let mut zero_ratio = stock_entry("3", &["r1"]);
    zero_ratio["supplied"] = json!({"kind": "split", "ratio": {"numerator": 0, "denominator": 1}});
    harness
        .json(Method::POST, "/manual-entries", &zero_ratio)
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, ABOUT_BLANK);
    // A JSON number could only have been read through floating point [ARC-006].
    let mut number = stock_entry("3", &["r1"]);
    number["supplied"]["shares"] = json!(3);
    harness
        .json(Method::POST, "/manual-entries", &number)
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, ABOUT_BLANK);
    // A count of nothing or less is refused, in either quantity-carrying shape (DEC-123).
    let disposal_of = |quantity: &str| {
        let mut entry = stock_entry("3", &["r1"]);
        entry["supplied"] = json!({"kind": "disposal", "quantity": quantity, "target": null});
        entry
    };
    let non_positive = [
        stock_entry("-3", &["r1"]),
        stock_entry("0", &["r1"]),
        stock_entry("-0.00000001", &["r1"]),
        disposal_of("-100"),
    ];
    for entry in &non_positive {
        let detail = harness
            .json(Method::POST, "/manual-entries", entry)
            .await
            .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, ABOUT_BLANK)
            .map(str::to_owned);
        assert!(
            detail.is_some_and(|detail| detail.contains("above zero")),
            "the refusal of {entry} says why"
        );
    }
    let detail = harness
        .json(Method::POST, "/manual-entries", &disposal_of("0"))
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, ABOUT_BLANK)
        .map(str::to_owned);
    assert!(
        detail.is_some_and(|detail| detail.contains("quantity") && detail.contains("above zero")),
        "the disposal's refusal names its own field and says why"
    );
    harness
        .json(Method::POST, "/manual-entries", &disposal_of("0.000000005"))
        .await
        .assert_problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "urn:fifolio:problem:unscaled-value",
        );
    // One step past the largest value at the 8-decimal scale no longer fits a decimal; it is
    // refused, not rounded to 7 decimals and stored as another number [ARC-010].
    for beyond in [
        "792281625142643375935.43950336",
        "792281625142643375935.439503355",
    ] {
        let detail = harness
            .json(
                Method::POST,
                "/manual-entries",
                &stock_entry(beyond, &["r1"]),
            )
            .await
            .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, ABOUT_BLANK)
            .map(str::to_owned);
        assert!(
            detail.is_some_and(|detail| detail.contains("is not a decimal")),
            "{beyond} is refused as no decimal"
        );
    }

    assert_eq!(harness.count("manual_entry").await, 0);

    // The smallest positive quantity sits exactly at the 8-decimal scale [ARC-020]: both
    // bounds admit it.
    let smallest = harness
        .json(
            Method::POST,
            "/manual-entries",
            &stock_entry("0.00000001", &["r1"]),
        )
        .await;
    assert_eq!(smallest.status, StatusCode::CREATED, "{}", smallest.body);
    assert_eq!(smallest.body["supplied"]["shares"], "0.00000001");
    let smallest = harness
        .json(Method::POST, "/manual-entries", &disposal_of("0.00000001"))
        .await;
    assert_eq!(smallest.status, StatusCode::CREATED, "{}", smallest.body);
    assert_eq!(smallest.body["supplied"]["quantity"], "0.00000001");
    let largest = harness
        .json(
            Method::POST,
            "/manual-entries",
            &stock_entry("792281625142643375935.43950335", &["r1"]),
        )
        .await;
    assert_eq!(largest.status, StatusCode::CREATED, "{}", largest.body);
    assert_eq!(
        largest.body["supplied"]["shares"],
        "792281625142643375935.43950335"
    );
    assert_eq!(harness.count("manual_entry").await, 3);
}

/// Undoing the import an entry answered leaves the entry standing, listed as waiting with the
/// identities it expects, and still recognized when its export is replayed; only deleting the
/// entry removes it, after which it is neither listed nor deletable again, and an id never stored
/// or not a number is refused as a problem [SRV-026], [SRV-053],
/// [SRV-048], [SRV-021], [DOM-109], [TST-005].
#[tokio::test]
async fn only_deleting_a_manual_entry_removes_it() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;
    let batch = harness.saxo_batch("2024.xlsx").await;
    harness.owned_record(batch, "r1").await;
    harness.owned_record(batch, "r3").await;
    let other = harness.saxo_batch("2025.xlsx").await;
    harness.owned_record(other, "r2").await;
    let waiting_one = harness
        .json(
            Method::POST,
            "/manual-entries",
            &stock_entry("3", &["r1", "r2"]),
        )
        .await
        .body;
    let complete = harness
        .json(Method::POST, "/manual-entries", &stock_entry("4", &["r2"]))
        .await
        .body;
    // Cites both records of the batch undone below, against file order, so `missing` must
    // follow the entry's own order and list every absent identity [DOM-109].
    let waiting_all = harness
        .json(
            Method::POST,
            "/manual-entries",
            &stock_entry("5", &["r3", "r1"]),
        )
        .await
        .body;
    assert_eq!(
        harness
            .request(Method::GET, "/manual-entries/waiting")
            .await
            .body,
        json!([])
    );

    let undone = harness
        .request(Method::DELETE, &format!("/imports/{batch}"))
        .await;

    assert_eq!(undone.status, StatusCode::NO_CONTENT, "{}", undone.body);
    assert_eq!(
        harness.request(Method::GET, "/manual-entries").await.body,
        json!([waiting_one, complete, waiting_all])
    );
    assert_eq!(
        harness
            .request(Method::GET, "/manual-entries/waiting")
            .await
            .body,
        json!([
            {"entry": waiting_one, "missing": [saxo_identity("r1")]},
            {"entry": waiting_all, "missing": [saxo_identity("r3"), saxo_identity("r1")]},
        ])
    );
    assert_eq!(
        harness
            .json(
                Method::POST,
                "/manual-entries",
                &stock_entry("3", &["r1", "r2"])
            )
            .await
            .status,
        StatusCode::OK,
        "a replayed export recognizes the waiting entry"
    );

    let uri = format!("/manual-entries/{}", waiting_one["id"]);
    let deleted = harness.request(Method::DELETE, &uri).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    assert_eq!(
        harness.request(Method::GET, "/manual-entries").await.body,
        json!([complete, waiting_all])
    );
    assert_eq!(
        harness
            .request(Method::GET, "/manual-entries/waiting")
            .await
            .body,
        json!([{"entry": waiting_all, "missing": [saxo_identity("r3"), saxo_identity("r1")]}])
    );
    harness.request(Method::DELETE, &uri).await.assert_problem(
        StatusCode::NOT_FOUND,
        "urn:fifolio:problem:unknown-manual-entry",
    );
    harness
        .request(Method::DELETE, "/manual-entries/999")
        .await
        .assert_problem(
            StatusCode::NOT_FOUND,
            "urn:fifolio:problem:unknown-manual-entry",
        );
    harness
        .request(Method::DELETE, "/manual-entries/abc")
        .await
        .assert_problem(StatusCode::BAD_REQUEST, ABOUT_BLANK);
}

/// No route edits a manual entry: a mistake is corrected by deleting it and supplying a new one
/// [SRV-027], [SRV-053], [TST-005].
#[tokio::test]
async fn no_route_edits_a_manual_entry() {
    let harness = Harness::new().await;

    for method in [Method::PUT, Method::PATCH] {
        harness
            .json(method, "/manual-entries/1", &stock_entry("3", &["r1"]))
            .await
            .assert_problem(StatusCode::METHOD_NOT_ALLOWED, ABOUT_BLANK);
    }
}

/// The ids a list reply carries, in its order.
fn listed_ids(reply: &Reply) -> Vec<i64> {
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply
        .body
        .as_array()
        .expect("a list")
        .iter()
        .map(|transaction| transaction["id"].as_i64().expect("an id"))
        .collect()
}

fn apple() -> Value {
    json!({
        "isin": "US0378331005",
        "name": "Apple",
        "security_type": "stock",
        "quotation": "per_unit",
    })
}

/// A `transfer_out` of 10 Philips into Apple on 2024-05-`day`, derived from `record`. The
/// target differs from the security the transfer closes, so the two cannot be confused unseen.
fn transfer_out_of(day: u32, record: RecordHandle) -> Transaction {
    TransferOut::new(
        Derivation::new(on(day), vec1![record]),
        Quantity::new(dec!(10.00000000)),
        valued(Money::new(dec!(0.00))),
        Ratio::new(NonZeroU32::MIN, NonZeroU32::MIN),
        Conversion::native(on(day)),
        Isin::new("US0378331005"),
    )
    .into()
}

/// The `transfer_in` a `transfer_out` of `record` on 2024-05-`day` emits.
fn transfer_in_of(day: u32, record: RecordHandle) -> Transaction {
    TransferIn::new(
        Derivation::new(on(day), vec1![record]),
        Quantity::new(dec!(10.00000000)),
        valued(Money::new(dec!(100.00))),
        valued(Money::new(dec!(0.00))),
        on(1),
        DateProvenance::Inherited,
        TransferInSource::CorporateAction,
        Conversion::native(on(day)),
    )
    .into()
}

/// Transactions are read one at a time and listed in canonical order, each with its account,
/// security, deriving batch, citations and its variant's own fields as decimal strings, filtered
/// on account, security, type and an inclusive trade-date range, and on closings no attribution
/// closes (DEC-126, provisional) [SRV-028], [SRV-029], [ARC-006], [TST-005].
#[tokio::test]
async fn transactions_are_read_and_listed_with_filters() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;
    harness.json(Method::POST, "/securities", &philips()).await;
    harness.json(Method::POST, "/securities", &apple()).await;
    let batch = harness.saxo_batch("2024.xlsx").await;
    let elsewhere = harness.elsewhere_buy().await;
    let late = harness.owned_record(batch, "s2").await;
    let late_sell = harness.derive(batch, &sell_of(6, late)).await;
    let b1 = harness.owned_record(batch, "b1").await;
    let buy = harness.derive(batch, &buy_of(b1)).await;
    let s1 = harness.owned_record(batch, "s1").await;
    let early_sell = harness.derive(batch, &sell_of(3, s1)).await;
    harness
        .database
        .attributions()
        .approve(
            early_sell,
            &[Allocation::new(buy, Quantity::new(dec!(10.00000000)))],
        )
        .await
        .expect("approve");
    let list = |query: &'static str| {
        let harness = &harness;
        async move {
            listed_ids(
                &harness
                    .request(Method::GET, &format!("/transactions{query}"))
                    .await,
            )
        }
    };
    let ids =
        |transactions: &[TransactionId]| transactions.iter().map(|id| id.get()).collect::<Vec<_>>();

    let all = harness.request(Method::GET, "/transactions").await;
    assert_eq!(
        listed_ids(&all),
        ids(&[buy, elsewhere, early_sell, late_sell])
    );
    assert_eq!(
        all.body[2],
        json!({
            "id": early_sell.get(),
            "account": {"broker": "Saxo", "id": "69900/1000000"},
            "security": "NL0000009538",
            "derived_by": batch.get(),
            "trade_date": "2024-05-03",
            "cites": [saxo_identity("s1")],
            "detail": {
                "type": "sell",
                "quantity": "10.00000000",
                "unit_price": {"native": "12.000000", "eur": "12.000000"},
                "gross": {"native": "120.00", "eur": "120.00"},
                "fees": {"native": "8.00", "eur": "8.00"},
                "conversion": {
                    "currency": "EUR",
                    "rate": "1.000000",
                    "source": "native",
                    "rate_date": "2024-05-03",
                },
            },
        })
    );

    assert_eq!(
        list("?broker=Saxo&account=69900%2F1000000").await,
        ids(&[buy, early_sell, late_sell])
    );
    assert_eq!(
        list("?broker=Trade%20Republic&account=DE0001").await,
        ids(&[elsewhere])
    );
    assert_eq!(
        list("?security=NL0000009538").await,
        ids(&[buy, early_sell, late_sell])
    );
    assert_eq!(list("?security=US0378331005").await, ids(&[elsewhere]));
    assert_eq!(list("?type=sell").await, ids(&[early_sell, late_sell]));
    assert_eq!(list("?type=buy").await, ids(&[buy, elsewhere]));
    assert_eq!(
        list("?from=2024-05-03&to=2024-05-06").await,
        ids(&[early_sell, late_sell]),
        "both ends of the range are inside it"
    );
    assert_eq!(
        list("?to=2024-05-05").await,
        ids(&[buy, elsewhere, early_sell])
    );
    assert_eq!(list("?unattributed=true").await, ids(&[late_sell]));
    assert_eq!(
        list("?unattributed=false").await,
        ids(&[buy, elsewhere, early_sell, late_sell])
    );
    assert_eq!(list("?unattributed=true&type=buy").await, Vec::<i64>::new());

    let read = harness
        .request(Method::GET, &format!("/transactions/{early_sell}"))
        .await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    assert_eq!(read.body, all.body[2]);
}

/// A `transfer_out` and the `transfer_in` it emitted read back with their own fields, the
/// emitted record deriving from no batch [DOM-090], [SRV-028], [TST-005].
#[tokio::test]
async fn transfers_read_back_with_their_own_fields() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;
    harness.json(Method::POST, "/securities", &philips()).await;
    harness.json(Method::POST, "/securities", &apple()).await;
    let batch = harness.saxo_batch("2024.xlsx").await;
    let t1 = harness.owned_record(batch, "t1").await;
    let transfer_out = harness.derive(batch, &transfer_out_of(4, t1.clone())).await;
    let transfer_in = harness
        .database
        .transactions()
        .insert(
            &Placement::emitted(Account::new("Saxo", "69900/1000000"), philips_isin()),
            &transfer_in_of(4, t1),
        )
        .await
        .expect("store the emitted transfer_in");
    let list = |query: &'static str| {
        let harness = &harness;
        async move {
            listed_ids(
                &harness
                    .request(Method::GET, &format!("/transactions{query}"))
                    .await,
            )
        }
    };

    let out = harness
        .request(Method::GET, &format!("/transactions/{transfer_out}"))
        .await;
    let emitted = harness
        .request(Method::GET, &format!("/transactions/{transfer_in}"))
        .await;

    assert_eq!(
        out.body["detail"],
        json!({
            "type": "transfer_out",
            "quantity": "10.00000000",
            "fees": {"native": "0.00", "eur": "0.00"},
            "ratio": {"numerator": 1, "denominator": 1},
            "target": "US0378331005",
            "conversion": {
                "currency": "EUR",
                "rate": "1.000000",
                "source": "native",
                "rate_date": "2024-05-04",
            },
        })
    );
    assert_eq!(
        out.body["security"], "NL0000009538",
        "listed under what it closes"
    );
    assert_eq!(
        list("?security=US0378331005").await,
        Vec::<i64>::new(),
        "not under its target"
    );
    assert_eq!(list("?type=transfer_out").await, [transfer_out.get()]);
    assert_eq!(list("?type=transfer_in").await, [transfer_in.get()]);
    assert_eq!(emitted.body["derived_by"], Value::Null);
    assert_eq!(
        emitted.body["detail"],
        json!({
            "type": "transfer_in",
            "quantity": "10.00000000",
            "cost_basis": {"native": "100.00", "eur": "100.00"},
            "fees": {"native": "0.00", "eur": "0.00"},
            "acquisition_date": "2024-05-01",
            "date_provenance": "inherited",
            "source": "corporate_action",
            "conversion": {
                "currency": "EUR",
                "rate": "1.000000",
                "source": "native",
                "rate_date": "2024-05-04",
            },
        })
    );
}

/// An absent transaction is a 404; a filter naming half an account, a range ending before it
/// starts, or an unknown type is a 400, and one naming an account or security that is not
/// stored is a 404 rather than an empty list (DEC-126, provisional) [SRV-028], [ARC-020],
/// [TST-005].
#[tokio::test]
async fn an_absent_transaction_or_filter_is_refused() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;

    harness
        .request(Method::GET, "/transactions/99")
        .await
        .assert_problem(
            StatusCode::NOT_FOUND,
            "urn:fifolio:problem:unknown-transaction",
        );
    for (query, status, problem_type) in [
        ("broker=Saxo", StatusCode::BAD_REQUEST, ABOUT_BLANK),
        (
            "account=69900%2F1000000",
            StatusCode::BAD_REQUEST,
            ABOUT_BLANK,
        ),
        (
            "from=2024-05-06&to=2024-05-03",
            StatusCode::BAD_REQUEST,
            ABOUT_BLANK,
        ),
        ("type=dividend", StatusCode::BAD_REQUEST, ABOUT_BLANK),
        ("from=yesterday", StatusCode::BAD_REQUEST, ABOUT_BLANK),
        (
            "broker=Saxo&account=nobody",
            StatusCode::NOT_FOUND,
            "urn:fifolio:problem:unknown-account",
        ),
        (
            "security=US0378331005",
            StatusCode::NOT_FOUND,
            "urn:fifolio:problem:unknown-security",
        ),
    ] {
        harness
            .request(Method::GET, &format!("/transactions?{query}"))
            .await
            .assert_problem(status, problem_type);
    }
}

/// Deleting a derived transaction returns the source record it cited to the pending list;
/// deleting an attributed transaction or an emitted `transfer_in` is refused as a 409 of that
/// rule's own type and removes nothing, and an absent transaction is a 404 [SRV-033], [DOM-069],
/// [DOM-094], [ARC-021], [TST-005].
#[tokio::test]
async fn deleting_a_transaction_returns_its_records_to_pending_or_is_refused() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;
    harness.json(Method::POST, "/securities", &philips()).await;
    let batch = harness.saxo_batch("2024.xlsx").await;
    let b1 = harness.owned_record(batch, "b1").await;
    let buy = harness.derive(batch, &buy_of(b1)).await;
    let s1 = harness.owned_record(batch, "s1").await;
    let sell = harness.derive(batch, &sell_of(3, s1)).await;
    harness.json(Method::POST, "/securities", &apple()).await;
    let t1 = harness.owned_record(batch, "t1").await;
    let transfer_out = harness.derive(batch, &transfer_out_of(4, t1.clone())).await;
    let transfer_in = harness
        .database
        .transactions()
        .insert(
            &Placement::emitted(Account::new("Saxo", "69900/1000000"), philips_isin()),
            &transfer_in_of(4, t1),
        )
        .await
        .expect("store the emitted transfer_in");
    harness
        .database
        .transactions()
        .record_emission(transfer_out, transfer_in)
        .await
        .expect("record the emission");
    let attribution = harness
        .database
        .attributions()
        .approve(
            sell,
            &[Allocation::new(buy, Quantity::new(dec!(10.00000000)))],
        )
        .await
        .expect("approve");
    let pending = || async {
        listed_identities(
            &harness
                .request(Method::GET, "/source-records?status=pending")
                .await,
        )
    };
    assert_eq!(pending().await, Vec::<String>::new());

    let detail = harness
        .request(Method::DELETE, &format!("/transactions/{sell}"))
        .await
        .assert_problem(
            StatusCode::CONFLICT,
            "urn:fifolio:problem:transaction-attributed",
        )
        .expect("a detail")
        .to_owned();
    assert!(detail.contains(&sell.to_string()), "{detail}");
    harness
        .request(Method::DELETE, &format!("/transactions/{transfer_in}"))
        .await
        .assert_problem(
            StatusCode::CONFLICT,
            "urn:fifolio:problem:emitted-transfer-in",
        );
    assert_eq!(harness.count("transaction_record").await, 4);
    assert_eq!(pending().await, Vec::<String>::new());

    harness
        .database
        .attributions()
        .delete(attribution)
        .await
        .expect("delete the attribution");
    let deleted = harness
        .request(Method::DELETE, &format!("/transactions/{sell}"))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    assert_eq!(pending().await, [saxo_identity("s1")]);
    harness
        .request(Method::GET, &format!("/transactions/{sell}"))
        .await
        .assert_problem(
            StatusCode::NOT_FOUND,
            "urn:fifolio:problem:unknown-transaction",
        );

    let deleted = harness
        .request(Method::DELETE, &format!("/transactions/{transfer_out}"))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    assert_eq!(
        pending().await,
        [saxo_identity("s1"), saxo_identity("t1")],
        "the emitted transfer_in went with its transfer_out, so nothing cites t1"
    );

    harness
        .request(Method::DELETE, &format!("/transactions/{sell}"))
        .await
        .assert_problem(
            StatusCode::NOT_FOUND,
            "urn:fifolio:problem:unknown-transaction",
        );
    harness
        .request(Method::DELETE, "/transactions/abc")
        .await
        .assert_problem(StatusCode::BAD_REQUEST, ABOUT_BLANK);
}

/// No endpoint edits a transaction: the routing table serves only reads and a deletion under
/// `/transactions`, asserted over the table itself, and PUT, PATCH or POST there is a 405; a
/// transferred parcel's acquisition date in particular has no route to change it [SRV-054],
/// [DOM-069], [TST-005].
#[tokio::test]
async fn no_route_edits_a_transaction() {
    let spec = serde_json::to_value(fifolio_server::openapi()).expect("the spec serializes");
    let paths = spec["paths"].as_object().expect("paths");
    let transaction_paths: Vec<&String> = paths
        .keys()
        .filter(|path| path.starts_with("/transactions"))
        .collect();
    assert_eq!(transaction_paths, ["/transactions", "/transactions/{id}"]);
    // The exact sets rather than an absence of PUT and PATCH, so an edit behind any other
    // method is caught too. A path item's other keys, such as `parameters`, are not methods.
    let http_methods = [
        "get", "put", "post", "delete", "options", "head", "patch", "trace",
    ];
    for (path, served) in [
        ("/transactions", vec!["get"]),
        ("/transactions/{id}", vec!["delete", "get"]),
    ] {
        let mut methods: Vec<&str> = paths[path]
            .as_object()
            .expect("a path item")
            .keys()
            .map(String::as_str)
            .filter(|key| http_methods.contains(key))
            .collect();
        methods.sort_unstable();
        assert_eq!(methods, served, "{path} serves no edit");
    }

    let harness = Harness::new().await;
    for uri in ["/transactions", "/transactions/1"] {
        for method in [Method::PUT, Method::PATCH, Method::POST] {
            harness
                .json(method, uri, &json!({"acquisition_date": "2020-01-01"}))
                .await
                .assert_problem(StatusCode::METHOD_NOT_ALLOWED, ABOUT_BLANK);
        }
    }
}

impl Harness {
    /// A buy of Apple in a second account, on the first account's buy date, derived by that
    /// account's own batch: what an account filter must leave out.
    async fn elsewhere_buy(&self) -> TransactionId {
        let other = Account::new("Trade Republic", "DE0001");
        self.json(
            Method::POST,
            "/accounts",
            &json!({"broker": "Trade Republic", "id": "DE0001"}),
        )
        .await;
        let batch = self
            .database
            .import_batches()
            .insert(&ImportBatch::new(
                other.clone(),
                "tr.csv",
                SourceFormat::SaxoNlXlsx,
                DateTime::from_timestamp(1_714_608_000, 0).expect("a timestamp"),
                ImportCounts::default(),
            ))
            .await
            .expect("insert the batch");
        let record = self
            .database
            .source_records()
            .insert(
                batch,
                &SourceRecord::new(
                    identify(&other, &IdentitySource::BrokerReference("x1")),
                    Order::new(1),
                    "raw",
                    BTreeMap::new(),
                ),
            )
            .await
            .expect("insert the record");
        self.database
            .transactions()
            .insert(
                &Placement::derived(other, Isin::new("US0378331005"), batch),
                &buy_of(record),
            )
            .await
            .expect("store the transaction")
    }
}

/// A buy, an expiration and a split read back with every field of their own, synthetic figures
/// kept distinct so that no two fields can be swapped unseen, and the type filter reaches each
/// (DEC-126, provisional) [SRV-028], [DOM-010], [ARC-006], [TST-005].
#[tokio::test]
async fn buys_expirations_and_splits_read_back_with_their_own_fields() {
    let harness = Harness::new().await;
    harness.json(Method::POST, "/accounts", &saxo()).await;
    harness.json(Method::POST, "/securities", &philips()).await;
    let batch = harness.saxo_batch("2024.xlsx").await;
    let b1 = harness.owned_record(batch, "b1").await;
    let buy = harness.derive(batch, &buy_of(b1)).await;
    let e1 = harness.owned_record(batch, "e1").await;
    let expiration = harness
        .derive(
            batch,
            &Expiration::new(
                Derivation::new(on(7), vec1![e1]),
                valued(Money::new(dec!(3.25))),
                valued(Money::new(dec!(1.50))),
                Conversion::native(on(7)),
            )
            .into(),
        )
        .await;
    let p1 = harness.owned_record(batch, "p1").await;
    let split = harness
        .derive(
            batch,
            &Split::new(
                Derivation::new(on(8), vec1![p1]),
                Ratio::new(
                    NonZeroU32::new(3).expect("a non-zero numerator"),
                    NonZeroU32::new(2).expect("a non-zero denominator"),
                ),
            )
            .into(),
        )
        .await;
    let detail = |id: TransactionId| {
        let harness = &harness;
        async move {
            let read = harness
                .request(Method::GET, &format!("/transactions/{id}"))
                .await;
            assert_eq!(read.status, StatusCode::OK, "{}", read.body);
            read.body["detail"].clone()
        }
    };
    let list = |query: &'static str| {
        let harness = &harness;
        async move {
            listed_ids(
                &harness
                    .request(Method::GET, &format!("/transactions{query}"))
                    .await,
            )
        }
    };

    assert_eq!(
        detail(buy).await,
        json!({
            "type": "buy",
            "quantity": "100.00000000",
            "unit_price": {"native": "10.000000", "eur": "10.000000"},
            "gross": {"native": "1000.00", "eur": "1000.00"},
            "fees": {"native": "8.00", "eur": "8.00"},
            "origin": "purchase",
            "conversion": {
                "currency": "EUR",
                "rate": "1.000000",
                "source": "native",
                "rate_date": "2024-05-01",
            },
        })
    );
    assert_eq!(
        detail(expiration).await,
        json!({
            "type": "expiration",
            "gross": {"native": "3.25", "eur": "3.25"},
            "fees": {"native": "1.50", "eur": "1.50"},
            "conversion": {
                "currency": "EUR",
                "rate": "1.000000",
                "source": "native",
                "rate_date": "2024-05-07",
            },
        })
    );
    assert_eq!(
        detail(split).await,
        json!({
            "type": "split",
            "ratio": {"numerator": 3, "denominator": 2},
        })
    );
    assert_eq!(list("?type=buy").await, [buy.get()]);
    assert_eq!(list("?type=expiration").await, [expiration.get()]);
    assert_eq!(list("?type=split").await, [split.get()]);
}
