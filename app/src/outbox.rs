use crate::metrics::outbox_backlog_gauge;
use crate::state::AppState;
use sqlx::FromRow;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, FromRow)]
struct OutboxRow {
    id: Uuid,
    hospital_id: Uuid,
    event_type: String,
    payload: serde_json::Value,
}

pub fn spawn_dispatcher(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(state.cfg.outbox_interval_ms));
        loop {
            tick.tick().await;
            if let Err(e) = dispatch_once(&state).await {
                tracing::error!(error = %e, "outbox dispatch failed");
            }
        }
    });
}

async fn dispatch_once(state: &Arc<AppState>) -> anyhow::Result<usize> {
    let backlog: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox WHERE dispatched_at IS NULL AND attempts < 10",
    )
    .fetch_one(&state.pool)
    .await?;
    outbox_backlog_gauge().set(backlog);

    let mut tx = state.pool.begin().await?;
    let events = sqlx::query_as::<_, OutboxRow>(
        r#"
        SELECT id, hospital_id, event_type, payload
        FROM outbox
        WHERE dispatched_at IS NULL AND attempts < 10
        ORDER BY created_at
        LIMIT 20
        FOR UPDATE SKIP LOCKED
        "#,
    )
    .fetch_all(&mut *tx)
    .await?;

    let mut delivered = 0usize;
    for event in &events {
        // Serialise the whole jsonb payload, not `as_str()` (which is None for
        // a JSON object and silently shipped an empty "{}").
        let payload = event.payload.to_string();
        let subject = format!("anamnesis.events.{}", event.event_type);

        match crate::events::publish(&subject, &payload).await {
            Ok(published) => {
                // Delivered, or no bus configured: either way it's done.
                tracing::debug!(id = %event.id, hospital = %event.hospital_id,
                    subject = %subject, published, "outbox event handled");
                sqlx::query(
                    "UPDATE outbox SET dispatched_at = now(), attempts = attempts + 1 WHERE id = $1",
                )
                .bind(event.id)
                .execute(&mut *tx)
                .await?;
                delivered += 1;
            }
            Err(e) => {
                // Leave dispatched_at NULL so it is retried until attempts hits 10.
                tracing::warn!(id = %event.id, subject = %subject, error = %e,
                    "outbox publish failed, will retry");
                sqlx::query("UPDATE outbox SET attempts = attempts + 1 WHERE id = $1")
                    .bind(event.id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }

    tx.commit().await?;
    Ok(delivered)
}
