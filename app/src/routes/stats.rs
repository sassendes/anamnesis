use crate::db::begin_as_tenant;
use crate::errors::ApiResult;
use crate::extractors::AuthUser;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::FromRow;
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/dashboard/stats", get(dashboard_stats))
}

#[derive(Debug, FromRow, Serialize)]
struct DashboardStats {
    total_patients: i64,
    active_admissions: i64,
    pending_lab_orders: i64,
    unpaid_amount_cents: i64,
    diagnoses_last_7d: i64,
    peak_temperature_c: Option<f64>,
    avg_temperature_c: Option<f64>,
}

async fn dashboard_stats(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<Value>> {
    let mut tx = begin_as_tenant(&state.pool, user.hospital_id()).await?;

    let stats = sqlx::query_as::<_, DashboardStats>(
        r#"
        SELECT
            (SELECT count(*) FROM patients) AS total_patients,
            (SELECT count(*) FROM admissions WHERE discharged_at IS NULL) AS active_admissions,
            (SELECT count(*) FROM lab_orders WHERE status = 'pending') AS pending_lab_orders,
            (SELECT coalesce(sum(amount_cents), 0)::bigint FROM invoices WHERE status = 'pending') AS unpaid_amount_cents,
            (SELECT count(*) FROM diagnoses WHERE created_at > now() - interval '7 days') AS diagnoses_last_7d,
            (SELECT max(temperature_c)::float8 FROM vital_signs) AS peak_temperature_c,
            (SELECT avg(temperature_c)::float8 FROM vital_signs) AS avg_temperature_c
    "#,
    )
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(json!({
        "total_patients": stats.total_patients,
        "active_admissions": stats.active_admissions,
        "pending_lab_orders": stats.pending_lab_orders,
        "unpaid_amount_cents": stats.unpaid_amount_cents,
        "diagnoses_last_7d": stats.diagnoses_last_7d,
        "peak_temperature_c": stats.peak_temperature_c,
        "avg_temperature_c": stats.avg_temperature_c
    })))
}
