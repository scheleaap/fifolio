//! Integration layer: the harness driving an `axum` router in process [TST-003].
//!
//! The router is exercised through `tower`'s `oneshot`, so a request travels the real routing,
//! extraction and response path without a socket. The socket is the end-to-end layer's
//! business.
//!
//! The router below is local to this file because the real one arrives with FIF-032, so this
//! is a demonstration of the harness and not coverage of `fifolio-server`: status codes, the
//! problem+json shape, the fingerprint conflict and the pending-records refusal (TST-005) land
//! with FIF-033, which is where this file starts naming ids.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use fifolio_test_support::TempDb;
use http_body_util::BodyExt;
use tower::ServiceExt;

/// The database a router is built over is a real temporary file, not a shared fixture.
fn router(db: &TempDb) -> Router {
    let url = db.url();
    Router::new().route("/probe", get(move || async move { url }))
}

#[tokio::test]
async fn the_harness_can_drive_an_axum_router() {
    let db = TempDb::new();
    let response = router(&db)
        .oneshot(
            Request::builder()
                .uri("/probe")
                .body(Body::empty())
                .expect("build the request"),
        )
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect the body")
        .to_bytes();
    assert_eq!(String::from_utf8_lossy(&body), db.url());
}

#[tokio::test]
async fn the_harness_sees_an_unrouted_path_as_a_404() {
    let db = TempDb::new();
    let response = router(&db)
        .oneshot(
            Request::builder()
                .uri("/nothing-here")
                .body(Body::empty())
                .expect("build the request"),
        )
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
