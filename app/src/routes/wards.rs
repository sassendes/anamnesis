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
    Router::new().route("/wards", get(list_wards))
}

#[derive(Debug, FromRow, Serialize)]
struct WardRow {
    id: uuid::Uuid,
    code: String,
    name: String,
    beds_total: i64,
    beds_occupied: i64,
}

async fn list_wards(State(state): State<Arc<AppState>>, user: AuthUser) -> ApiResult<Json<Value>> {
    let mut tx = begin_as_tenant(&state.pool, user.hospital_id()).await?;

    let wards = sqlx::query_as::<_, WardRow>(
        r#"
        SELECT w.id, w.code, w.name,
               count(b.id)::bigint AS beds_total,
               count(a.id)::bigint AS beds_occupied
        FROM wards w
        LEFT JOIN beds b ON b.ward_id = w.id
        LEFT JOIN admissions a ON a.bed_id = b.id AND a.discharged_at IS NULL
        GROUP BY w.id
        ORDER BY w.code
        "#,
    )
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "wards": wards, "count": wards.len() })))
}
