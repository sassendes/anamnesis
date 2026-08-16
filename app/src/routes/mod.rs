pub mod audit;
pub mod auth;
pub mod billing;
pub mod clinical;
pub mod health;
pub mod labs;
pub mod patients;
pub mod query_params;
pub mod stats;
pub mod wards;

use crate::state::AppState;
use axum::Router;
use std::sync::Arc;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new().nest("/api/v1", api_v1(state))
}

fn api_v1(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(health::routes())
        .merge(auth::routes())
        .merge(patients::routes())
        .merge(clinical::routes())
        .merge(labs::routes())
        .merge(billing::routes())
        .merge(stats::routes())
        .merge(wards::routes())
        .merge(audit::routes())
        .with_state(state)
}
