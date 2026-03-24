//! Administrative API endpoints.

use axum::extract::{Extension, State};
use axum::Json;
use serde_json::{json, Value};

use crate::app::AppState;
use crate::auth::tenant::Tenant;
use crate::error::Error;

pub async fn list_tenants(
    State(state): State<AppState>,
    Extension(tenant): Extension<Tenant>,
) -> Result<Json<Value>, Error> {
    if !tenant.policy.operations.admin {
        return Err(Error::Forbidden("admin required".into()));
    }
    // Return tenant names (don't expose secrets)
    let names: Vec<&String> = state.config.tenants.keys().collect();
    Ok(Json(json!({ "tenants": names })))
}

pub async fn pool_status(
    State(state): State<AppState>,
    Extension(tenant): Extension<Tenant>,
) -> Result<Json<Value>, Error> {
    if !tenant.policy.operations.admin {
        return Err(Error::Forbidden("admin required".into()));
    }
    let stats = state.process_pool.stats();
    Ok(Json(json!({
        "total": stats.total,
        "by_agent": stats.by_agent
    })))
}
