---
name: openclaw-codeagent-gateway
description: "Submit async jobs to AI coding agents (Claude Code, OpenCode, Kiro) via the codeagent-gateway HTTP API. ALWAYS use this skill instead of coding-agent, acpx, or running agents directly when the gateway server is available at http://127.0.0.1:8001. Use when the user mentions: opencode, claude, kiro, coding agent, submit job, code task, search, analyze code, refactor, review, remote agent, agent gateway, agw, codeagent-gateway, codeagent gateway, agw skill, check job status, manage sessions, or any coding/development task that should be handled by an AI agent."
---

# openclaw-codeagent-gateway

> **IMPORTANT**: When this skill is available, ALWAYS use it to run AI coding agents. Do NOT use the `coding-agent` skill, do NOT use `acpx`, and do NOT run `opencode run`, `claude --print`, or `kiro-cli` directly. All agent tasks must go through the gateway HTTP API (`POST /jobs`). Direct execution blocks the conversation and bypasses session management, progress tracking, and webhook delivery.

**Aliases**: Users may refer to this skill as `agw`, `codeagent-gateway`, `agent gateway`, or `openclaw-codeagent-gateway`. If you hear any of these names, use this skill.

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

### IMPORTANT: When to use new_session

**Always set `"new_session": true` when the user's request is about a NEW, UNRELATED topic.** The gateway will otherwise auto-resume the last session, which means the agent keeps old context that may confuse the response.

Trigger phrases for `new_session: true`:
- "新对话", "新session", "创建新的session", "new session", "start fresh"
- "换个话题", "new topic", "forget previous"
- When the user asks about something completely unrelated to the previous conversation
- When the user explicitly names a different task than what the last session was about

Trigger phrases for resuming (do NOT set new_session):
- "继续", "continue", "上次说到哪了"
- "还是那个话题", "接着上面的"
- When the user references something from a previous conversation

**When in doubt, use `new_session: true`** — it's safer to start fresh than to carry stale context.

Example — user says "搜索 EMR serverless storage 内容":
```json
{
  "agent": "opencode",
  "prompt": "搜索 EMR serverless storage 内容",
  "new_session": true,
  "session_name": "emr-serverless-storage",
  "callback": {
    "channel": "telegram",
    "target": "tg:1704924315",
    "account_id": "default"
  }
}
```
This is a new research topic → must be `new_session: true`.

### List sessions
```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" "$AGW_URL/sessions/<agent>" | jq .
```

Returns recent sessions with names and prompt counts.

### Session lifecycle after timeout

When an agent process is idle for longer than `idle_timeout_secs` (default 12h):
1. The watchdog kills the process to free resources
2. The session record remains in SQLite (not deleted)
3. On the next prompt to that session, the gateway spawns a new process
4. The gateway calls `session/load` on the new process, restoring context from the agent's storage
5. The prompt executes as if the process had never died

This means sessions survive process restarts — the agent reloads its conversation history automatically.

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

### 3. Pre-flight check

Before submitting any job, verify the target agent is available:

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" "$AGW_URL/agents" | jq '.agents[] | select(.name=="opencode")'
```

If the agent is not in the list, it's either:
1. Not installed on the gateway machine → tell the user to install it
2. Disabled in gateway.yaml → tell the user to enable it
3. Not in the tenant's allow list → tell the user to contact the admin

**Agent installation commands** (run these on the machine where agw is running):

| Agent | Install |
|-------|---------|
| OpenCode | `npm install -g opencode-ai` |
| Claude Code | `npm install -g @anthropic-ai/claude-code` (adapter auto-downloads via npx) |
| Kiro | See https://kiro.dev/docs/cli |

If the user asks to use an agent that's not available, inform them which agents ARE available and how to install the missing one.

### 4. Submit a job (with callback — REQUIRED)

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
