use anamnesis::config::Config;
use anamnesis::db::build_pool;
use anamnesis::state::AppState;
use axum::http::header;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{MakeRequestUuid, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Kept alive for the whole process so the OTLP exporter is flushed on
    // shutdown. If OTLP isn't configured, fall back to a plain fmt subscriber.
    let mut trace_guard = None;
    if let Some(endpoint) = std::env::var("ANAMNESIS_OTEL_ENDPOINT")
        .ok()
        .filter(|s| !s.is_empty())
    {
        if let Some(g) = anamnesis::trace::init_subscriber(&endpoint) {
            tracing::info!(endpoint = %endpoint, "otlp tracing enabled");
            trace_guard = Some(g);
        }
    }
    if trace_guard.is_none() {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "anamnesis=info,tower_http=info".into()),
            )
            .init();
    }

    let cfg = Arc::new(Config::from_env()?);
    let pool = build_pool(&cfg.database_url, cfg.max_connections).await?;

    if std::env::var("ANAMNESIS_RUN_MIGRATIONS").as_deref() == Ok("1") {
        sqlx::migrate!("./migrations").run(&pool).await?;
        tracing::info!("migrations applied");
        return Ok(());
    }

    let state = Arc::new(AppState {
        pool: pool.clone(),
        cfg: cfg.clone(),
    });

    anamnesis::outbox::spawn_dispatcher(state.clone());

    if let Some(url) = &cfg.nats_url {
        if let Err(e) = anamnesis::events::init(url).await {
            tracing::warn!(error = %e, "nats unavailable, events will be dropped");
        }
    }

    let app = anamnesis::routes::router(state.clone())
        .layer(RequestBodyLimitLayer::new(2 * 1024 * 1024))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::new(
            header::HeaderName::from_static("x-request-id"),
            MakeRequestUuid,
        ));

    let listener = tokio::net::TcpListener::bind(&cfg.listen_addr).await?;
    tracing::info!(addr = %cfg.listen_addr, version = env!("GIT_VERSION"), "anamnesis serving");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("ctrl-c handler installed");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("sigterm handler installed")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("graceful shutdown requested");
}
