//! API request handlers.

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::app::AppState;
use crate::auth::tenant::Tenant;
use crate::error::Error;
use crate::scheduler::job::Job;

// ── Request types ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct JobSubmitRequest {
    pub agent: String,
    pub prompt: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub callback: Option<CallbackInput>,
    #[serde(default = "default_true")]
    pub progress_notify: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct CallbackInput {
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub account_id: String,
}

// ── Handlers ────────────────────────────────────────────────────────

pub async fn list_agents(
    State(state): State<AppState>,
    Extension(tenant): Extension<Tenant>,
) -> Result<Json<Value>, Error> {
    let agents: Vec<Value> = state
        .config
        .agents
        .iter()
        .filter(|(name, cfg)| {
            cfg.enabled
                && tenant
                    .policy
                    .agents
                    .allow
                    .iter()
                    .any(|a| a == *name || a == "*")
        })
        .map(|(name, cfg)| {
            json!({
                "name": name,
                "mode": &cfg.mode,
                "description": &cfg.description
            })
        })
        .collect();

    Ok(Json(json!({ "agents": agents })))
}

pub async fn submit_job(
    State(state): State<AppState>,
    Extension(tenant): Extension<Tenant>,
    Json(req): Json<JobSubmitRequest>,
) -> Result<(StatusCode, Json<Value>), Error> {
    // Validate agent
    let _agent_cfg = state
        .config
        .agents
        .get(&req.agent)
        .filter(|a| a.enabled)
        .ok_or_else(|| Error::AgentNotFound(req.agent.clone()))?;

    if !tenant
        .policy
        .agents
        .allow
        .iter()
        .any(|a| a == &req.agent || a == "*")
    {
        return Err(Error::Forbidden(format!(
            "agent '{}' not allowed",
            req.agent
        )));
    }

    // Pre-flight: check pool has capacity for this agent
    if !state.process_pool.has_capacity(
        &req.agent,
        state.config.pool.max_processes,
        state.config.pool.max_per_agent,
    ) {
        return Err(Error::PoolExhausted(format!(
            "no capacity for agent '{}' — try again later",
            req.agent
        )));
    }

    // Create job
    let session_id = req
        .session_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut job = Job::new(&tenant.id, &req.agent, &session_id, &req.prompt);
    job.progress_notify = req.progress_notify;

    if let Some(cb) = req.callback {
        job.callback_url = state.config.callback.default_url.clone();
        job.callback_routing = serde_json::to_string(&crate::scheduler::job::CallbackRequest {
            channel: cb.channel,
            target: cb.target,
            account_id: cb.account_id,
        })
        .unwrap_or_else(|_| "{}".to_string());
    }

    state.job_store.insert(&job)?;

    // Spawn executor
    let executor_state = state.clone();
    let job_id = job.id.clone();
    tokio::spawn(async move {
        crate::scheduler::executor::execute_job(job_id, executor_state).await;
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "job_id": job.id,
            "status": "pending",
            "agent": req.agent,
            "session_id": session_id
        })),
    ))
}

pub async fn list_jobs(
    State(state): State<AppState>,
    Extension(tenant): Extension<Tenant>,
) -> Result<Json<Value>, Error> {
    let jobs = state.job_store.list_by_tenant(&tenant.id, 100)?;
    let items: Vec<Value> = jobs
        .iter()
        .map(|j| {
            json!({
                "id": j.id,
                "agent": j.agent,
                "status": j.status_str(),
                "created_at": j.created_at,
                "completed_at": j.completed_at,
            })
        })
        .collect();
    Ok(Json(json!({ "jobs": items })))
}

pub async fn get_job(
    State(state): State<AppState>,
    Extension(tenant): Extension<Tenant>,
    Path(job_id): Path<String>,
) -> Result<Json<Value>, Error> {
    let job = state
        .job_store
        .get(&job_id)?
        .ok_or_else(|| Error::JobNotFound(job_id.clone()))?;

    // Ensure tenant owns this job
    if job.tenant_id != tenant.id {
        return Err(Error::JobNotFound(job_id));
    }

    let progress_value: serde_json::Value = if job.progress.is_empty() {
        json!({})
    } else {
        serde_json::from_str(&job.progress).unwrap_or_else(|_| json!({}))
    };

    Ok(Json(json!({
        "id": job.id,
        "agent": job.agent,
        "session_id": job.session_id,
        "status": job.status_str(),
        "result": job.result,
        "error": job.error,
        "tools": job.tools,
        "created_at": job.created_at,
        "completed_at": job.completed_at,
        "duration_secs": job.duration_secs(),
        "progress": progress_value,
        "progress_notify": job.progress_notify,
    })))
}

pub async fn close_session(
    State(state): State<AppState>,
    Extension(tenant): Extension<Tenant>,
    Path((agent, session_id)): Path<(String, String)>,
) -> Result<Json<Value>, Error> {
    if !tenant.policy.operations.session_manage {
        return Err(Error::Forbidden("session_manage not permitted".into()));
    }

    state.process_pool.close_session(&agent, &session_id);
    Ok(Json(json!({
        "status": "closed",
        "agent": agent,
        "session_id": session_id
    })))
}
