---
name: agent-gateway
description: "Interact with a running Agent Gateway (agw) — submit async jobs to AI coding agents (Claude, OpenCode, Kiro) via HTTP with fire-and-forget pattern and webhook callbacks. Use when the user mentions: agent gateway, agw, submit job, remote agent, agent-to-agent, check job status, gateway health, manage sessions, list agents, or webhook callback."
---

# Agent Gateway Skill

Submit async jobs to AI coding agents (Claude, OpenCode, Kiro) via an HTTP gateway with automatic webhook delivery.

## CRITICAL: Fire-and-forget

After executing the curl command to submit a job:
1. Do NOT output any text — no confirmation, no explanation, nothing
2. The gateway automatically sends confirmation + progress + result to the chat
3. Just end your response silently after the curl command

If you output text after submitting, the user sees duplicate messages.

**CORRECT:**
```
POST /jobs → get job_id → END TURN (no reply text)
```

**WRONG (redundant reply):**
```
POST /jobs → reply "» Task submitted..." → ...
```

**WRONG (blocks the session):**
```
POST /jobs → sleep → GET /jobs/{id} → sleep → GET /jobs/{id} → ...
```

Only use `GET /jobs/{id}` when the user explicitly asks "what's the status of job X?"

## Environment (pre-configured)

```bash
AGW_URL=http://127.0.0.1:8001
AGW_TOKEN=agw-local-token-2024
```

## Session Management

Sessions maintain conversation context across multiple prompts to the same agent.

### Auto-resume (default)
When no `session_name` is specified, the gateway automatically resumes the most recent session for the same agent. The agent remembers previous conversation context.

### Named sessions
Add `session_name` to create or resume a named session:
```json
{"agent": "opencode", "prompt": "...", "session_name": "auth-refactor"}
```

When the user mentions a previous topic by name, use that as `session_name`.

### New session
Force a fresh session with no prior context:
```json
{"agent": "opencode", "prompt": "...", "new_session": true}
```

Use when the user says "new conversation", "start fresh", "forget everything", etc.

### List sessions
```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" "$AGW_URL/sessions/<agent>" | jq .
```

Returns recent sessions with names and prompt counts.

### Naming sessions

When submitting a job, generate a `session_name` that describes the TASK CONTENT — what the user wants to accomplish. Use 2-4 English words, hyphenated, lowercase. The gateway will automatically append a short unique suffix.

**Good names** (describe the task):
| User prompt | session_name |
|-------------|-------------|
| "帮我分析 Palantir 的 AIP 平台" | `palantir-aip-analysis` |
| "重构 auth 模块的权限检查" | `auth-refactor` |
| "查看磁盘空间使用情况" | `disk-usage-check` |
| "搜索 EMR Serverless 的最新文档" | `emr-serverless-docs` |
| "写一个 Python 脚本处理 CSV" | `csv-processing-script` |
| "Review this PR for security issues" | `security-pr-review` |

**Bad names** (don't describe the task):
| ❌ Bad | Why |
|--------|-----|
| `start-completely-fresh` | Describes the action, not the content |
| `new-conversation` | Generic, says nothing about the task |
| `help-me` | Too vague |
| `opencode-task` | Just the agent name |
| `session-1` | Sequential number, meaningless |

The name should answer: "If I see this name in a list of sessions, will I know what this conversation was about?"

## Server Installation

If the Agent Gateway server is not running, install it first.

### One-command install (Linux x86_64)

```bash
curl -LO https://github.com/yhyyz/openclaw-codeagent-gateway/releases/download/v0.1.0/agw-linux-x86_64.tar.gz
tar xzf agw-linux-x86_64.tar.gz
chmod +x agw-linux-x86_64
sudo mv agw-linux-x86_64 /usr/local/bin/agw
rm agw-linux-x86_64.tar.gz
```

### Create config

```bash
curl -LO https://raw.githubusercontent.com/yhyyz/openclaw-codeagent-gateway/main/gateway.yaml.example
cp gateway.yaml.example gateway.yaml
```

Edit `gateway.yaml`:
1. Set a tenant token (any string you choose)
2. Enable the agents you have installed (kiro, claude, opencode)
3. If using with OpenClaw on the same machine, set callback:
   ```yaml
   callback:
     default_url: "http://127.0.0.1:18789/tools/invoke"
     default_token: "<your-openclaw-gateway-password>"
   ```

### Start the server

```bash
mkdir -p data/
agw serve --config gateway.yaml
```

### Register as systemd service (recommended)

```bash
cat > /tmp/agw.service << 'EOF'
[Unit]
Description=Agent Gateway (agw)
After=network.target

[Service]
Type=simple
User=$USER
WorkingDirectory=$HOME
ExecStart=/usr/local/bin/agw serve --config $HOME/gateway.yaml
Restart=on-failure
RestartSec=5
Environment="PATH=$HOME/.cargo/bin:$HOME/.npm-global/bin:/usr/local/bin:/usr/bin:/bin"
Environment="HOME=$HOME"

[Install]
WantedBy=multi-user.target
EOF

sudo cp /tmp/agw.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now agw
```

### Verify

```bash
curl -sf http://127.0.0.1:8001/health | jq .
```

> **Note**: The Agent Gateway server currently supports **Linux only** (x86_64). Build from source for other architectures: `cargo build --release` from the [repository](https://github.com/yhyyz/openclaw-codeagent-gateway).

## Quick start

### 1. Health check

```bash
curl -sf "$AGW_URL/health" | jq .
```

### 2. List agents

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" "$AGW_URL/agents" | jq .
```

### 3. Submit a job (with callback — REQUIRED)

```bash
curl -sf -X POST "$AGW_URL/jobs" \
  -H "Authorization: Bearer $AGW_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "agent": "claude",
    "prompt": "Analyze the auth module and suggest improvements",
    "session_name": "auth-analysis",
    "progress_notify": true,
    "callback": {
      "channel": "telegram",
      "target": "tg:1704924315",
      "account_id": "default"
    }
  }' | jq .
```

Response: `202 Accepted` with `job_id`, `status: "pending"`, `session_id`, `session_name`.

### Fields

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `agent` | yes | — | Agent name: `claude`, `opencode`, `kiro` |
| `prompt` | yes | — | Task description |
| `callback` | yes* | — | Webhook routing (*without it, results are lost) |
| `session_name` | no | auto-generated | Human-readable session name for resume |
| `new_session` | no | `false` | Force a fresh session |
| `session_id` | no | auto-generated | Low-level session ID override |
| `progress_notify` | no | `true` | `false` for silent mode (only final result delivered) |

### Callback format

```json
{
  "channel": "telegram",
  "target": "tg:1704924315",
  "account_id": "default"
}
```

The gateway POSTs results to `http://127.0.0.1:18789/tools/invoke` using OpenClaw format:

```json
{
  "tool": "message",
  "args": {
    "action": "send",
    "channel": "telegram",
    "target": "tg:1704924315",
    "message": "..."
  },
  "sessionKey": "main"
}
```

## Auto-notification

- **Progress**: Tool starts, plans, and intermediate updates pushed to chat during execution (when `progress_notify: true`)
- **Final result**: Full result + tools used + token usage + cost pushed automatically on completion
- **Errors**: Failures also delivered via callback with error details

## Check job status (only when user explicitly asks)

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" "$AGW_URL/jobs/<job_id>" | jq .
```

Status values: `pending` → `running` → `completed` / `failed` / `interrupted`

### List all jobs

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" "$AGW_URL/jobs" | jq .
```

## Close a session

```bash
curl -sf -X DELETE -H "Authorization: Bearer $AGW_TOKEN" \
  "$AGW_URL/sessions/<agent>/<session_id>" | jq .
```

## Administration

Requires `operations.admin: true` in tenant policy.

| Endpoint | Description |
|----------|-------------|
| `GET /health` | Gateway health (no auth) |
| `GET /health/agents` | Agent health check |
| `GET /admin/tenants` | List all tenants |
| `GET /admin/pool` | Process pool status |

## Agent notes

| Agent | ACP Command | Token reporting | Notes |
|-------|------------|-----------------|-------|
| claude | `npx -y @zed-industries/claude-agent-acp` | Full (input/output/cache read/cache write) | |
| opencode | `opencode acp` | Total only | |
| kiro | `kiro-cli acp -a` | None | Slow startup (~19s), needs `-a` for auto-accept permissions |

## Using agw-client.sh

```bash
AGW_CLIENT="<skill-dir>/scripts/agw-client.sh"

$AGW_CLIENT -l                              # List agents
$AGW_CLIENT "Refactor the auth module"      # Submit job (default agent)
$AGW_CLIENT -a claude "Analyze code"        # Specify agent
$AGW_CLIENT --job-status <job_id>           # Query job
$AGW_CLIENT -s <uuid> "Continue from here"  # Multi-turn session
```

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `401 unauthorized` | Bad token | Check `tenants.*.credentials` in gateway.yaml |
| `403 agent not allowed` | Agent not in allow list | Add to `policy.agents.allow` |
| `403 admin required` | No admin permission | Set `operations.admin: true` |
| `429 quota exceeded` | Hit concurrent limit | Increase `quotas.max_concurrent_*` |
| `404 agent not found` | Agent disabled or missing | Set `agents.*.enabled: true` |
| `503 pool exhausted` | No capacity for agent | Wait or increase `pool.max_per_agent` |
| Health timeout | Gateway not running | Check `systemctl status agw` |
| Job stuck running | Agent hung | Auto-fails after `stuck_timeout_secs` |
| No callback received | Missing callback field | Always include `callback` in request |

## Security

- Never display tokens in plaintext — use `AGW_TOKEN` env var
- Gateway filters env vars per tenant (`env_deny` blocks secrets)
- Each tenant gets an isolated workspace directory
- Cross-tenant job isolation enforced
