// End-to-end tenant isolation test. It drives the real router against a real
// Postgres as the least-privileged app role, so it exercises the RLS path.
//
// Gated on ANAMNESIS_TEST_DATABASE_URL (the app-role URL). When unset the test
// no-ops, so `cargo test` stays green on machines without a database.
// ANAMNESIS_TEST_ADMIN_URL (superuser) is used only to provision staff; it
// falls back to the app URL if the app role happens to be privileged.

use anamnesis::auth::hash_password;
use anamnesis::config::Config;
use anamnesis::routes::router;
use anamnesis::state::AppState;
use axum::http::StatusCode;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tower::ServiceExt;

const H1: &str = "11111111-1111-1111-1111-111111111111";
const H2: &str = "22222222-2222-2222-2222-222222222222";
const PASS: &str = "Iso-Test-Passw0rd!";

fn test_config(database_url: &str) -> Config {
    // Constructed directly (not from_env) so the test never races other tests
    // over process-wide environment variables.
    Config {
        database_url: database_url.to_string(),
        listen_addr: "127.0.0.1:0".into(),
        metrics_addr: "127.0.0.1:0".into(),
        jwt_secret: "integration-test-secret".into(),
        jwt_ttl_seconds: 3600,
        max_connections: 5,
        outbox_interval_ms: 2000,
        oidc_issuer: None,
        oidc_client_id: None,
        nats_url: None,
        webhook_url: None,
        otel_endpoint: None,
    }
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = axum::http::Request::builder().method(method).uri(path);
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => builder
            .header("content-type", "application/json")
            .body(axum::body::Body::from(b.to_string()))
            .unwrap(),
        None => builder.body(axum::body::Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

async fn login(app: &Router, hospital: &str, user: &str) -> String {
    let (status, body) = call(
        app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({ "username": user, "password": PASS, "hospital_id": hospital })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "login failed: {body}");
    body["token"]
        .as_str()
        .expect("token in response")
        .to_string()
}

#[tokio::test]
async fn cross_tenant_patient_is_not_visible() {
    let Ok(app_url) = std::env::var("ANAMNESIS_TEST_DATABASE_URL") else {
        eprintln!("skipping tenant_isolation: ANAMNESIS_TEST_DATABASE_URL not set");
        return;
    };
    let admin_url = std::env::var("ANAMNESIS_TEST_ADMIN_URL").unwrap_or_else(|_| app_url.clone());

    // Provision one active admin per hospital (as superuser, so RLS is bypassed
    // for the bootstrap insert exactly like the provision_admin tool).
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&admin_url)
        .await
        .expect("connect admin db");
    let hash = hash_password(PASS).expect("hash");
    for (hospital, username) in [(H1, "iso_admin_1"), (H2, "iso_admin_2")] {
        sqlx::query(
            "INSERT INTO staff (hospital_id, username, name, role_title, roles, active, password_hash) \
             VALUES ($1::uuid, $2, $2, 'Admin', ARRAY['doctor','admin'], true, $3) \
             ON CONFLICT (hospital_id, username) \
             DO UPDATE SET password_hash = EXCLUDED.password_hash, active = true",
        )
        .bind(hospital)
        .bind(username)
        .bind(&hash)
        .execute(&admin)
        .await
        .expect("provision staff");
    }

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&app_url)
        .await
        .expect("connect app db");
    let state = Arc::new(AppState {
        pool,
        cfg: Arc::new(test_config(&app_url)),
    });
    let app = router(state);

    let token1 = login(&app, H1, "iso_admin_1").await;
    let token2 = login(&app, H2, "iso_admin_2").await;

    // Create a patient in hospital 1.
    let (status, body) = call(
        &app,
        "POST",
        "/api/v1/patients",
        Some(&token1),
        Some(json!({ "full_name": "Iso Patient", "birth_date": "1990-01-01", "sex": "F" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create patient failed: {body}");
    let patient_id = body["id"].as_str().expect("patient id").to_string();

    // Hospital 2 must not be able to read it.
    let (cross, _) = call(
        &app,
        "GET",
        &format!("/api/v1/patients/{patient_id}"),
        Some(&token2),
        None,
    )
    .await;
    assert_eq!(
        cross,
        StatusCode::NOT_FOUND,
        "a patient must be invisible across tenants"
    );

    // Hospital 1 can.
    let (same, _) = call(
        &app,
        "GET",
        &format!("/api/v1/patients/{patient_id}"),
        Some(&token1),
        None,
    )
    .await;
    assert_eq!(same, StatusCode::OK, "owner hospital must still read it");
}
