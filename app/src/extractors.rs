use crate::auth::{decode_token, Claims};
use crate::errors::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::header;
use axum::http::request::Parts;
use sqlx::FromRow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct ActiveCache {
    inner: Mutex<HashMap<String, (bool, Instant)>>,
}

impl Default for ActiveCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ActiveCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, key: &str) -> Option<bool> {
        let mut g = self.inner.lock().unwrap();
        let entry = g.get(key)?;
        if entry.1.elapsed() > Duration::from_secs(30) {
            g.remove(key);
            return None;
        }
        Some(entry.0)
    }

    pub fn set(&self, key: &str, active: bool) {
        let mut g = self.inner.lock().unwrap();
        g.insert(key.to_string(), (active, Instant::now()));
        if g.len() > 4096 {
            g.retain(|_, (_, ts)| ts.elapsed() <= Duration::from_secs(30));
        }
    }
}

static ACTIVE_CACHE: std::sync::OnceLock<ActiveCache> = std::sync::OnceLock::new();

fn active_cache() -> &'static ActiveCache {
    ACTIVE_CACHE.get_or_init(ActiveCache::new)
}

#[derive(Debug, FromRow)]
struct StaffActiveRow {
    active: bool,
}

#[derive(Clone)]
pub struct AuthUser {
    pub claims: Claims,
}

impl AuthUser {
    pub fn hospital_id(&self) -> &str {
        &self.claims.hospital
    }

    pub fn staff_id(&self) -> &str {
        &self.claims.sub
    }

    pub fn has_role(&self, role: &str) -> bool {
        self.claims.roles.iter().any(|r| r == role)
    }

    pub fn require_role(&self, role: &str) -> ApiResult<()> {
        if self.has_role(role) {
            Ok(())
        } else {
            Err(ApiError::forbidden(format!("requires role {role}")))
        }
    }
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    Arc<AppState>: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| ApiError::unauthorized("missing bearer token"))?;
        let app = Arc::<AppState>::from_ref(state);
        let claims = decode_token(&app.cfg.jwt_secret, token)
            .map_err(|_| ApiError::unauthorized("invalid or expired token"))?;

        if active_cache().get(&claims.sub).is_none() {
            let cache = active_cache();
            // Set the tenant from the (verified) token claim so RLS lets us see
            // the staff row; the bare pool has no tenant context under RLS.
            let mut tx = crate::db::begin_as_tenant(&app.pool, &claims.hospital)
                .await
                .map_err(|_| ApiError::internal("auth backend unavailable"))?;
            let row =
                sqlx::query_as::<_, StaffActiveRow>("SELECT active FROM staff WHERE id = $1::uuid")
                    .bind(&claims.sub)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|_| ApiError::internal("auth backend unavailable"))?;
            let _ = tx.commit().await;
            let active = row.map(|r| r.active).unwrap_or(false);
            cache.set(&claims.sub, active);
        }

        if !active_cache().get(&claims.sub).unwrap_or(false) {
            return Err(ApiError::unauthorized("staff account is deactivated"));
        }

        Ok(AuthUser { claims })
    }
}
