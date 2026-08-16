use std::sync::OnceLock;

static NATS: OnceLock<Option<async_nats::Client>> = OnceLock::new();

pub async fn init(url: &str) -> anyhow::Result<()> {
    let client = async_nats::connect(url).await?;
    let _ = NATS.set(Some(client));
    tracing::info!(nats_url = %url, "event bus connected");
    Ok(())
}

/// Publish an event. `Ok(true)` = delivered, `Ok(false)` = no bus configured
/// (nothing to deliver to), `Err` = a configured bus rejected it (retry).
pub async fn publish(subject: &str, payload: &str) -> anyhow::Result<bool> {
    match NATS.get().and_then(|c| c.as_ref()) {
        Some(client) => {
            let bytes: bytes::Bytes = payload.to_string().into();
            client.publish(subject.to_string(), bytes).await?;
            Ok(true)
        }
        None => Ok(false),
    }
}

pub fn enabled() -> bool {
    NATS.get().map(|c| c.is_some()).unwrap_or(false)
}
