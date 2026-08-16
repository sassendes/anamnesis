use crate::auth::verify_password;
use crate::errors::{ApiError, ApiResult};
use crate::extractors::AuthUser;
use crate::metrics;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A real argon2 hash used to equalise login timing when the username is
/// unknown, so verification runs the same work regardless of user existence.
fn dummy_hash() -> &'static str {
    static H: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    H.get_or_init(|| {
        crate::auth::hash_password("anamnesis-nonexistent-account").unwrap_or_default()
    })
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub hospital_id: String,
}

#[derive(Debug, FromRow)]
struct StaffRow {
    id: String,
    name: String,
    password_hash: String,
    roles: Vec<String>,
    active: bool,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub staff: Value,
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/oidc", post(login_oidc))
        .route("/auth/me", get(me))
}

async fn login(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<LoginRequest>,
) -> ApiResult<Json<Value>> {
    let client = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    if !crate::ratelimit::login_allowed(&client) {
        metrics::record_login("rate_limited");
        return Err(ApiError::too_many_requests(
            "too many login attempts, slow down",
        ));
    }
    fn hs(h: &str, u: &str) -> String {
        format!("{h}:{u}")
    }

    type LockState = HashMap<String, (u32, Option<Instant>)>;

    static LOCKOUT: std::sync::OnceLock<Mutex<LockState>> = std::sync::OnceLock::new();

    fn lockout() -> &'static Mutex<LockState> {
        LOCKOUT.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn backoff(failures: u32) -> Duration {
        Duration::from_secs(5 * 2u64.pow(failures.min(4)))
    }

    fn locked_for(key: &str) -> Option<Duration> {
        let map = lockout().lock().expect("lockout map poisoned");
        let (fails, until) = map.get(key)?;
        match until {
            Some(t) if *t > Instant::now() => Some(*t - Instant::now()),
            Some(_) => None,
            None if *fails > 0 => Some(backoff(*fails)),
            None => None,
        }
    }

    fn record_failure(key: &str) {
        let mut map = lockout().lock().expect("lockout map poisoned");
        let (fails, until) = map.entry(key.to_string()).or_insert((0, None));
        *fails += 1;
        *until = Some(Instant::now() + backoff(*fails));
        if *fails > 4 {
            map.retain(|_, (_, u)| matches!(u, Some(t) if *t > Instant::now()));
        }
    }

    fn clear_failures(key: &str) {
        lockout().lock().expect("lockout map poisoned").remove(key);
    }

    if body.hospital_id.parse::<uuid::Uuid>().is_err() {
        return Err(ApiError::bad_request("invalid hospital_id"));
    }
    let key = hs(&body.hospital_id, &body.username);
    if let Some(wait) = locked_for(&key) {
        metrics::record_login("locked");
        if let Some(url) = &state.cfg.webhook_url {
            crate::webhooks::fire(
                url,
                &json!({
                    "event": "login.throttled",
                    "username": body.username,
                    "hospital_id": body.hospital_id,
                    "message": "account temporarily locked",
                }),
            );
        }
        return Err(ApiError::too_many_requests(format!(
            "too many failed logins, retry in {}s",
            wait.as_secs()
        )));
    }

    // Set the tenant to the requested hospital so RLS scopes the lookup to it;
    // credentials still have to match, this only bounds which rows are visible.
    let mut tx = crate::db::begin_as_tenant(&state.pool, &body.hospital_id).await?;
    let staff = sqlx::query_as::<_, StaffRow>(
        "SELECT id::text AS id, name, password_hash, roles, active FROM staff WHERE hospital_id = $1::uuid AND username = $2",
    )
    .bind(&body.hospital_id)
    .bind(&body.username)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;

    // Always run a password verification, even when the user doesn't exist, so
    // response time doesn't reveal which usernames are valid. The dummy hash is
    // a real argon2 hash so the work factor matches.
    let hash_to_check = staff
        .as_ref()
        .map(|s| s.password_hash.as_str())
        .unwrap_or_else(|| dummy_hash());
    let password_ok = verify_password(&body.password, hash_to_check).unwrap_or(false);

    let staff = match staff {
        Some(staff) if password_ok => staff,
        _ => {
            record_failure(&key);
            metrics::record_login("failure");
            return Err(ApiError::unauthorized("invalid credentials"));
        }
    };

    // Only revealed to someone who already presented valid credentials.
    if !staff.active {
        metrics::record_login("deactivated");
        return Err(ApiError::forbidden("staff account is deactivated"));
    }

    clear_failures(&key);
    metrics::record_login("success");

    let token = crate::auth::encode_token(
        &state.cfg.jwt_secret,
        &staff.id,
        &body.hospital_id,
        &staff.name,
        staff.roles.clone(),
        state.cfg.jwt_ttl_seconds,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(json!({
        "token": token,
        "staff": {
            "id": staff.id,
            "name": staff.name,
            "roles": staff.roles,
            "hospital_id": body.hospital_id
        }
    })))
}

async fn me(State(state): State<Arc<AppState>>, user: AuthUser) -> Result<Json<Value>, ApiError> {
    let mut tx = crate::db::begin_as_tenant(&state.pool, user.hospital_id()).await?;
    let row = sqlx::query_as::<_, (String, String, Vec<String>)>(
        "SELECT name, role_title, roles FROM staff WHERE id = $1::uuid",
    )
    .bind(user.staff_id())
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;

    match row {
        Some((name, role_title, roles)) => Ok(Json(json!({
            "id": user.staff_id(),
            "name": name,
            "role_title": role_title,
            "roles": roles,
            "hospital_id": user.hospital_id()
        }))),
        None => Err(ApiError::unauthorized("token subject not found")),
    }
}

#[derive(serde::Deserialize)]
struct OidcRequest {
    id_token: String,
    hospital_id: String,
}

async fn login_oidc(
    State(state): State<Arc<AppState>>,
    Json(body): Json<OidcRequest>,
) -> ApiResult<Json<Value>> {
    let Some(issuer) = &state.cfg.oidc_issuer else {
        return Err(ApiError::bad_request(
            "OIDC is not enabled on this instance",
        ));
    };
    let Some(client_id) = &state.cfg.oidc_client_id else {
        return Err(ApiError::internal("OIDC client id not configured"));
    };
    if body.hospital_id.parse::<uuid::Uuid>().is_err() {
        return Err(ApiError::bad_request("invalid hospital_id"));
    }
    let oidc_user = crate::oidc::verify_id_token(issuer, client_id, &body.id_token).await?;

    let mut tx = crate::db::begin_as_tenant(&state.pool, &body.hospital_id).await?;
    let staff = sqlx::query_as::<_, StaffRow>(
        "SELECT id::text AS id, name, password_hash, roles, active FROM staff WHERE hospital_id = $1::uuid AND username = $2",
    )
    .bind(&body.hospital_id)
    .bind(&oidc_user.username)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;

    let Some(staff) = staff else {
        metrics::record_login("oidc_unprovisioned");
        if let Some(url) = &state.cfg.webhook_url {
            crate::webhooks::fire(
                url,
                &json!({
                    "event": "login.oidc.unprovisioned",
                    "username": oidc_user.username,
                    "hospital_id": body.hospital_id,
                }),
            );
        }
        return Err(ApiError::forbidden(
            "no account is provisioned for this email; ask an admin",
        ));
    };
    if !staff.active {
        metrics::record_login("deactivated");
        return Err(ApiError::forbidden("staff account is deactivated"));
    }
    metrics::record_login("oidc_success");

    let token = crate::auth::encode_token(
        &state.cfg.jwt_secret,
        &staff.id,
        &body.hospital_id,
        &staff.name,
        staff.roles.clone(),
        state.cfg.jwt_ttl_seconds,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(json!({
        "token": token,
        "staff": {
            "id": staff.id,
            "name": staff.name,
            "roles": staff.roles,
            "hospital_id": body.hospital_id
        }
    })))
}
