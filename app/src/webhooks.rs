use std::time::Duration;

/// Fire-and-forget a webhook. Uses reqwest so https URLs actually get TLS
/// (the old hand-rolled client always spoke plaintext to port 80) and so the
/// call has a real timeout. Must be called from within the tokio runtime.
pub fn fire(url: &str, event: &serde_json::Value) {
    let url = url.to_string();
    let event = event.clone();

    tokio::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(url = %url, error = %e, "webhook client build failed");
                return;
            }
        };
        match client.post(&url).json(&event).send().await {
            Ok(resp) => {
                tracing::debug!(url = %url, status = %resp.status(), "webhook delivered")
            }
            Err(e) => tracing::warn!(url = %url, error = %e, "webhook delivery failed"),
        }
    });
}
