//! Integration layer: the real router serving its OpenAPI document in process [TST-003].

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

/// `GET /openapi.json` answers with JSON that is exactly what `fifolio-server openapi` prints,
/// both being read from one route list [SRV-005, SRV-006].
#[tokio::test]
async fn get_openapi_json_returns_the_spec() {
    let response = fifolio_server::router()
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(Body::empty())
                .expect("build the request"),
        )
        .await
        .expect("the router answers");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect the body")
        .to_bytes();
    let served: serde_json::Value = serde_json::from_slice(&body).expect("the body is JSON");
    let printed = serde_json::to_value(fifolio_server::openapi()).expect("the spec serializes");
    assert_eq!(served, printed);
    assert_eq!(served["info"]["title"], "fifolio-server");
}
