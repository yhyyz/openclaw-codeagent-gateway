//! HTTP route definitions.

use axum::middleware::from_fn_with_state;
use axum::routing::{delete, get};
use axum::Router;

use crate::app::AppState;

use super::{admin, handlers, health, middleware};

pub fn build_router(state: AppState) -> Router {
    let public = Router::new().route("/health", get(health::health));

    let authenticated = Router::new()
        .route("/agents", get(handlers::list_agents))
        .route("/jobs", get(handlers::list_jobs).post(handlers::submit_job))
        .route("/jobs/{job_id}", get(handlers::get_job))
        .route(
            "/sessions/{agent}/{session_id}",
            delete(handlers::close_session),
        )
        .route("/sessions/{agent}", get(handlers::list_sessions))
        .route("/health/agents", get(health::health_agents))
        .route("/admin/tenants", get(admin::list_tenants))
        .route("/admin/pool", get(admin::pool_status))
        .layer(from_fn_with_state(state.clone(), middleware::auth_middleware));

    Router::new()
        .merge(public)
        .merge(authenticated)
        .with_state(state)
}
