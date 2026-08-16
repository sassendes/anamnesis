use anamnesis::config::Config;
use anamnesis::routes::router;
use anamnesis::state::AppState;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tower::ServiceExt;

fn test_app() -> Router {
    std::env::set_var(
        "ANAMNESIS_DATABASE_URL",
        "postgres://unused:unused@127.0.0.1:1/unused",
    );
    std::env::set_var("ANAMNESIS_JWT_SECRET", "integration-test-secret");
    std::env::set_var("ANAMNESIS_LISTEN_ADDR", "127.0.0.1:0");
    std::env::set_var("ANAMNESIS_METRICS_ADDR", "127.0.0.1:0");

    let cfg = Arc::new(Config::from_env().unwrap());
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy(&cfg.database_url)
        .unwrap();
    let state = Arc::new(AppState { pool, cfg });
    router(state)
}

async fn get(app: &Router, path: &str) -> (axum::http::StatusCode, Value) {
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(path)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

#[tokio::test]
async fn healthz_endpoints_ok() {
    let app = test_app();
    for path in ["/api/v1/healthz", "/api/v1/livez", "/api/v1/_health"] {
        let (status, _) = get(&app, path).await;
        assert_eq!(status, axum::http::StatusCode::OK, "{path} should be 200");
    }
}

#[tokio::test]
async fn unauthenticated_patient_route_is_401() {
    let app = test_app();
    let (status, _) = get(&app, "/api/v1/patients?query=").await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unknown_api_route_404() {
    let app = test_app();
    let (status, _) = get(&app, "/api/v1/nope").await;
    assert_eq!(status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn metrics_endpoint_ok() {
    let app = test_app();
    let (status, _) = get(&app, "/api/v1/metrics").await;
    assert_eq!(status, axum::http::StatusCode::OK);
}

#[tokio::test]
async fn wards_requires_auth() {
    let app = test_app();
    let (status, _) = get(&app, "/api/v1/wards").await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn status_endpoint_reports_database_down_gracefully() {
    let app = test_app();
    let (status, body) = get(&app, "/api/v1/status").await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["ok"], serde_json::Value::Bool(false));
    assert_eq!(body["database"], "down");
}
