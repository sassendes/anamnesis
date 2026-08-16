use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub listen_addr: String,
    pub metrics_addr: String,
    pub jwt_secret: String,
    pub jwt_ttl_seconds: i64,
    pub max_connections: u32,
    pub outbox_interval_ms: u64,
    pub oidc_issuer: Option<String>,
    pub oidc_client_id: Option<String>,
    pub nats_url: Option<String>,
    pub webhook_url: Option<String>,
    pub otel_endpoint: Option<String>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            database_url: env::var("ANAMNESIS_DATABASE_URL")
                .or_else(|_| env::var("DATABASE_URL"))
                .map_err(|_| anyhow::anyhow!("DATABASE_URL is required"))?,
            listen_addr: env::var("ANAMNESIS_LISTEN_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8080".into()),
            metrics_addr: env::var("ANAMNESIS_METRICS_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:9090".into()),
            jwt_secret: env::var("ANAMNESIS_JWT_SECRET")
                .map_err(|_| anyhow::anyhow!("ANAMNESIS_JWT_SECRET is required"))?,
            jwt_ttl_seconds: env::var("ANAMNESIS_JWT_TTL_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600),
            max_connections: env::var("ANAMNESIS_MAX_CONNECTIONS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(20),
            outbox_interval_ms: env::var("ANAMNESIS_OUTBOX_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2000),
            oidc_issuer: env::var("ANAMNESIS_OIDC_ISSUER")
                .ok()
                .filter(|s| !s.is_empty()),
            oidc_client_id: env::var("ANAMNESIS_OIDC_CLIENT_ID")
                .ok()
                .filter(|s| !s.is_empty()),
            nats_url: env::var("ANAMNESIS_NATS_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            webhook_url: env::var("ANAMNESIS_WEBHOOK_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            otel_endpoint: env::var("ANAMNESIS_OTEL_ENDPOINT")
                .ok()
                .filter(|s| !s.is_empty()),
        })
    }
}
