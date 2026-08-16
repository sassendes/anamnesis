use crate::db::begin_as_tenant;
use crate::errors::ApiResult;
use crate::extractors::AuthUser;
use crate::routes::query_params::QueryParams;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/audit", get(list_audit))
}

#[derive(Debug, FromRow, Serialize)]
struct AuditEvent {
    id: i64,
    hospital_id: Uuid,
    table_name: String,
    record: serde_json::Value,
    action: String,
    before: Option<serde_json::Value>,
    after: Option<serde_json::Value>,
    by_user: Option<String>,
    at: chrono::DateTime<chrono::Utc>,
}

async fn list_audit(
    State(state): State<Arc<AppState>>,
    user: AuthUser,
    Query(qp): Query<QueryParams>,
) -> ApiResult<Json<Value>> {
    let limit = qp.page.unwrap_or(200).clamp(1, 1000);

    // Filter in SQL under the tenant context so we never pull another
    // hospital's audit rows into the process, and so LIMIT counts our rows.
    let mut tx = begin_as_tenant(&state.pool, user.hospital_id()).await?;
    let events = sqlx::query_as::<_, AuditEvent>(
        r#"
        SELECT id, hospital_id, table_name, record_key, operation,
               old_json AS "before", new_json AS "after", changed_by AS by_user, changed_at AS at
        FROM app_audit
        WHERE hospital_id = app.current_hospital_id()
        ORDER BY id DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    let events = events
        .into_iter()
        .map(|e| {
            json!({
                "id": e.id,
                "table": e.table_name,
                "record": e.record,
                "action": e.action,
                "before": e.before,
                "after": e.after,
                "by": e.by_user,
                "at": e.at
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(json!({ "audit": events, "count": events.len() })))
}
