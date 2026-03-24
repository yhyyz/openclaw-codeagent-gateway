//! Integration tests — start real server, test via HTTP.

use std::net::TcpListener;
use std::time::Duration;

use reqwest::Client;
use serde_json::Value;

/// Find an available port
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Create a test config YAML string
fn test_config(port: u16) -> String {
    format!(
        r#"
server:
  host: "127.0.0.1"
  port: {port}
  shutdown_timeout_secs: 5
  request_timeout_secs: 30
agents:
  echo-agent:
    enabled: true
    mode: "pty"
    command: "echo"
    pty_args: []
    working_dir: "/tmp"
    description: "Test echo agent"
pool:
  max_processes: 5
  max_per_agent: 3
  idle_timeout_secs: 60
  watchdog_interval_secs: 300
  stuck_timeout_secs: 60
store:
  path: ":memory:"
  job_retention_secs: 600
observability:
  log_level: "warn"
  log_format: "pretty"
gateway:
  allowed_ips: []
tenants:
  test-team:
    credentials:
      - token: "test-token-123"
    policy:
      agents:
        allow: ["echo-agent"]
      operations:
        sync_call: true
        stream: false
        async_jobs: true
        session_manage: true
        admin: true
      resources:
        workspace: "/tmp"
      quotas:
        max_concurrent_sessions: 3
        max_concurrent_jobs: 2
        max_prompt_length: 10000
        session_ttl_hours: 1
      callbacks:
        allowed_urls: []
        allowed_channels: []
  limited-team:
    credentials:
      - token: "limited-token"
    policy:
      agents:
        allow: ["echo-agent"]
      operations:
        sync_call: true
        stream: false
        async_jobs: false
        session_manage: false
        admin: false
      resources:
        workspace: "/tmp"
      quotas:
        max_concurrent_sessions: 1
        max_concurrent_jobs: 0
        max_prompt_length: 100
        session_ttl_hours: 1
      callbacks:
        allowed_urls: []
        allowed_channels: []
"#
    )
}

/// Start server in background, return the port
async fn start_test_server() -> (u16, tokio::task::JoinHandle<()>) {
    let port = free_port();
    let config_str = test_config(port);

    let config_path = format!("/tmp/agw-test-{}.yaml", port);
    std::fs::write(&config_path, &config_str).unwrap();

    let handle = tokio::spawn(async move {
        let config = agent_gateway::config::load_config(&config_path).unwrap();
        let config = std::sync::Arc::new(config);

        let tenant_registry =
            std::sync::Arc::new(agent_gateway::auth::tenant::TenantRegistry::from_config(&config));
        let quota_tracker = std::sync::Arc::new(agent_gateway::auth::quota::QuotaTracker::new());
        let audit_log = std::sync::Arc::new(agent_gateway::auth::audit::AuditLog::noop());
        let process_pool =
            std::sync::Arc::new(agent_gateway::runtime::pool::ProcessPool::new());
        let job_store = std::sync::Arc::new(
            agent_gateway::scheduler::store::JobStore::open(":memory:").unwrap(),
        );
        let webhook_dispatcher = std::sync::Arc::new(
            agent_gateway::dispatch::webhook::WebhookDispatcher::new(0, 1),
        );

        let state = agent_gateway::app::AppState {
            config,
            tenant_registry,
            quota_tracker,
            audit_log,
            process_pool,
            job_store,
            webhook_dispatcher,
            start_time: std::time::Instant::now(),
        };

        let app = agent_gateway::api::router::build_router(state);
        let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port))
            .await
            .unwrap();
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(500)).await;

    (port, handle)
}

fn client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

#[tokio::test]
async fn test_health_endpoint() {
    let (port, handle) = start_test_server().await;
    let resp = client()
        .get(format!("http://127.0.0.1:{}/health", port))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert!(body["version"].is_string());
    handle.abort();
}

#[tokio::test]
async fn test_unauthorized_without_token() {
    let (port, handle) = start_test_server().await;
    let resp = client()
        .get(format!("http://127.0.0.1:{}/agents", port))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    handle.abort();
}

#[tokio::test]
async fn test_unauthorized_with_wrong_token() {
    let (port, handle) = start_test_server().await;
    let resp = client()
        .get(format!("http://127.0.0.1:{}/agents", port))
        .header("Authorization", "Bearer wrong-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    handle.abort();
}

#[tokio::test]
async fn test_list_agents_with_valid_token() {
    let (port, handle) = start_test_server().await;
    let resp = client()
        .get(format!("http://127.0.0.1:{}/agents", port))
        .header("Authorization", "Bearer test-token-123")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let agents = body["agents"].as_array().unwrap();
    assert!(!agents.is_empty());
    assert!(agents.iter().any(|a| a["name"] == "echo-agent"));
    handle.abort();
}

#[tokio::test]
async fn test_admin_forbidden_for_non_admin() {
    let (port, handle) = start_test_server().await;
    let resp = client()
        .get(format!("http://127.0.0.1:{}/admin/tenants", port))
        .header("Authorization", "Bearer limited-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
    handle.abort();
}

#[tokio::test]
async fn test_admin_allowed_for_admin() {
    let (port, handle) = start_test_server().await;
    let resp = client()
        .get(format!("http://127.0.0.1:{}/admin/tenants", port))
        .header("Authorization", "Bearer test-token-123")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert!(body["tenants"].is_array());
    handle.abort();
}

#[tokio::test]
async fn test_submit_job() {
    let (port, handle) = start_test_server().await;
    let resp = client()
        .post(format!("http://127.0.0.1:{}/jobs", port))
        .header("Authorization", "Bearer test-token-123")
        .json(&serde_json::json!({
            "agent": "echo-agent",
            "prompt": "hello world"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 202);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "pending");
    assert!(body["job_id"].is_string());

    tokio::time::sleep(Duration::from_secs(2)).await;

    let job_id = body["job_id"].as_str().unwrap();
    let resp = client()
        .get(format!("http://127.0.0.1:{}/jobs/{}", port, job_id))
        .header("Authorization", "Bearer test-token-123")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    handle.abort();
}

#[tokio::test]
async fn test_close_session() {
    let (port, handle) = start_test_server().await;
    let resp = client()
        .delete(format!(
            "http://127.0.0.1:{}/sessions/echo-agent/test-session",
            port
        ))
        .header("Authorization", "Bearer test-token-123")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    handle.abort();
}
