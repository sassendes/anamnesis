use prometheus::{
    default_registry, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, Opts, TextEncoder,
};
use std::sync::OnceLock;

static HTTP_REQUESTS: OnceLock<HistogramVec> = OnceLock::new();
static HTTP_ERRORS: OnceLock<IntCounterVec> = OnceLock::new();
static LOGIN_ATTEMPTS: OnceLock<IntCounterVec> = OnceLock::new();
static OUTBOX_BACKLOG: OnceLock<IntGauge> = OnceLock::new();

fn http_requests() -> &'static HistogramVec {
    HTTP_REQUESTS.get_or_init(|| {
        let opts = HistogramOpts::new(
            "http_request_duration_seconds",
            "HTTP request latency seconds",
        )
        .const_label("service", "anamnesis")
        .buckets(vec![
            0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
        ]);
        let histogram = HistogramVec::new(opts, &["method", "route", "status"])
            .expect("histogram registration");
        default_registry()
            .register(Box::new(histogram.clone()))
            .ok();
        histogram
    })
}

fn http_errors() -> &'static IntCounterVec {
    HTTP_ERRORS.get_or_init(|| {
        let opts = Opts::new("http_errors_total", "total 5xx responses");
        let counter = IntCounterVec::new(opts, &["route"]).expect("counter_registration");
        default_registry().register(Box::new(counter.clone())).ok();
        counter
    })
}

fn login_attempts() -> &'static IntCounterVec {
    LOGIN_ATTEMPTS.get_or_init(|| {
        let opts = Opts::new("login_attempts_total", "login attempts by outcome");
        let counter = IntCounterVec::new(opts, &["result"]).expect("counter_registration");
        default_registry().register(Box::new(counter.clone())).ok();
        counter
    })
}

pub fn record_login(result: &str) {
    login_attempts().with_label_values(&[result]).inc();
}

pub fn outbox_backlog_gauge() -> &'static IntGauge {
    OUTBOX_BACKLOG.get_or_init(|| {
        let gauge = IntGauge::new("outbox_backlog", "undispatched outbox events").unwrap();
        default_registry().register(Box::new(gauge.clone())).ok();
        gauge
    })
}

pub fn record_http(method: &str, route: &str, status: u16, seconds: f64) {
    http_requests()
        .with_label_values(&[method, route, &status.to_string()])
        .observe(seconds);
    if status >= 500 {
        http_errors().with_label_values(&[route]).inc();
    }
}

/// Times every request and feeds [`record_http`]. Uses the matched route
/// pattern (e.g. `/patients/{id}`) rather than the raw path, so per-id URLs
/// don't explode label cardinality.
pub async fn track_http(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let method = req.method().as_str().to_owned();
    let route = req
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|| req.uri().path().to_owned());

    let start = std::time::Instant::now();
    let resp = next.run(req).await;
    record_http(
        &method,
        &route,
        resp.status().as_u16(),
        start.elapsed().as_secs_f64(),
    );
    resp
}

pub fn metrics_body() -> anyhow::Result<String> {
    let encoder = TextEncoder::new();
    let mut body = String::new();
    encoder.encode_utf8(&default_registry().gather(), &mut body)?;
    Ok(body)
}
