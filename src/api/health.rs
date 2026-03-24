//! Health check endpoints.

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::app::AppState;

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": state.start_time.elapsed().as_secs()
    }))
}

pub async fn health_agents(State(state): State<AppState>) -> Json<Value> {
    let stats = state.process_pool.stats();
    Json(json!({
        "agents": {
            "total": stats.total,
            "by_agent": stats.by_agent
        }
    }))
}
