use crate::db::begin_as_tenant;
use crate::errors::{ApiError, ApiResult};
use crate::extractors::AuthUser;
use crate::routes::query_params::QueryParams;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/diagnostics/codes", get(list_icd_codes))
        .route("/medications", get(list_medications))
        .route("/labs/orders", get(list_lab_orders))
        .route("/labs/results/{order_id}", get(get_lab_result))
        .route("/results", get(list_results))
}

#[derive(Debug, FromRow, Serialize)]
struct IcdCode {
    code: String,
    description: String,
    category: String,
}

async fn list_icd_codes(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(qp): Query<QueryParams>,
) -> ApiResult<Json<Value>> {
    let term = qp.q.unwrap_or_default().trim().to_string();
    let limit = qp.page.unwrap_or(100).clamp(1, 500);

    let codes = if term.is_empty() {
        sqlx::query_as::<_, IcdCode>(
            "SELECT code, description, category FROM icd_codes ORDER BY code LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as::<_, IcdCode>(
            r#"
            SELECT code, description, category FROM icd_codes
            WHERE code ILIKE $1 || '%' OR description ILIKE '%' || $1 || '%'
            ORDER BY code LIMIT $2
            "#,
        )
        .bind(&term)
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    };

    Ok(Json(json!({ "icd_codes": codes, "count": codes.len() })))
}

#[derive(Debug, FromRow, Serialize)]
struct Medication {
    id: Uuid,
    generic_name: String,
    brand_name: Option<String>,
    route: String,
    strength: String,
}

async fn list_medications(
    State(state): State<Arc<AppState>>,
    _user: AuthUser,
    Query(qp): Query<QueryParams>,
) -> ApiResult<Json<Value>> {
    let term = qp.q.unwrap_or_default().trim().to_string();
    let limit = qp.page.unwrap_or(100).clamp(1, 500);

    let meds = if term.is_empty() {
        sqlx::query_as::<_, Medication>(
            "SELECT id, generic_name, brand_name, route, strength FROM medications ORDER BY generic_name LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as::<_, Medication>(
            r#"
            SELECT id, generic_name, brand_name, route, strength FROM medications
            WHERE generic_name ILIKE '%' || $1 || '%' OR brand_name ILIKE '%' || $1 || '%'
            ORDER BY generic_name LIMIT $2
            "#,
        )
        .bind(&term)
        .bind(limit)
        .fetch_all(&state.pool)
        .await?
    };

    Ok(Json(json!({ "medications": meds, "count": meds.len() })))
}

#[derive(Debug, FromRow, Serialize)]
struct LabOrder {
    id: Uuid,
    patient_id: Uuid,
    panel: String,
    status: String,
    priority: String,
    requested_by: Option<String>,
    requested_at: chrono::DateTime<chrono::Utc>,
}

async fn list_lab_orders(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Query(qp): Query<QueryParams>,
) -> ApiResult<Json<Value>> {
    let patient_id =
        qp.q.as_deref()
            .and_then(|v| v.parse::<Uuid>().ok())
            .ok_or_else(|| ApiError::bad_request("q must be a patient uuid"))?;
    let mut tx = begin_as_tenant(&state.pool, user.hospital_id()).await?;
    let orders = sqlx::query_as::<_, LabOrder>(
        r#"
        SELECT id, patient_id, panel, status, priority, requested_by, requested_at
        FROM lab_orders
        WHERE patient_id = $1 AND hospital_id = app.current_hospital_id()
        ORDER BY requested_at DESC
        LIMIT 100
        "#,
    )
    .bind(patient_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(json!({ "lab_orders": orders, "count": orders.len() })))
}

#[derive(Debug, FromRow, Serialize)]
struct LabResult {
    id: Uuid,
    order_id: Uuid,
    analysis: String,
    value: f64,
    unit: String,
    reference_low: Option<f64>,
    reference_high: Option<f64>,
    flag: String,
}

async fn get_lab_result(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(order_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let mut tx = begin_as_tenant(&state.pool, user.hospital_id()).await?;
    let results = sqlx::query_as::<_, LabResult>(
        r#"
        SELECT id, order_id, analysis, value::float8 AS value, unit,
               ref_low::float8 AS reference_low, ref_high::float8 AS reference_high, flag
        FROM lab_results
        WHERE order_id = $1 AND hospital_id = app.current_hospital_id()
        ORDER BY analysis
        "#,
    )
    .bind(order_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(
        json!({ "lab_results": results, "count": results.len() }),
    ))
}

#[derive(Debug, FromRow, Serialize)]
struct LabResultFlat {
    id: Uuid,
    patient_id: Uuid,
    patient_name: String,
    analysis: String,
    value: f64,
    unit: String,
    flag: String,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn list_results(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Query(qp): Query<QueryParams>,
) -> ApiResult<Json<Value>> {
    let patient_id =
        qp.q.as_deref()
            .and_then(|v| v.parse::<Uuid>().ok())
            .ok_or_else(|| ApiError::bad_request("q must be a patient uuid"))?;
    let mut tx = begin_as_tenant(&state.pool, user.hospital_id()).await?;
    let results = sqlx::query_as::<_, LabResultFlat>(
        r#"
        SELECT lr.id, lo.patient_id, p.full_name AS patient_name,
               lr.analysis, lr.value::float8 AS value, lr.unit, lr.flag, lo.completed_at
        FROM lab_results lr
        JOIN lab_orders lo ON lo.id = lr.order_id
        JOIN patients p ON p.id = lo.patient_id
        WHERE lo.patient_id = $1 AND lo.hospital_id = app.current_hospital_id()
        ORDER BY lo.completed_at DESC NULLS LAST
        LIMIT 100
        "#,
    )
    .bind(patient_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(json!({ "results": results, "count": results.len() })))
}
