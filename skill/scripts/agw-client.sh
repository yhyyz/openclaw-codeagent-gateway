#!/usr/bin/env bash
# agw-client.sh — CLI client for Agent Gateway (agw)
# Usage: agw-client.sh [OPTIONS] [PROMPT]
#
# Requires: curl, jq, uuidgen

set -euo pipefail

readonly VERSION="0.2.0"
readonly DEFAULT_URL="http://127.0.0.1:8001"
readonly DEFAULT_RETRIES=3
readonly RETRY_BASE_DELAY=2

# ── Configuration (env vars or flags) ────────────────────────────────
AGW_URL="${AGW_URL:-$DEFAULT_URL}"
AGW_TOKEN="${AGW_TOKEN:-}"
AGW_AGENT="${AGW_AGENT:-}"
SESSION_ID=""
JOB_STATUS_ID=""
RETRIES="$DEFAULT_RETRIES"
PROMPT=""

# ── Helpers ──────────────────────────────────────────────────────────
die()  { echo "error: $*" >&2; exit 1; }
info() { echo ":: $*" >&2; }

check_deps() {
  for cmd in curl jq uuidgen; do
    command -v "$cmd" >/dev/null 2>&1 || die "missing dependency: $cmd"
  done
}

auth_header() {
  [[ -n "$AGW_TOKEN" ]] || die "no auth token — set AGW_TOKEN or use -t <token>"
  echo "Authorization: Bearer $AGW_TOKEN"
}

retry() {
  local max=$1; shift
  local attempt=0 delay=$RETRY_BASE_DELAY
  while true; do
    if "$@"; then return 0; fi
    attempt=$((attempt + 1))
    if (( attempt >= max )); then
      die "failed after $max attempts"
    fi
    info "attempt $attempt/$max failed, retrying in ${delay}s..."
    sleep "$delay"
    delay=$((delay * 2))
  done
}

# ── API calls ────────────────────────────────────────────────────────
do_health() {
  curl -sf "$AGW_URL/health" | jq .
}

do_list_agents() {
  curl -sf -H "$(auth_header)" "$AGW_URL/agents" | jq -r \
    '.agents[] | "\(.name)\t\(.mode)\t\(.description)"' | column -t -s $'\t'
}

resolve_agent() {
  if [[ -n "$AGW_AGENT" ]]; then
    echo "$AGW_AGENT"
    return
  fi
  local first
  first=$(curl -sf -H "$(auth_header)" "$AGW_URL/agents" | jq -r '.agents[0].name // empty')
  [[ -n "$first" ]] || die "no agents available"
  info "using agent: $first"
  echo "$first"
}

resolve_session() {
  if [[ -n "$SESSION_ID" ]]; then
    echo "$SESSION_ID"
    return
  fi
  uuidgen
}

do_submit_job() {
  local agent session payload response
  agent=$(resolve_agent)
  session=$(resolve_session)

  payload=$(jq -n \
    --arg agent "$agent" \
    --arg prompt "$PROMPT" \
    --arg session "$session" \
    '{agent: $agent, prompt: $prompt, session_id: $session}')

  info "submitting job: agent=$agent session=$session"

  response=$(retry "$RETRIES" curl -sf -X POST "$AGW_URL/jobs" \
    -H "$(auth_header)" \
    -H "Content-Type: application/json" \
    -d "$payload")

  local job_id
  job_id=$(echo "$response" | jq -r '.job_id // empty')
  if [[ -n "$job_id" ]]; then
    echo "job_id: $job_id"
    echo "status: $(echo "$response" | jq -r '.status')"
  else
    echo "$response" | jq .
  fi
}

do_job_status() {
  [[ -n "$JOB_STATUS_ID" ]] || die "no job_id provided"
  curl -sf -H "$(auth_header)" "$AGW_URL/jobs/$JOB_STATUS_ID" | jq .
}

do_list_jobs() {
  curl -sf -H "$(auth_header)" "$AGW_URL/jobs" | jq .
}

# ── Usage ────────────────────────────────────────────────────────────
usage() {
  cat <<EOF
agw-client.sh v$VERSION — Agent Gateway CLI client

USAGE:
  agw-client.sh [OPTIONS] [PROMPT]

OPTIONS:
  -l, --list              List available agents
  -a, --agent <name>      Specify agent (default: first available)
  -s, --session <uuid>    Session ID for multi-turn conversations
  -t, --token <token>     Auth token (or set AGW_TOKEN)
  -u, --url <url>         Gateway URL (default: $DEFAULT_URL, or set AGW_URL)
      --job-status <id>   Query status of a job
      --list-jobs         List all your jobs
      --health            Check gateway health (no auth needed)
      --retries <n>       Max retries on failure (default: $DEFAULT_RETRIES)
  -h, --help              Show this help
  -V, --version           Show version

ENVIRONMENT:
  AGW_URL     Gateway base URL
  AGW_TOKEN   Bearer auth token
  AGW_AGENT   Default agent name

EXAMPLES:
  agw-client.sh -l
  agw-client.sh "Explain this codebase"
  agw-client.sh -a claude "Refactor the auth module"
  agw-client.sh --job-status 550e8400-e29b-41d4-a716-446655440000
  agw-client.sh -s my-session-id "Continue from where we left off"
EOF
  exit 0
}

# ── Argument parsing ─────────────────────────────────────────────────
ACTION=""

parse_args() {
  while (( $# > 0 )); do
    case "$1" in
      -h|--help)       usage ;;
      -V|--version)    echo "agw-client.sh v$VERSION"; exit 0 ;;
      -l|--list)       ACTION="list"; shift ;;
      -a|--agent)      AGW_AGENT="${2:?missing agent name}"; shift 2 ;;
      -s|--session)    SESSION_ID="${2:?missing session id}"; shift 2 ;;
      -t|--token)      AGW_TOKEN="${2:?missing token}"; shift 2 ;;
      -u|--url)        AGW_URL="${2:?missing url}"; shift 2 ;;
      --job-status)    ACTION="job-status"; JOB_STATUS_ID="${2:?missing job id}"; shift 2 ;;
      --list-jobs)     ACTION="list-jobs"; shift ;;
      --health)        ACTION="health"; shift ;;
      --retries)       RETRIES="${2:?missing retry count}"; shift 2 ;;
      -*)              die "unknown option: $1 (try --help)" ;;
      *)               PROMPT="$1"; shift ;;
    esac
  done
}

# ── Main ─────────────────────────────────────────────────────────────
main() {
  check_deps
  parse_args "$@"

  case "$ACTION" in
    health)     do_health ;;
    list)       do_list_agents ;;
    job-status) do_job_status ;;
    list-jobs)  do_list_jobs ;;
    "")
      [[ -n "$PROMPT" ]] || die "no prompt provided (try --help)"
      do_submit_job
      ;;
  esac
}

main "$@"
