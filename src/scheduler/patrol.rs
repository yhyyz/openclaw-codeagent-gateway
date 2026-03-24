//! Periodic patrol for stuck/zombie jobs.

use crate::app::AppState;
use crate::scheduler::job::{CallbackRequest, CallbackTarget};

pub async fn patrol_loop(
    state: AppState,
    interval_secs: u64,
    stuck_timeout_secs: i64,
    retention_secs: i64,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
    loop {
        interval.tick().await;

        // 1. Mark stuck jobs
        if let Ok(stuck) = state.job_store.mark_stuck_jobs(stuck_timeout_secs) {
            if stuck > 0 {
                tracing::warn!(count = stuck, "marked stuck jobs as failed");
            }
        }

        // 2. Retry pending webhooks
        if let Ok(pending) = state.job_store.list_pending_webhooks() {
            for job in pending {
                if !job.callback_url.is_empty() {
                    if let Ok(routing) =
                        serde_json::from_str::<CallbackRequest>(&job.callback_routing)
                    {
                        let target = CallbackTarget {
                            url: job.callback_url.clone(),
                            token: if state.config.callback.default_token.is_empty() {
                                None
                            } else {
                                Some(state.config.callback.default_token.clone())
                            },
                            routing,
                        };
                        let sent = state.webhook_dispatcher.deliver(&target, &job).await;
                        if sent {
                            let _ = state.job_store.mark_webhook_sent(&job.id);
                        }
                    }
                }
            }
        }

        // 3. Cleanup old jobs
        if let Ok(cleaned) = state.job_store.cleanup(retention_secs) {
            if cleaned > 0 {
                tracing::info!(count = cleaned, "cleaned old jobs");
            }
        }

        // 4. Cleanup idle pool connections
        state
            .process_pool
            .cleanup_idle(stuck_timeout_secs)
            .await;
    }
}
