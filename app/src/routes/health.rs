use crate::metrics;
use crate::state::AppState;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Instant;

static STARTED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/livez", get(livez))
        .route("/_health", get(healthz))
        .route("/status", get(status))
        .route("/metrics", get(metrics_handler))
}

async fn healthz() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

async fn livez() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

async fn readyz(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    use axum::http::StatusCode;
    let start = Instant::now();
    match verify_database(&state.pool).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "database": "up",
                "latency_ms": start.elapsed().as_millis()
            })),
        ),
        // 503 so Kubernetes actually pulls the pod out of rotation.
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "ok": false, "database": "down" })),
        ),
    }
}

async fn verify_database(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await
        .map(|_| ())
}

async fn status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let started = *STARTED.get_or_init(Instant::now);
    let db_ok = verify_database(&state.pool).await.is_ok();
    Json(json!({
        "ok": db_ok,
        "database": if db_ok { "up" } else { "down" },
        "version": env!("CARGO_PKG_VERSION"),
        "git_version": env!("GIT_VERSION"),
        "uptime_seconds": started.elapsed().as_secs(),
        "metrics_addr": state.cfg.metrics_addr,
    }))
}

async fn metrics_handler() -> impl IntoResponse {
    match metrics::metrics_body() {
        Ok(body) => (axum::http::StatusCode::OK, body).into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "metrics unavailable",
        )
            .into_response(),
    }
}
