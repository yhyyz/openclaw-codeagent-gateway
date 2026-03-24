use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::net::TcpListener;
use tokio::signal;

use agent_gateway::app::AppState;
use agent_gateway::api::router::build_router;
use agent_gateway::auth::audit::AuditLog;
use agent_gateway::auth::quota::QuotaTracker;
use agent_gateway::auth::tenant::TenantRegistry;
use agent_gateway::config::load_config;
use agent_gateway::dispatch::webhook::WebhookDispatcher;
use agent_gateway::observability::metrics;
use agent_gateway::observability::tracing_setup::init_tracing;
use agent_gateway::runtime::pool::ProcessPool;
use agent_gateway::scheduler::patrol::patrol_loop;
use agent_gateway::scheduler::store::JobStore;

#[derive(Parser)]
#[command(name = "agw", version, about = "Agent Gateway — multi-tenant AI agent HTTP gateway")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Serve {
        #[arg(short, long, default_value = "gateway.yaml")]
        config: String,

        #[arg(long)]
        host: Option<String>,

        #[arg(long)]
        port: Option<u16>,

        #[arg(short, long)]
        verbose: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve {
            config: config_path,
            host,
            port,
            verbose,
        } => {
            let mut config = load_config(&config_path)?;

            if let Some(h) = host {
                config.server.host = h;
            }
            if let Some(p) = port {
                config.server.port = p;
            }

            let log_level = if verbose {
                "debug"
            } else {
                &config.observability.log_level
            };
            let json_format = config.observability.log_format == "json";
            init_tracing(log_level, json_format);

            metrics::init_metrics(config.observability.metrics_enabled);

            let store_path = &config.store.path;
            if let Some(parent) = std::path::Path::new(store_path).parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            let job_store = JobStore::open(store_path)?;

            let recovered = job_store.recover_stale()?;
            if !recovered.is_empty() {
                tracing::warn!(count = recovered.len(), "recovered stale jobs from previous run");
            }

            let tenant_registry = TenantRegistry::from_config(&config);
            let quota_tracker = QuotaTracker::new();
            let process_pool = ProcessPool::new();

            let audit_log = if config.observability.audit_path.is_empty() {
                AuditLog::noop()
            } else {
                AuditLog::new(std::path::PathBuf::from(
                    &config.observability.audit_path,
                ))
            };

            let webhook_dispatcher = WebhookDispatcher::new(
                config.callback.retry_max,
                config.callback.retry_base_delay_secs,
            );

            let watchdog_interval = config.pool.watchdog_interval_secs;
            let stuck_timeout = config.pool.stuck_timeout_secs;
            let retention = config.store.job_retention_secs;

            let state = AppState {
                config: Arc::new(config),
                tenant_registry: Arc::new(tenant_registry),
                quota_tracker: Arc::new(quota_tracker),
                audit_log: Arc::new(audit_log),
                process_pool: Arc::new(process_pool),
                job_store: Arc::new(job_store),
                webhook_dispatcher: Arc::new(webhook_dispatcher),
                start_time: std::time::Instant::now(),
            };

            let patrol_state = state.clone();
            tokio::spawn(async move {
                patrol_loop(
                    patrol_state,
                    watchdog_interval,
                    stuck_timeout as i64,
                    retention as i64,
                )
                .await;
            });

            let router = build_router(state.clone());

            let bind_addr = format!(
                "{}:{}",
                state.config.server.host, state.config.server.port
            );
            let listener = TcpListener::bind(&bind_addr).await?;
            tracing::info!(%bind_addr, "agent-gateway listening");

            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown_signal())
                .await?;

            tracing::info!("shutting down");
            state.process_pool.shutdown();
        }
    }

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
