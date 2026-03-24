//! Application state assembly.

use std::sync::Arc;

use crate::auth::audit::AuditLog;
use crate::auth::quota::QuotaTracker;
use crate::auth::tenant::TenantRegistry;
use crate::config::GatewayConfig;
use crate::dispatch::webhook::WebhookDispatcher;
use crate::runtime::pool::ProcessPool;
use crate::scheduler::store::JobStore;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<GatewayConfig>,
    pub tenant_registry: Arc<TenantRegistry>,
    pub quota_tracker: Arc<QuotaTracker>,
    pub audit_log: Arc<AuditLog>,
    pub process_pool: Arc<ProcessPool>,
    pub job_store: Arc<JobStore>,
    pub webhook_dispatcher: Arc<WebhookDispatcher>,
    pub start_time: std::time::Instant,
}
