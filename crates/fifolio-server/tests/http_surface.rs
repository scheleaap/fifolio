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

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::middleware::map_response;
use axum::routing::post;
use fifolio_core::entities::{Isin, Quotation, Security, SecurityType};
use fifolio_core::storage::Database;
use fifolio_server::problem::{ABOUT_BLANK, CONTENT_TYPE, Problem, problem_for_bare_errors};
use fifolio_test_support::TempDb;
use http_body_util::BodyExt;
use serde_json::Value;
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

/// A refusal storage makes against the harness's database reaches the client as the problem
/// its variant maps to, with the message naming what collided [ARC-020, ARC-021, TST-005].
///
/// No endpoint yet inserts a security, so the case mounts one that does exactly that beside the
/// served router; FIF-034's endpoint will replace it with the real one. `Router::layer` wraps
/// only the routes added before it, so the probe is given the server's problem layer itself and
/// its problem still passes through that layer on the way out.
#[tokio::test]
async fn a_storage_refusal_is_the_problem_its_variant_maps_to() {
    let harness = Harness::new().await;
    let database = harness.database.clone();
    let harness = Harness {
        router: harness.router.merge(
            Router::new()
                .route(
                    "/probe/security",
                    post(move || async move {
                        let security = Security::new(
                            Isin::new("NL0000009538"),
                            "Philips",
                            SecurityType::Stock,
                            Quotation::PerUnit,
                        );
                        database
                            .securities()
                            .insert(&security)
                            .await
                            .map_err(Problem::from)
                    }),
                )
                .layer(map_response(problem_for_bare_errors)),
        ),
        ..harness
    };

    let first = harness.request(Method::POST, "/probe/security").await;
    assert_eq!(first.status, StatusCode::OK);

    let second = harness.request(Method::POST, "/probe/security").await;
    let detail = second.assert_problem(StatusCode::CONFLICT, "urn:fifolio:problem:duplicate-isin");
    assert!(
        detail.is_some_and(|detail| detail.contains("NL0000009538")),
        "{}",
        second.body
    );
}
