use crate::db::{begin_as_tenant, begin_as_tenant_staff};
use crate::errors::{ApiError, ApiResult};
use crate::extractors::AuthUser;
use crate::routes::query_params::QueryParams;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

// Projected patient columns. numeric columns are cast to float8 and the
// smallint age to its native width so the row decodes into `Patient` cleanly;
// kept in one place so every query stays in sync.
const PATIENT_COLS: &str = "id, mrn, full_name, birth_date, sex, blood_type, \
    weight_kg::float8 AS weight_kg, height_cm::float8 AS height_cm, age_years, \
    phone, email, address, insurance_id, emergency_contact, created_at, updated_at";

// Same idea for vitals: numeric columns cast to float8, smallints kept native.
const VITAL_COLS: &str = "id, patient_id, recorded_at, heart_rate, systolic_bp, diastolic_bp, \
    temperature_c::float8 AS temperature_c, respiratory_rate, spo2, \
    weight_kg::float8 AS weight_kg, height_cm::float8 AS height_cm, bmi::float8 AS bmi";

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/patients", get(list_patients).post(create_patient))
        .route("/patients/{id}", get(get_patient).patch(update_patient))
        .route("/patients/{id}/vitals", get(list_vitals).post(add_vitals))
        .route("/patients/{id}/allergies", post(add_allergy))
        .route("/patients/{id}/diagnoses", post(add_diagnosis))
}

#[derive(Debug, FromRow, Serialize)]
struct Patient {
    id: Uuid,
    mrn: String,
    full_name: String,
    birth_date: chrono::NaiveDate,
    sex: String,
    blood_type: Option<String>,
    weight_kg: Option<f64>,
    height_cm: Option<f64>,
    age_years: Option<i16>,
    phone: Option<String>,
    email: Option<String>,
    address: Option<String>,
    insurance_id: Option<String>,
    emergency_contact: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
struct CreatePatient {
    full_name: String,
    birth_date: chrono::NaiveDate,
    sex: String,
    blood_type: Option<String>,
    weight_kg: Option<f64>,
    height_cm: Option<f64>,
    phone: Option<String>,
    email: Option<String>,
    address: Option<String>,
    insurance_id: Option<String>,
    emergency_contact: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdatePatient {
    full_name: Option<String>,
    blood_type: Option<String>,
    weight_kg: Option<f64>,
    height_cm: Option<f64>,
    phone: Option<String>,
    email: Option<String>,
    address: Option<String>,
    insurance_id: Option<String>,
    emergency_contact: Option<String>,
}

async fn list_patients(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Query(qp): Query<QueryParams>,
) -> ApiResult<Json<Value>> {
    let term = qp.q.unwrap_or_default().trim().to_string();
    let limit = qp.page.unwrap_or(50).clamp(1, 500);

    // Scoped to the caller's hospital: RLS enforces it, and the explicit
    // predicate keeps it safe even if the DB role could bypass RLS.
    let mut tx = begin_as_tenant(&state.pool, user.hospital_id()).await?;
    let patients = if term.is_empty() {
        sqlx::query_as::<_, Patient>(&format!(
            "SELECT {PATIENT_COLS} FROM patients \
             WHERE hospital_id = app.current_hospital_id() \
             ORDER BY full_name LIMIT $1"
        ))
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?
    } else {
        sqlx::query_as::<_, Patient>(&format!(
            "SELECT {PATIENT_COLS} FROM patients \
             WHERE hospital_id = app.current_hospital_id() \
               AND (search_vector @@ websearch_to_tsquery('english', $1) \
                    OR full_name ILIKE '%' || $1 || '%') \
             ORDER BY ts_rank(search_vector, websearch_to_tsquery('english', $1)) DESC \
             LIMIT $2"
        ))
        .bind(&term)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?
    };
    tx.commit().await?;

    Ok(Json(
        json!({ "patients": patients, "count": patients.len() }),
    ))
}

async fn get_patient(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Patient>> {
    let mut tx = begin_as_tenant(&state.pool, user.hospital_id()).await?;
    let patient = sqlx::query_as::<_, Patient>(&format!(
        "SELECT {PATIENT_COLS} FROM patients \
         WHERE id = $1 AND hospital_id = app.current_hospital_id()"
    ))
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(patient))
}

async fn create_patient(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Json(body): Json<CreatePatient>,
) -> ApiResult<Json<Patient>> {
    if body.sex != "F" && body.sex != "M" && body.sex != "O" {
        return Err(ApiError::bad_request("sex must be F, M or O"));
    }
    let mut tx = begin_as_tenant_staff(&state.pool, user.hospital_id(), user.staff_id()).await?;

    let patient = sqlx::query_as::<_, Patient>(&format!(
        "INSERT INTO patients ( \
            hospital_id, mrn, full_name, birth_date, sex, blood_type, weight_kg, height_cm, \
            phone, email, address, insurance_id, emergency_contact \
        ) \
        VALUES ( \
            app.current_hospital_id(), app.next_mrn(), $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11 \
        ) \
        RETURNING {PATIENT_COLS}"
    ))
    .bind(&body.full_name)
    .bind(body.birth_date)
    .bind(&body.sex)
    .bind(&body.blood_type)
    .bind(body.weight_kg)
    .bind(body.height_cm)
    .bind(&body.phone)
    .bind(&body.email)
    .bind(&body.address)
    .bind(&body.insurance_id)
    .bind(&body.emergency_contact)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Json(patient))
}

async fn update_patient(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdatePatient>,
) -> ApiResult<Json<Patient>> {
    let mut tx = begin_as_tenant_staff(&state.pool, user.hospital_id(), user.staff_id()).await?;

    let patient = sqlx::query_as::<_, Patient>(&format!(
        "UPDATE patients SET \
            full_name         = COALESCE($2, full_name), \
            blood_type        = COALESCE($3, blood_type), \
            weight_kg         = COALESCE($4, weight_kg), \
            height_cm         = COALESCE($5, height_cm), \
            phone             = COALESCE($6, phone), \
            email             = COALESCE($7, email), \
            address           = COALESCE($8, address), \
            insurance_id      = COALESCE($9, insurance_id), \
            emergency_contact = COALESCE($10, emergency_contact), \
            updated_at        = now() \
        WHERE id = $1 AND hospital_id = app.current_hospital_id() \
        RETURNING {PATIENT_COLS}"
    ))
    .bind(id)
    .bind(&body.full_name)
    .bind(&body.blood_type)
    .bind(body.weight_kg)
    .bind(body.height_cm)
    .bind(&body.phone)
    .bind(&body.email)
    .bind(&body.address)
    .bind(&body.insurance_id)
    .bind(&body.emergency_contact)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Json(patient))
}

#[derive(Debug, FromRow, Serialize)]
struct VitalSigns {
    id: Uuid,
    patient_id: Uuid,
    recorded_at: chrono::DateTime<chrono::Utc>,
    heart_rate: Option<i16>,
    systolic_bp: Option<i16>,
    diastolic_bp: Option<i16>,
    temperature_c: Option<f64>,
    respiratory_rate: Option<i16>,
    spo2: Option<i16>,
    weight_kg: Option<f64>,
    height_cm: Option<f64>,
    bmi: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct CreateVitals {
    heart_rate: Option<i32>,
    systolic_bp: Option<i32>,
    diastolic_bp: Option<i32>,
    temperature_c: Option<f64>,
    respiratory_rate: Option<i32>,
    spo2: Option<i32>,
    weight_kg: Option<f64>,
    height_cm: Option<f64>,
}

async fn list_vitals(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let mut tx = begin_as_tenant(&state.pool, user.hospital_id()).await?;
    let vitals = sqlx::query_as::<_, VitalSigns>(&format!(
        "SELECT {VITAL_COLS} FROM vital_signs \
         WHERE patient_id = $1 AND hospital_id = app.current_hospital_id() \
         ORDER BY recorded_at DESC LIMIT 100"
    ))
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "vitals": vitals, "count": vitals.len() })))
}

async fn add_vitals(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateVitals>,
) -> ApiResult<Json<VitalSigns>> {
    // Validate before touching the database, not after.
    if body.systolic_bp.is_some() && body.diastolic_bp.is_none() {
        return Err(ApiError::bad_request(
            "diastolic_bp required with systolic_bp",
        ));
    }

    let mut tx = begin_as_tenant_staff(&state.pool, user.hospital_id(), user.staff_id()).await?;

    // INSERT ... SELECT FROM patients so a record can only be attached to a
    // patient that belongs to the caller's hospital; no row means "not found".
    let vital = sqlx::query_as::<_, VitalSigns>(&format!(
        "INSERT INTO vital_signs ( \
            hospital_id, patient_id, heart_rate, systolic_bp, diastolic_bp, \
            temperature_c, respiratory_rate, spo2, weight_kg, height_cm \
        ) \
        SELECT app.current_hospital_id(), p.id, $2, $3, $4, $5, $6, $7, $8, $9 \
        FROM patients p \
        WHERE p.id = $1 AND p.hospital_id = app.current_hospital_id() \
        RETURNING {VITAL_COLS}"
    ))
    .bind(id)
    .bind(body.heart_rate)
    .bind(body.systolic_bp)
    .bind(body.diastolic_bp)
    .bind(body.temperature_c)
    .bind(body.respiratory_rate)
    .bind(body.spo2)
    .bind(body.weight_kg)
    .bind(body.height_cm)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(vital) = vital else {
        tx.rollback().await?;
        return Err(ApiError::not_found("patient not found"));
    };
    tx.commit().await?;
    Ok(Json(vital))
}

#[derive(Debug, Deserialize)]
struct AddAllergy {
    allergen: String,
    severity: String,
    reaction: Option<String>,
}

async fn add_allergy(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<AddAllergy>,
) -> ApiResult<Json<Value>> {
    if !matches!(
        body.severity.as_str(),
        "mild" | "moderate" | "severe" | "anaphylactic"
    ) {
        return Err(ApiError::bad_request(
            "severity must be mild, moderate, severe or anaphylactic",
        ));
    }

    let mut tx = begin_as_tenant_staff(&state.pool, user.hospital_id(), user.staff_id()).await?;

    let done = sqlx::query(
        r#"
        INSERT INTO patient_allergies (hospital_id, patient_id, allergen, severity, reaction)
        SELECT app.current_hospital_id(), p.id, $2, $3, $4
        FROM patients p
        WHERE p.id = $1 AND p.hospital_id = app.current_hospital_id()
        ON CONFLICT (hospital_id, patient_id, allergen) DO NOTHING
        "#,
    )
    .bind(id)
    .bind(&body.allergen)
    .bind(&body.severity)
    .bind(&body.reaction)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    // rows_affected is 0 either for an unknown patient or a duplicate allergen;
    // both are safe to treat as success for an idempotent add.
    Ok(Json(
        json!({ "ok": true, "inserted": done.rows_affected() }),
    ))
}

#[derive(Debug, Deserialize)]
struct AddDiagnosis {
    icd_code: String,
    provisional: bool,
    note: Option<String>,
}

async fn add_diagnosis(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<AddDiagnosis>,
) -> ApiResult<Json<Value>> {
    let mut tx = begin_as_tenant_staff(&state.pool, user.hospital_id(), user.staff_id()).await?;

    // Join both the patient (tenant-scoped) and the ICD code so an unknown
    // patient or an unknown code inserts nothing rather than erroring late.
    let done = sqlx::query(
        r#"
        INSERT INTO diagnoses (hospital_id, patient_id, icd_code, provisional, note)
        SELECT app.current_hospital_id(), p.id, c.code, $2, $3
        FROM patients p
        JOIN icd_codes c ON c.code = $4
        WHERE p.id = $1 AND p.hospital_id = app.current_hospital_id()
        "#,
    )
    .bind(id)
    .bind(body.provisional)
    .bind(&body.note)
    .bind(&body.icd_code)
    .execute(&mut *tx)
    .await?;

    if done.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(ApiError::bad_request("unknown patient or ICD code"));
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}
