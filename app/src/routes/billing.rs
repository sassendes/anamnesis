use crate::db::{begin_as_tenant, begin_as_tenant_staff};
use crate::errors::{ApiError, ApiResult};
use crate::extractors::AuthUser;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/invoices/{id}", get(get_invoice))
        .route("/visits/{id}/invoice", post(bill_visit))
        .route("/invoices/{id}/charge", post(charge_invoice))
}

#[derive(Debug, FromRow, Serialize)]
struct Invoice {
    id: Uuid,
    visit_id: Option<Uuid>,
    patient_id: Uuid,
    amount_cents: i64,
    status: String,
    created_at: chrono::DateTime<chrono::Utc>,
    paid_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn get_invoice(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(invoice_id): Path<Uuid>,
) -> ApiResult<Json<Invoice>> {
    let mut tx = begin_as_tenant(&state.pool, user.hospital_id()).await?;
    let invoice = sqlx::query_as::<_, Invoice>(
        r#"
        SELECT id, visit_id, patient_id, amount_cents, status, created_at, paid_at
        FROM invoices WHERE id = $1 AND hospital_id = app.current_hospital_id()
        "#,
    )
    .bind(invoice_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(invoice))
}

async fn bill_visit(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(visit_id): Path<Uuid>,
) -> ApiResult<Json<Invoice>> {
    let mut tx = begin_as_tenant_staff(&state.pool, user.hospital_id(), user.staff_id()).await?;

    let invoice = sqlx::query_as::<_, Invoice>(
        r#"
        SELECT id, visit_id, patient_id, amount_cents, status, created_at, paid_at
        FROM app.bill_visit($1, $2)
        "#,
    )
    .bind(visit_id)
    .bind(user.staff_id())
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Json(invoice))
}

#[derive(Debug, Deserialize)]
struct ChargeBody {
    amount_cents: i64,
}

async fn charge_invoice(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Path(invoice_id): Path<Uuid>,
    Json(body): Json<ChargeBody>,
) -> ApiResult<Json<Value>> {
    if body.amount_cents <= 0 {
        return Err(ApiError::bad_request("amount_cents must be positive"));
    }
    let mut tx = begin_as_tenant_staff(&state.pool, user.hospital_id(), user.staff_id()).await?;

    // charge_invoice raises on a missing/invalid invoice; surface that as a
    // 409 instead of a generic 500.
    let row = sqlx::query_scalar::<_, bool>("SELECT app.charge_invoice($1, $2, $3)")
        .bind(invoice_id)
        .bind(user.staff_id())
        .bind(body.amount_cents)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(_) => ApiError::conflict("invoice payment refused"),
            other => other.into(),
        })?;

    if row != Some(true) {
        tx.rollback().await?;
        return Err(ApiError::conflict("invoice payment refused"));
    }

    tx.commit().await?;

    if let Some(url) = &state.cfg.webhook_url {
        crate::webhooks::fire(
            url,
            &json!({
                "event": "invoice.charged",
                "invoice_id": invoice_id,
                "amount_cents": body.amount_cents,
                "billed_by": user.staff_id(),
                "hospital_id": user.claims.hospital,
            }),
        );
    }

    Ok(Json(json!({ "ok": true })))
}
