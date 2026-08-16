use crate::db::begin_as_tenant_staff;
use crate::errors::{ApiError, ApiResult};
use crate::extractors::AuthUser;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/patients/{id}/admissions", post(admit_patient))
        .route("/admissions/{id}/discharge", post(discharge_patient))
        .route("/patients/{id}/prescriptions", post(create_prescription))
        .route("/patients/{id}/lab-orders", post(create_lab_order))
        .route("/lab-results", post(submit_lab_result))
}

#[derive(Debug, FromRow, Serialize)]
struct Admission {
    id: Uuid,
    patient_id: Uuid,
    ward: String,
    bed_id: Option<Uuid>,
    admitted_at: chrono::DateTime<chrono::Utc>,
    discharged_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize)]
struct AdmitRequest {
    ward: String,
    bed_code: Option<String>,
    reason: String,
}

async fn admit_patient(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(patient_id): Path<Uuid>,
    Json(body): Json<AdmitRequest>,
) -> ApiResult<Json<Admission>> {
    let mut tx = begin_as_tenant_staff(&state.pool, user.hospital_id(), user.staff_id()).await?;

    let bed_id = if let Some(code) = &body.bed_code {
        Some(
            sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM beds WHERE hospital_id = app.current_hospital_id() AND code = $1 FOR UPDATE",
            )
            .bind(code)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| ApiError::bad_request(format!("unknown bed {code}")))?,
        )
    } else {
        None
    };

    // INSERT ... SELECT FROM patients so an admission can only reference a
    // patient in the caller's hospital.
    let admission = sqlx::query_as::<_, Admission>(
        r#"
        INSERT INTO admissions (hospital_id, patient_id, ward, bed_id, reason, admitted_by)
        SELECT app.current_hospital_id(), p.id, $2, $3, $4, app.current_staff_id()
        FROM patients p
        WHERE p.id = $1 AND p.hospital_id = app.current_hospital_id()
        RETURNING id, patient_id, ward, bed_id, admitted_at, discharged_at
        "#,
    )
    .bind(patient_id)
    .bind(&body.ward)
    .bind(bed_id)
    .bind(&body.reason)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(admission) = admission else {
        tx.rollback().await?;
        return Err(ApiError::not_found("patient not found"));
    };
    tx.commit().await?;
    Ok(Json(admission))
}

async fn discharge_patient(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(admission_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let mut tx = begin_as_tenant_staff(&state.pool, user.hospital_id(), user.staff_id()).await?;

    let updated = sqlx::query(
        r#"
        UPDATE admissions
        SET discharged_at = now(), discharged_by = app.current_staff_id()
        WHERE id = $1 AND discharged_at IS NULL
          AND hospital_id = app.current_hospital_id()
        "#,
    )
    .bind(admission_id)
    .execute(&mut *tx)
    .await?;

    if updated.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(ApiError::not_found(
            "admission not found or already discharged",
        ));
    }

    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, FromRow, Serialize)]
struct Prescription {
    id: Uuid,
    patient_id: Uuid,
    medication_id: Uuid,
    drug_name: String,
    dosage: f64,
    unit: String,
    frequency: serde_json::Value,
    route: String,
    duration_days: i16,
    issued_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
struct CreatePrescription {
    medication_id: Uuid,
    dosage: f64,
    unit: String,
    frequency: serde_json::Value,
    duration_days: i32,
}

async fn create_prescription(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(patient_id): Path<Uuid>,
    Json(body): Json<CreatePrescription>,
) -> ApiResult<Json<Prescription>> {
    if body.duration_days <= 0 || body.duration_days > 365 {
        return Err(ApiError::bad_request("duration_days out of range"));
    }
    if body.dosage <= 0.0 {
        return Err(ApiError::bad_request("dosage must be positive"));
    }

    let mut tx = begin_as_tenant_staff(&state.pool, user.hospital_id(), user.staff_id()).await?;

    // Join both the patient (tenant-scoped) and the medication so a foreign
    // patient or unknown medication inserts nothing instead of erroring late.
    let prescription = sqlx::query_as::<_, Prescription>(
        r#"
        INSERT INTO prescriptions (
            hospital_id, patient_id, medication_id, drug_name, dosage, unit,
            frequency, route, duration_days, prescribed_by
        )
        SELECT app.current_hospital_id(), p.id, m.id, m.generic_name, $3, $4, $5, m.route, $6,
               app.current_staff_id()
        FROM patients p
        JOIN medications m ON m.id = $2
        WHERE p.id = $1 AND p.hospital_id = app.current_hospital_id()
        RETURNING id, patient_id, medication_id, drug_name, dosage::float8 AS dosage, unit,
                  frequency, route, duration_days, issued_at
        "#,
    )
    .bind(patient_id)
    .bind(body.medication_id)
    .bind(body.dosage)
    .bind(&body.unit)
    .bind(&body.frequency)
    .bind(body.duration_days)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(prescription) = prescription else {
        tx.rollback().await?;
        return Err(ApiError::not_found("patient or medication not found"));
    };
    tx.commit().await?;
    Ok(Json(prescription))
}

#[derive(Debug, Deserialize)]
struct CreateLabOrder {
    panel: String,
    priority: String,
}

async fn create_lab_order(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(patient_id): Path<Uuid>,
    Json(body): Json<CreateLabOrder>,
) -> ApiResult<Json<Value>> {
    if !matches!(body.priority.as_str(), "routine" | "urgent" | "stat") {
        return Err(ApiError::bad_request(
            "priority must be routine, urgent or stat",
        ));
    }

    let mut tx = begin_as_tenant_staff(&state.pool, user.hospital_id(), user.staff_id()).await?;

    // panel_id is NOT NULL and required by submit_lab_result's join, so resolve
    // it from the panel code here instead of leaving it unset.
    let order_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO lab_orders (hospital_id, patient_id, panel_id, panel, priority, requested_by)
        SELECT app.current_hospital_id(), p.id, lp.id, lp.code, $3, app.current_staff_id()
        FROM patients p
        JOIN lab_panels lp ON lp.code = $2
        WHERE p.id = $1 AND p.hospital_id = app.current_hospital_id()
        RETURNING id
        "#,
    )
    .bind(patient_id)
    .bind(&body.panel)
    .bind(&body.priority)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(order_id) = order_id else {
        tx.rollback().await?;
        return Err(ApiError::bad_request("unknown patient or lab panel"));
    };

    tx.commit().await?;
    Ok(Json(json!({ "order_id": order_id })))
}

#[derive(Debug, Deserialize)]
struct LabResultSubmission {
    order_id: Uuid,
    analysis: String,
    value: f64,
    unit: String,
}

async fn submit_lab_result(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(body): Json<LabResultSubmission>,
) -> ApiResult<Json<Value>> {
    let mut tx = begin_as_tenant_staff(&state.pool, user.hospital_id(), user.staff_id()).await?;

    // The join to lab_orders is tenant-scoped, so a result can only be filed
    // against an order in the caller's hospital.
    let result = sqlx::query(
        r#"
        INSERT INTO lab_results (
            hospital_id, order_id, analysis, value, unit, ref_low, ref_high, flag
        )
        SELECT app.current_hospital_id(), lo.id, $2, $3, $4, rr.low, rr.high,
               CASE
                   WHEN $3 < rr.low THEN 'L'
                   WHEN $3 > rr.high THEN 'H'
                   ELSE 'N'
               END
        FROM lab_orders lo
        JOIN lab_panels lp ON lp.id = lo.panel_id
        JOIN lab_reference_ranges rr ON rr.panel_id = lp.id AND rr.analysis = $2
        WHERE lo.id = $1 AND lo.hospital_id = app.current_hospital_id()
        RETURNING id
        "#,
    )
    .bind(body.order_id)
    .bind(&body.analysis)
    .bind(body.value)
    .bind(&body.unit)
    .fetch_optional(&mut *tx)
    .await?;

    if result.is_none() {
        tx.rollback().await?;
        return Err(ApiError::bad_request(
            "unknown order, panel or analysis for this order",
        ));
    }

    sqlx::query(
        "UPDATE lab_orders SET status = 'result_done', completed_at = now() \
         WHERE id = $1 AND hospital_id = app.current_hospital_id()",
    )
    .bind(body.order_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "ok": true, "flag": "applied" })))
}
