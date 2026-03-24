//! Job execution orchestration.

use std::collections::HashMap;
use std::sync::Arc;

use crate::app::AppState;
use crate::auth::policy::ExecutionContext;
use crate::config::AgentConfig;
use crate::runtime::event::AgentEvent;
use crate::runtime::protocol::{build_initialize, build_prompt, build_session_new};
use crate::scheduler::job::{CallbackRequest, CallbackTarget, Job, JobStatus};
use tokio::sync::broadcast;

#[derive(Default)]
struct AcpResult {
    text: String,
    tool_counts: HashMap<String, usize>,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    total_tokens: u64,
    cost_usd: f64,
}

pub async fn execute_job(job_id: String, state: AppState) {
    let job = match state.job_store.get(&job_id) {
        Ok(Some(j)) => j,
        _ => {
            tracing::error!(%job_id, "job not found in executor");
            return;
        }
    };

    let _ = state
        .job_store
        .update_status(&job_id, &JobStatus::Running, "", "");

    let agent_cfg = match state.config.agents.get(&job.agent) {
        Some(cfg) if cfg.enabled => cfg,
        _ => {
            let _ = state
                .job_store
                .mark_failed(&job_id, "agent not found or disabled");
            return;
        }
    };

    let callback_target = if !job.callback_url.is_empty() {
        serde_json::from_str::<CallbackRequest>(&job.callback_routing)
            .ok()
            .map(|routing| CallbackTarget {
                url: job.callback_url.clone(),
                token: if state.config.callback.default_token.is_empty() {
                    None
                } else {
                    Some(state.config.callback.default_token.clone())
                },
                routing,
            })
    } else {
        None
    };

    let result: Result<AcpResult, String> = if agent_cfg.mode == "pty" {
        match crate::runtime::pty::run_pty(
            &agent_cfg.command,
            &agent_cfg.pty_args,
            &job.prompt,
            &agent_cfg.working_dir,
            &agent_cfg.env,
        )
        .await
        {
            Ok((output, tools)) => {
                let mut tool_counts = HashMap::new();
                for t in &tools {
                    *tool_counts.entry(t.clone()).or_insert(0) += 1;
                }
                Ok(AcpResult {
                    text: output,
                    tool_counts,
                    ..Default::default()
                })
            }
            Err(e) => Err(e.to_string()),
        }
    } else {
        run_acp_prompt(&job, &job_id, agent_cfg, &state, callback_target.as_ref()).await
    };

    match &result {
        Ok(acp) => {
            let tools_vec: Vec<String> = acp.tool_counts.keys().cloned().collect();
            let final_progress = serde_json::json!({
                "tool_counts": acp.tool_counts,
                "usage": {
                    "input_tokens": acp.input_tokens,
                    "output_tokens": acp.output_tokens,
                    "cache_read_tokens": acp.cache_read_tokens,
                    "cache_write_tokens": acp.cache_write_tokens,
                    "total_tokens": acp.total_tokens,
                    "cost_usd": acp.cost_usd,
                },
                "updated_at": chrono::Utc::now().timestamp()
            })
            .to_string();
            let _ = state.job_store.update_progress(&job_id, &final_progress);
            let _ = state
                .job_store
                .mark_completed(&job_id, &acp.text, &tools_vec);
        }
        Err(err) => {
            let _ = state.job_store.mark_failed(&job_id, err);
        }
    }

    if let Some(ref target) = callback_target {
        if let Ok(Some(updated_job)) = state.job_store.get(&job_id) {
            let sent = state.webhook_dispatcher.deliver(target, &updated_job).await;
            if sent {
                let _ = state.job_store.mark_webhook_sent(&job_id);
            }
        }
    }
}

async fn run_acp_prompt(
    job: &Job,
    job_id: &str,
    agent_cfg: &AgentConfig,
    state: &AppState,
    callback_target: Option<&CallbackTarget>,
) -> Result<AcpResult, String> {
    let context = ExecutionContext {
        tenant_id: job.tenant_id.clone(),
        workspace: std::path::PathBuf::from(&agent_cfg.working_dir),
        env_inject: std::collections::HashMap::new(),
        env_deny: std::collections::HashSet::new(),
        session_ttl: std::time::Duration::from_secs(3600),
        idle_timeout: std::time::Duration::from_secs(state.config.pool.idle_timeout_secs),
    };

    let session_guard = state
        .quota_tracker
        .try_acquire_session(&job.tenant_id, 100)
        .map_err(|e| e.to_string())?;

    let process = state
        .process_pool
        .acquire(
            &job.agent,
            &job.session_id,
            agent_cfg,
            &context,
            session_guard,
            state.config.pool.max_processes,
            state.config.pool.max_per_agent,
        )
        .await
        .map_err(|e| format!("pool acquire failed: {}", e))?;

    let init_req = build_initialize(process.next_id(), env!("CARGO_PKG_VERSION"));
    match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        process.send_rpc(&init_req),
    )
    .await
    {
        Ok(Ok(resp)) => {
            if resp.error.is_some() {
                tracing::warn!(job_id = %job_id, "ACP initialize returned error, continuing anyway");
            }
        }
        Ok(Err(e)) => {
            tracing::warn!(job_id = %job_id, error = %e, "ACP initialize failed, trying prompt anyway");
        }
        Err(_) => {
            return Err("ACP initialize timeout (30s)".into());
        }
    }

    let mut acp_session_id = String::from("default");

    let session_new_req =
        build_session_new(process.next_id(), &context.workspace.to_string_lossy());
    match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        process.send_rpc(&session_new_req),
    )
    .await
    {
        Ok(Ok(resp)) => {
            if let Some(result) = &resp.result {
                if let Some(sid) = result.get("sessionId").and_then(|v| v.as_str()) {
                    tracing::debug!(job_id = %job_id, acp_session = %sid, "ACP session created");
                    acp_session_id = sid.to_string();
                }
            }
        }
        Ok(Err(e)) => {
            tracing::warn!(job_id = %job_id, error = %e, "ACP session/new failed, trying prompt anyway");
        }
        Err(_) => {
            return Err("ACP session/new timeout (30s)".into());
        }
    }

    let mut event_rx = process.subscribe();

    let prompt_process = Arc::clone(&process);
    let prompt_req = build_prompt(process.next_id(), &acp_session_id, &job.prompt);
    let prompt_handle = tokio::spawn(async move {
        tokio::time::timeout(
            std::time::Duration::from_secs(900),
            prompt_process.send_rpc(&prompt_req),
        )
        .await
    });
    tokio::pin!(prompt_handle);

    let mut result_text = String::new();
    let mut tool_counts: HashMap<String, usize> = HashMap::new();
    let mut tools_notified: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut tools_completed_count: usize = 0;
    let first_event_time = std::time::Instant::now();
    let mut first_progress_sent = false;
    let mut usage_used: u64 = 0;
    let mut usage_cost: f64 = 0.0;

    loop {
        tokio::select! {
            prompt_result = &mut prompt_handle => {
                match prompt_result {
                    Ok(Ok(Ok(resp))) => {
                        while let Ok(raw) = event_rx.try_recv() {
                            if let Some(evt) = AgentEvent::from_notification(&raw) {
                                match evt {
                                    AgentEvent::TextChunk(t) => result_text.push_str(&t),
                                    AgentEvent::ToolStart { title, .. } => {
                                        if !title.is_empty() {
                                            *tool_counts.entry(title).or_insert(0) += 1;
                                        }
                                    }
                                    AgentEvent::UsageUpdate { used, cost_usd } => {
                                        usage_used = used;
                                        usage_cost = cost_usd;
                                    }
                                    _ => {}
                                }
                            }
                        }

                        let mut input_tokens: u64 = 0;
                        let mut output_tokens: u64 = 0;
                        let mut cache_read_tokens: u64 = 0;
                        let mut cache_write_tokens: u64 = 0;
                        let mut total_tokens: u64 = 0;

                        if let Some(result) = &resp.result {
                            if let Some(usage) = result.get("usage") {
                                input_tokens = usage.get("inputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
                                output_tokens = usage.get("outputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
                                cache_read_tokens = usage.get("cachedReadTokens").and_then(|v| v.as_u64()).unwrap_or(0);
                                cache_write_tokens = usage.get("cachedWriteTokens").and_then(|v| v.as_u64()).unwrap_or(0);
                                total_tokens = usage.get("totalTokens").and_then(|v| v.as_u64()).unwrap_or(0);
                            }
                        }

                        if total_tokens == 0 && usage_used > 0 {
                            total_tokens = usage_used;
                        }
                        if let Some(err) = resp.error {
                            return Err(format!("ACP error: {}", err.message));
                        }

                        return Ok(AcpResult {
                            text: result_text,
                            tool_counts,
                            input_tokens,
                            output_tokens,
                            cache_read_tokens,
                            cache_write_tokens,
                            total_tokens,
                            cost_usd: usage_cost,
                        });
                    }
                    Ok(Ok(Err(e))) => return Err(format!("prompt send failed: {}", e)),
                    Ok(Err(_)) => return Err("prompt timeout (900s)".into()),
                    Err(e) => return Err(format!("prompt task panicked: {}", e)),
                }
            }
            event = event_rx.recv() => {
                match event {
                    Ok(raw) => {
                        if let Some(evt) = AgentEvent::from_notification(&raw) {
                            match evt {
                                AgentEvent::TextChunk(text) => result_text.push_str(&text),
                                AgentEvent::ToolStart { title, .. } => {
                                    if title.is_empty() {
                                        continue;
                                    }
                                    *tool_counts.entry(title.clone()).or_insert(0) += 1;

                                    let progress = serde_json::json!({
                                        "current_tool": title,
                                        "tool_counts": tool_counts,
                                        "updated_at": chrono::Utc::now().timestamp()
                                    }).to_string();
                                    let _ = state.job_store.update_progress(job_id, &progress);

                                    if job.progress_notify && !tools_notified.contains(&title) {
                                        tools_notified.insert(title.clone());
                                        if let Some(target) = callback_target {
                                            if !first_progress_sent {
                                                let elapsed = first_event_time.elapsed();
                                                if elapsed < std::time::Duration::from_secs(5) {
                                                    tokio::time::sleep(std::time::Duration::from_secs(5) - elapsed).await;
                                                }
                                                first_progress_sent = true;
                                            }
                                            let msg = format!("\u{23f3} [{}] {} \u{1f527} {}", job.agent, &job_id[..8.min(job_id.len())], title);
                                            let progress_target = target.clone();
                                            tokio::spawn(async move {
                                                send_progress_webhook(&progress_target, &msg).await;
                                            });
                                        }
                                    }
                                }
                                AgentEvent::ToolDone { title, status, .. } => {
                                    tools_completed_count += 1;
                                    let progress = serde_json::json!({
                                        "current_tool": "",
                                        "tool_counts": tool_counts,
                                        "tools_completed": tools_completed_count,
                                        "last_event": format!("\u{2705} {} ({})", title, status),
                                        "updated_at": chrono::Utc::now().timestamp()
                                    }).to_string();
                                    let _ = state.job_store.update_progress(job_id, &progress);
                                }
                                AgentEvent::Plan(text) => {
                                    let truncated = &text[..text.len().min(100)];
                                    let progress = serde_json::json!({
                                        "current_tool": "",
                                        "plan": text,
                                        "last_event": format!("\u{1f4cb} Plan: {}", truncated),
                                        "updated_at": chrono::Utc::now().timestamp()
                                    }).to_string();
                                    let _ = state.job_store.update_progress(job_id, &progress);

                                    if job.progress_notify {
                                        if let Some(target) = callback_target {
                                            if !first_progress_sent {
                                                let elapsed = first_event_time.elapsed();
                                                if elapsed < std::time::Duration::from_secs(5) {
                                                    tokio::time::sleep(std::time::Duration::from_secs(5) - elapsed).await;
                                                }
                                                first_progress_sent = true;
                                            }
                                            let plan_preview = &text[..text.len().min(200)];
                                            let msg = format!("\u{23f3} [{}] {} \u{1f4cb} {}", job.agent, &job_id[..8.min(job_id.len())], plan_preview);
                                            let progress_target = target.clone();
                                            tokio::spawn(async move {
                                                send_progress_webhook(&progress_target, &msg).await;
                                            });
                                        }
                                    }
                                }
                                AgentEvent::UsageUpdate { used, cost_usd } => {
                                    usage_used = used;
                                    usage_cost = cost_usd;
                                }
                                AgentEvent::Error(err) => return Err(err),
                                _ => {}
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(job_id = %job_id, skipped = n, "event receiver lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        if result_text.is_empty() {
                            return Err("agent process terminated".into());
                        }
                        break;
                    }
                }
            }
        }
    }

    Ok(AcpResult {
        text: result_text,
        tool_counts,
        cost_usd: usage_cost,
        ..Default::default()
    })
}

async fn send_progress_webhook(target: &CallbackTarget, message: &str) {
    let payload = serde_json::json!({
        "tool": "message",
        "args": {
            "action": "send",
            "channel": target.routing.channel,
            "target": target.routing.target,
            "message": message,
        },
        "sessionKey": "main"
    });

    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(ct) = "application/json".parse() {
        headers.insert("Content-Type", ct);
    }
    if let Some(token) = &target.token {
        if let Ok(auth) = format!("Bearer {}", token).parse() {
            headers.insert("Authorization", auth);
        }
    }

    let client = reqwest::Client::new();
    let _ = client
        .post(&target.url)
        .headers(headers)
        .json(&payload)
        .send()
        .await;
}
