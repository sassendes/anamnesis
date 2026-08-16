use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub fn init_subscriber(endpoint: &str) -> Option<TracingGuard> {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(format!("{}/v1/traces", endpoint.trim_end_matches('/')))
        .build()
        .ok()?;
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .build();
    use opentelemetry::trace::TracerProvider as _;
    let tracer = provider.tracer("anamnesis");

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "anamnesis=info,tower_http=info".into());
    tracing_subscriber::registry()
        .with(otel_layer)
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .with(filter)
        .init();

    Some(TracingGuard {
        _provider: provider,
    })
}

pub struct TracingGuard {
    _provider: SdkTracerProvider,
}

impl Drop for TracingGuard {
    fn drop(&mut self) {
        tracing::info!("stopping trace exporter");
    }
}
