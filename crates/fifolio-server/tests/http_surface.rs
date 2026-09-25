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

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use chrono::DateTime;
use fifolio_core::entities::{
    Account, ImportBatch, ImportCounts, Isin, Order, Quotation, Security, SecurityType,
    SourceFormat, SourceRecord,
};
use fifolio_core::identity::{IdentitySource, identify};
use fifolio_core::manual_entry::{Election, ManualEntry, Supplied};
use fifolio_core::storage::Database;
use fifolio_server::problem::{ABOUT_BLANK, CONTENT_TYPE};
use fifolio_test_support::TempDb;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

/// The server as a request sees it, over a database of its own.
struct Harness {
    /// Held so the directory outlives the router; dropping it removes the database.
    _dir: TempDb,
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
            _dir: dir,
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
    ] {
        assert!(spec["paths"].get(path).is_some(), "{path} is undocumented");
    }
    assert_eq!(
        spec["components"]["schemas"]["SecurityTypeBody"]["enum"],
        json!(["stock", "bond", "etf", "fund", "derivative", "other"])
    );
}
