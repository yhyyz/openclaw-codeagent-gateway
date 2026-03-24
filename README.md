# Agent Gateway (agw)

A multi-tenant HTTP gateway that exposes local CLI AI agents (Kiro, Claude Code, OpenCode) over HTTP with permission-driven access control, async job execution, and channel-agnostic webhook callbacks.

## Quick Start

### Prerequisites

- Rust 1.80+ (or use pre-built binary)
- At least one ACP-compatible CLI agent installed:
  - Kiro: `kiro-cli acp -a`
  - Claude Code: `npx -y @zed-industries/claude-agent-acp`
  - OpenCode: `opencode acp`

### Build

```bash
source $HOME/.cargo/env
cargo build --release
# Binary: target/release/agw
```

### Run

```bash
cp gateway.yaml.example gateway.yaml
# Edit gateway.yaml (set tokens, agent paths)
./target/release/agw serve --config gateway.yaml
```

### CLI Options

| Flag | Description | Default |
|------|-------------|---------|
| `--config <path>` | Path to YAML config file | `gateway.yaml` |
| `--host <addr>` | Override `server.host` | from config |
| `--port <port>` | Override `server.port` | from config |
| `--verbose` | Force log level to `debug` | off |

### Verify

```bash
curl http://localhost:8001/health
# {"status":"ok","version":"0.1.0","uptime_secs":5}
```

## Install the Skill

The agent-gateway skill lets any AI coding agent interact with a running gateway.

### Via npx skills (recommended)

```bash
# Install to all detected agents
npx skills add <repo-url>

# Install to specific agents
npx skills add <repo-url> -a openclaw -a claude-code -a opencode

# Install globally (available across all projects)
npx skills add <repo-url> -g -a openclaw
```

### Manual installation

```bash
# For OpenClaw
cp -r skill/ ~/clawd/skills/agent-gateway/

# For Claude Code
cp -r skill/ ~/.claude/skills/agent-gateway/

# For OpenCode
cp -r skill/ ~/.config/opencode/skills/agent-gateway/

# For Kiro CLI
cp -r skill/ ~/.kiro/skills/agent-gateway/
```

After installing, restart your agent or start a new session for the skill to be discovered.

## Architecture

```
HTTP Client (OpenClaw / curl / AI agent)
    │
    ▼
┌──────────────────────┐
│   Agent Gateway       │
│   (agw :8001)         │
│                       │
│   Auth → Policy →     │
│   Process Pool →      │
│   Job Scheduler →     │
│   Webhook Dispatch    │
└──────────┬───────────┘
           │ ACP protocol (JSON-RPC over stdio)
    ┌──────┼──────┐
    ▼      ▼      ▼
  Kiro  Claude  OpenCode
```

### Key Design Decisions

- **Async-only execution**: All jobs submitted via `POST /jobs`, results delivered via webhook callback. No blocking upstream sessions.
- **Channel-agnostic callbacks**: Gateway sends `{channel, target, message}` — doesn't know about Discord/Telegram/Slack. The upstream platform (e.g., OpenClaw) handles routing.
- **Fire-and-forget pattern**: Submit job → get `job_id` → results pushed automatically. No polling needed.
- **Progress webhooks**: Tool starts and plans are pushed to the caller during execution.
- **Process pool with reuse**: Same `(agent, session_id)` reuses the same subprocess — context preserved across turns.
- **Multi-tenant**: Each token maps to a tenant with 5-dimension policy (agents, operations, resources, quotas, callbacks).

## Configuration

### Minimal gateway.yaml

```yaml
server:
  host: "0.0.0.0"
  port: 8001

agents:
  claude:
    enabled: true
    mode: acp
    command: "npx"
    acp_args: ["-y", "@zed-industries/claude-agent-acp"]
    working_dir: "/home/user/work"
    env: {}

  opencode:
    enabled: true
    mode: acp
    command: "opencode"
    acp_args: ["acp"]
    working_dir: "/home/user/work"
    env: {}

  kiro:
    enabled: true
    mode: acp
    command: "kiro-cli"
    acp_args: ["acp", "-a"]
    working_dir: "/home/user/work"
    env: {}

pool:
  max_processes: 20
  max_per_agent: 10
  idle_timeout_secs: 120
  watchdog_interval_secs: 30
  stuck_timeout_secs: 900

store:
  path: "data/gateway.db"
  job_retention_secs: 86400

callback:
  default_url: ""
  default_token: ""
  retry_max: 3
  retry_base_delay_secs: 5

observability:
  log_level: "info"
  log_format: "json"

gateway:
  allowed_ips: []

tenants:
  default:
    credentials:
      - token: "your-secret-token"
    policy:
      agents:
        allow: ["*"]
      operations:
        async_jobs: true
        session_manage: true
        admin: true
      quotas:
        max_concurrent_sessions: 10
        max_concurrent_jobs: 5
        max_prompt_length: 65536
        session_ttl_hours: 24
      callbacks:
        allowed_urls: ["*"]
        allowed_channels:
          - channel: "*"
            targets: ["*"]
```

### Environment variable expansion

All string values in `gateway.yaml` support `${VAR_NAME}` syntax. Before YAML parsing, the gateway replaces every `${...}` occurrence with the corresponding environment variable value. Undefined variables resolve to an empty string.

```yaml
tenants:
  ops:
    credentials:
      - token: "${OPS_TEAM_TOKEN}"
```

### Agent-specific notes

| Agent | Command | Flags | Notes |
|-------|---------|-------|-------|
| Claude Code | `npx -y @zed-industries/claude-agent-acp` | — | Via Zed's ACP adapter. Permissions auto-approved. |
| OpenCode | `opencode acp` | — | Native ACP support. |
| Kiro | `kiro-cli acp -a` | `-a` = trust all tools | Without `-a`, tool calls require manual approval (hangs in headless). Startup ~19s (MCP server init). |
| Codex | `codex exec --full-auto` | PTY mode | Set `mode: pty`, `pty_args: ["exec", "--full-auto"]`. Not ACP — one-shot execution. Experimental. |

### Token usage reporting

| Agent | Input/Output | Cache Read/Write | Cost | Total |
|-------|-------------|-----------------|------|-------|
| Claude Code | ✅ | ✅ | ✅ | ✅ |
| OpenCode | — | — | ✅ | ✅ (total only) |
| Kiro | — | — | — | — |

### ACP mode vs PTY mode

| Aspect | ACP (`"acp"`) | PTY (`"pty"`) |
|--------|---------------|---------------|
| Process lifecycle | Long-running, managed by process pool | One-shot per invocation |
| Communication | JSON-RPC over stdin/stdout | Prompt passed as CLI argument, stdout captured |
| Session support | Yes — process reused across calls with same session_id | No — each call is independent |
| Arguments field | `acp_args` | `pty_args` |
| Output processing | JSON-RPC response parsing | ANSI escape code stripping |
| Status | Production | Experimental |

### Full configuration schema

#### `server` — HTTP server settings

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `host` | string | `"0.0.0.0"` | Listen address |
| `port` | integer | `8001` | Listen port |
| `shutdown_timeout_secs` | integer | `30` | Graceful shutdown timeout (seconds) |
| `request_timeout_secs` | integer | `300` | Per-request timeout (seconds) |

#### `agents` — Agent definitions

| Field | Type | Default | Required | Description |
|-------|------|---------|----------|-------------|
| `enabled` | boolean | `true` | no | Whether this agent is active |
| `mode` | string | — | **yes** | `"acp"` or `"pty"` |
| `command` | string | — | **yes** | Path to the agent executable |
| `acp_args` | list of string | `[]` | no | Arguments for ACP mode |
| `pty_args` | list of string | `[]` | no | Arguments for PTY mode |
| `working_dir` | string | `"."` | no | Working directory for the agent process |
| `description` | string | `""` | no | Human-readable description |
| `env` | map of string → string | `{}` | no | Environment variables injected into the agent process |

#### `pool` — Process pool settings

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_processes` | integer | `8` | Global maximum active processes |
| `max_per_agent` | integer | `4` | Maximum processes per agent type |
| `idle_timeout_secs` | integer | `600` | Reclaim idle processes after this many seconds |
| `watchdog_interval_secs` | integer | `10` | Health check loop interval (seconds) |
| `stuck_timeout_secs` | integer | `900` | Force-fail jobs running longer than this (seconds) |

#### `store` — Persistent storage

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `path` | string | `"data/gateway.db"` | SQLite database file path |
| `job_retention_secs` | integer | `604800` (7 days) | How long to keep completed job records |

#### `callback` — Webhook settings

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `default_url` | string | `""` | Default callback URL (if job doesn't specify one) |
| `default_token` | string | `""` | Default auth token for callback requests |
| `retry_max` | integer | `3` | Maximum delivery retry attempts |
| `retry_base_delay_secs` | integer | `5` | Base delay between retries (exponential backoff) |

#### `observability` — Logging and metrics

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `log_level` | string | `"info"` | `trace`, `debug`, `info`, `warn`, `error` |
| `log_format` | string | `"json"` | `json` or `text` |
| `metrics_enabled` | boolean | `false` | Enable metrics collection |
| `audit_path` | string | `""` | Audit log file path (empty = disabled) |

#### `gateway` — Network security

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `allowed_ips` | list of string | `[]` | IP allowlist (CIDR format). Empty = allow all |
| `rate_limit.requests_per_minute` | integer | — | Required if `rate_limit` is set |
| `rate_limit.burst` | integer | `10` | Burst capacity above the per-minute rate |

#### `tenants` — Multi-tenant configuration (5-dimension policy)

**Dimension 1: `policy.agents`** — Agent access

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `allow` | list of string | — | **Required.** Allowed agent names. `"*"` = all agents |
| `deny` | list of string | `[]` | Denied agent names (takes priority over allow) |

**Dimension 2: `policy.operations`** — Operation permissions

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `async_jobs` | boolean | `false` | Allow `POST /jobs` |
| `session_manage` | boolean | `false` | Allow `DELETE /sessions/...` |
| `admin` | boolean | `false` | Allow `/admin/*` endpoints |

**Dimension 3: `policy.resources`** — Resource isolation

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `workspace` | string | `"/tmp/agw-workspaces"` | Tenant's workspace root directory |
| `env_inject` | map of string → string | `{}` | Extra env vars injected into agent processes |
| `env_deny` | list of string | `[]` | Env var names blocked from reaching agents |

**Dimension 4: `policy.quotas`** — Rate and resource limits

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_concurrent_sessions` | integer | `5` | Max active sessions for this tenant |
| `max_concurrent_jobs` | integer | `10` | Max active async jobs |
| `max_prompt_length` | integer | `32768` | Max prompt length in characters |
| `session_ttl_hours` | integer | `24` | Session time-to-live (hours) |

**Dimension 5: `policy.callbacks`** — Callback restrictions

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `allowed_urls` | list of string | `[]` | Allowed callback URL patterns (`*` suffix wildcard) |
| `allowed_channels[].channel` | string | — | Platform name (e.g., `"telegram"`, `"slack"`, `"*"`) |
| `allowed_channels[].targets` | list of string | — | Allowed targets (e.g., `"#ops-alerts"`, `"*"`) |

### Validation rules

The gateway validates the config at startup and refuses to start if:

1. `tenants` is empty — at least one tenant must be configured
2. No agent has `enabled: true` — at least one active agent is required
3. Any enabled agent has a `mode` other than `"acp"` or `"pty"`

## API Reference

### Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/health` | No | Liveness check |
| `GET` | `/agents` | Bearer | List agents (filtered by tenant policy) |
| `POST` | `/jobs` | Bearer | Submit async job |
| `GET` | `/jobs` | Bearer | List tenant's jobs |
| `GET` | `/jobs/{id}` | Bearer | Job status + progress + result |
| `DELETE` | `/sessions/{agent}/{sid}` | Bearer | Close agent session |
| `GET` | `/health/agents` | Bearer | Agent process health |
| `GET` | `/admin/tenants` | Bearer + admin | List tenants |
| `GET` | `/admin/pool` | Bearer + admin | Process pool status |

All authenticated endpoints require `Authorization: Bearer <token>` header.
All error responses use the format `{"error": "<message>"}`.

### Health check

```bash
curl -sf http://localhost:8001/health | jq .
```

```json
{
  "status": "ok",
  "version": "0.1.0",
  "uptime_secs": 12345
}
```

### List agents

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" http://localhost:8001/agents | jq .
```

```json
{
  "agents": [
    {"name": "claude", "mode": "acp", "description": "Claude Code agent (via ACP adapter)"},
    {"name": "kiro", "mode": "acp", "description": "AWS Kiro coding agent"},
    {"name": "opencode", "mode": "acp", "description": "OpenCode multi-model agent"}
  ]
}
```

### Submit a job

```bash
curl -sf -X POST http://localhost:8001/jobs \
  -H "Authorization: Bearer $AGW_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "agent": "claude",
    "prompt": "Analyze the auth module and suggest improvements",
    "progress_notify": true,
    "callback": {
      "channel": "telegram",
      "target": "tg:1704924315",
      "account_id": "default"
    }
  }' | jq .
```

Response (`202 Accepted`):

```json
{
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "pending",
  "agent": "claude",
  "session_id": "def-456"
}
```

#### Request fields

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `agent` | yes | — | Agent name: `claude`, `opencode`, `kiro` |
| `prompt` | yes | — | Task description |
| `callback` | yes* | — | Webhook routing (*without it, results are lost) |
| `callback.channel` | no | `""` | Message platform identifier (e.g., `telegram`, `slack`) |
| `callback.target` | no | `""` | Routing destination (e.g., `tg:1704924315`, `#ops-alerts`) |
| `callback.account_id` | no | `""` | Bot account identifier |
| `session_id` | no | auto-generated UUID v4 | Reuse for multi-turn conversations |
| `progress_notify` | no | `true` | `false` for silent mode (only final result delivered) |

### Job lifecycle

```
pending → running → completed / failed / interrupted
```

| Status | Description |
|--------|-------------|
| `pending` | Job created, waiting for agent process |
| `running` | Agent is actively processing |
| `completed` | Agent finished successfully |
| `failed` | Agent returned an error |
| `interrupted` | Job was cancelled or timed out (`stuck_timeout_secs`) |

### Get job status

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" \
  http://localhost:8001/jobs/550e8400-e29b-41d4-a716-446655440000 | jq .
```

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "agent": "claude",
  "session_id": "sess-abc-123",
  "status": "completed",
  "result": "Refactored auth module: extracted trait, added 12 unit tests...",
  "error": "",
  "tools": ["read_file", "write_file", "run_command"],
  "created_at": 1711234567,
  "completed_at": 1711234600,
  "duration_secs": 33.0
}
```

### List jobs

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" http://localhost:8001/jobs | jq '.jobs[] | {id, status}'
```

Returns at most 100 jobs, ordered by `created_at` descending. Cross-tenant isolation is enforced — you only see your own jobs.

### Close a session

```bash
curl -sf -X DELETE -H "Authorization: Bearer $AGW_TOKEN" \
  http://localhost:8001/sessions/kiro/sess-abc-123 | jq .
```

```json
{"status": "closed", "agent": "kiro", "session_id": "sess-abc-123"}
```

### Admin endpoints

Requires `operations.admin: true` in tenant policy.

```bash
# List tenants
curl -sf -H "Authorization: Bearer $AGW_TOKEN" http://localhost:8001/admin/tenants | jq .

# Process pool status
curl -sf -H "Authorization: Bearer $AGW_TOKEN" http://localhost:8001/admin/pool | jq .
```

### Error reference

| HTTP Status | Error Type | Trigger |
|-------------|-----------|---------|
| `401` | Unauthorized | Missing or invalid Bearer token |
| `403` | Forbidden | Agent not in allow list, operation not permitted, callback denied |
| `404` | Not Found | Agent disabled/missing, job not found or cross-tenant |
| `422` | Unprocessable Entity | Prompt exceeds `max_prompt_length` |
| `429` | Too Many Requests | Quota exceeded, rate limited, process pool exhausted |
| `500` | Internal Server Error | Agent crash, I/O error, database error |
| `504` | Gateway Timeout | Request exceeded `request_timeout_secs` |

### Final result (via webhook callback)

The gateway formats a human-readable message and delivers it via callback:

```
[Claude] abc12345

🔧 bash ×3 | read_file ×1

Analysis results here...

⏱ 27s
📊 input: 156 | output: 420 | cache read: 4,349 | cache write: 3,711 | total: 8,636 tokens
💰 $0.0255
```

## OpenClaw Integration

This setup has been tested end-to-end: Telegram → OpenClaw → agw → agent → webhook → OpenClaw → Telegram.

### Step 1: gateway.yaml for OpenClaw

This is the verified working configuration:

```yaml
# Agent Gateway — local configuration
# All agents inherit system environment (no API keys needed)

server:
  host: "127.0.0.1"
  port: 8001
  shutdown_timeout_secs: 30
  request_timeout_secs: 600

agents:
  kiro:
    enabled: true
    mode: acp
    command: "kiro-cli"
    acp_args: ["acp", "-a"]
    working_dir: "/home/ec2-user/work"
    description: "AWS Kiro coding agent"
    env: {}

  claude:
    enabled: true
    mode: acp
    command: "npx"
    acp_args: ["-y", "@zed-industries/claude-agent-acp"]
    working_dir: "/home/ec2-user/work"
    description: "Claude Code agent (via ACP adapter)"
    env: {}

  opencode:
    enabled: true
    mode: acp
    command: "opencode"
    acp_args: ["acp"]
    working_dir: "/home/ec2-user/work"
    description: "OpenCode multi-model agent"
    env: {}

pool:
  max_processes: 20
  max_per_agent: 10
  idle_timeout_secs: 120
  watchdog_interval_secs: 30
  stuck_timeout_secs: 900

store:
  path: "/home/ec2-user/work/aws/acp-gateway/data/gateway.db"
  job_retention_secs: 86400

callback:
  default_url: "http://127.0.0.1:18789/tools/invoke"
  default_token: "ssa123456"
  retry_max: 3
  retry_base_delay_secs: 5

observability:
  log_level: "info"
  log_format: "json"
  metrics_enabled: false
  audit_path: ""

gateway:
  allowed_ips: []
  rate_limit:
    requests_per_minute: 300
    burst: 10

tenants:
  openclaw:
    credentials:
      - token: "agw-local-token-2024"
    policy:
      agents:
        allow: ["*"]
        deny: []
      operations:
        sync_call: true
        stream: true
        async_jobs: true
        session_manage: true
        admin: true
      resources:
        workspace: "/home/ec2-user/work"
        env_inject: {}
        env_deny: []
      quotas:
        max_concurrent_sessions: 10
        max_concurrent_jobs: 5
        max_prompt_length: 65536
        session_ttl_hours: 48
      callbacks:
        allowed_urls:
          - "http://127.0.0.1:18789/tools/invoke"
        allowed_channels:
          - channel: "*"
            targets: ["*"]
```

### Step 2: Install skill to OpenClaw

```bash
# Option A: npx skills
npx skills add <repo-url> -a openclaw -g

# Option B: Manual
cp -r skill/ ~/clawd/skills/agent-gateway/
chmod +x ~/clawd/skills/agent-gateway/scripts/agw-client.sh
```

### Step 3: Start agw service

```bash
# As systemd service
sudo cp agw.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now agw

# Or run directly
./target/release/agw serve --config gateway.yaml
```

### Step 4: Restart OpenClaw gateway

```bash
openclaw gateway restart
```

### Step 5: Test from Telegram/Feishu/Discord

Send a message to your OpenClaw bot:

```
用 claude 帮我分析一下当前项目的代码结构
```

The bot will:
1. Submit a job to agw
2. Immediately reply "✅ Task submitted"
3. Send progress updates as the agent works
4. Send the final result with token usage stats

### systemd service file

The verified working systemd unit file:

```ini
[Unit]
Description=Agent Gateway (agw)
After=network.target

[Service]
Type=simple
User=ec2-user
Group=ec2-user
WorkingDirectory=/home/ec2-user/work/aws/acp-gateway
ExecStart=/home/ec2-user/work/aws/acp-gateway/target/release/agw serve --config /home/ec2-user/work/aws/acp-gateway/gateway.yaml
Restart=on-failure
RestartSec=5
Environment="PATH=/home/ec2-user/.cargo/bin:/home/ec2-user/.local/bin:/home/ec2-user/.opencode/bin:/home/ec2-user/.npm-global/bin:/home/ec2-user/.bun/bin:/usr/local/bin:/usr/bin:/bin"
Environment="HOME=/home/ec2-user"

[Install]
WantedBy=multi-user.target
```

### Webhook callback format

agw sends this payload to OpenClaw's `/tools/invoke`:

```json
{
  "tool": "message",
  "args": {
    "action": "send",
    "channel": "telegram",
    "target": "tg:1704924315",
    "message": "[Claude] abc12345\n\n🔧 bash ×2\n\nResult text...\n\n⏱ 15s\n📊 input: 3 | output: 5 | total: 8,068 tokens\n💰 $0.0255"
  },
  "sessionKey": "main"
}
```

## Session Management

- **Multi-turn**: Provide the same `session_id` across jobs to maintain conversation context
- **Isolation**: Different session IDs → isolated agent processes
- **Auto-rebuild**: If an agent crashes, gateway rebuilds (context lost, user notified)
- **Pool reuse**: Same `(agent, session_id)` reuses the same subprocess — no cold start penalty on follow-up messages

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `401 unauthorized` | Bad token | Check `tenants.*.credentials` in gateway.yaml |
| `403 agent not allowed` | Agent not in allow list | Add to `policy.agents.allow` |
| `403 admin required` | No admin permission | Set `operations.admin: true` |
| `403 callback URL denied` | Callback URL not in allowlist | Add URL to `callbacks.allowed_urls` |
| `403 callback channel denied` | Channel not permitted | Add to `callbacks.allowed_channels` |
| `404 agent not found` | Agent disabled or missing | Set `agents.*.enabled: true` |
| `422 prompt too long` | Prompt exceeds max length | Increase `quotas.max_prompt_length` or shorten prompt |
| `429 quota exceeded` | Hit concurrent limit | Increase `quotas.max_concurrent_*` or wait for jobs to finish |
| `429 rate limited` | Too many requests | Increase `gateway.rate_limit.requests_per_minute` |
| `503 pool exhausted` | No capacity for agent | Wait or increase `pool.max_per_agent` |
| `504 timeout` | Request took too long | Increase `server.request_timeout_secs` |
| Health check fails | Gateway not running | Check `systemctl status agw` or verify port 8001 is in use |
| Job stuck in `running` | Agent hung | Auto-fails after `stuck_timeout_secs` (default 900s) |
| No callback received | Missing callback field | Always include `callback` in the job request |
| No callback received | OpenClaw not listening | Verify OpenClaw is running on port 18789 |
| Kiro takes ~19s to start | MCP server initialization | Normal — first job on a new Kiro session is slow |
| Agent process dies | Crash or OOM | Gateway auto-rebuilds on next request (context lost) |

## Security

- **Token isolation**: Each tenant authenticates with its own Bearer token(s). Tokens are arbitrary strings — generate with `openssl rand -hex 32`.
- **Environment clearing**: `env: {}` in agent config means agents inherit NO environment variables from the host (agw does `env_clear()`). Only explicitly listed vars are passed.
- **Workspace isolation**: Each tenant gets an isolated workspace directory via `resources.workspace`.
- **Callback restrictions**: Callbacks only deliver to URLs in the tenant's `allowed_urls` list. Channel/target filtering is also enforced.
- **Environment deny-list**: `env_deny` blocks specific variables (e.g., `AWS_SECRET_ACCESS_KEY`) from reaching agent processes.
- **IP allowlist**: `gateway.allowed_ips` restricts access by source IP (CIDR format).
- **Rate limiting**: Global rate limit prevents abuse (`requests_per_minute` + `burst`).
- **Audit log**: When `observability.audit_path` is set, every auth decision is recorded.
- **No credential exposure**: `GET /admin/tenants` lists tenant names only — never credentials or policies.

## Project Structure

```
src/
├── main.rs           # CLI entry (clap) — serve subcommand
├── config.rs         # gateway.yaml parsing + ${VAR} expansion + validation
├── error.rs          # Error types → HTTP status codes
├── app.rs            # AppState assembly (Arc-shared across handlers)
├── lib.rs            # Module re-exports
├── auth/             # Multi-tenant auth + 5-dimension policy enforcement
├── api/              # HTTP handlers (axum) + auth middleware + router
├── runtime/          # ACP/PTY process management + process pool + JSON-RPC
├── scheduler/        # Job lifecycle + SQLite store + patrol loop (watchdog)
├── dispatch/         # Webhook delivery + result formatting + retry logic
└── observability/    # Tracing setup + metrics initialization

skill/                # AI agent skill (installable to OpenClaw/Claude/OpenCode/Kiro)
├── SKILL.md          # Skill instructions for AI agents
├── scripts/          # agw-client.sh helper script
└── references/       # API + configuration reference docs

data/
└── gateway.db        # SQLite database (auto-created)
```

## Config loading flow

```
gateway.yaml
  → read raw text
  → expand ${VAR} references from environment
  → YAML deserialize into GatewayConfig
  → validate_config() checks invariants
  → CLI --host/--port/--verbose overrides applied
  → TenantRegistry built (token → tenant index)
  → ProcessPool, QuotaTracker, JobStore initialized
  → AppState assembled (Arc-shared across handlers)
  → Patrol loop spawned (watchdog + job reaper)
  → Axum server starts with graceful shutdown
```

## License

MIT
