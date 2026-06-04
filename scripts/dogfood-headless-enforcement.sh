#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DATE="$(date -u +%Y-%m-%d)"
OUT="${1:-evidence/dogfood/${DATE}-headless-enforcement}"
PORT="${CURB_DOGFOOD_PORT:-18768}"
ADDR="127.0.0.1:${PORT}"
MARKER="curb-dogfood-enforcement-$$-$(date -u +%s)"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/curb-dogfood-enforcement.XXXXXX")"
HOME_DIR="${SCRATCH}/home"
STATE_DIR="${SCRATCH}/state"
CONFIG="${OUT}/config.yaml"
LEDGER="${OUT}/ledger.ndjson"
LOG="${OUT}/headless-enforcement.ndjson"
STDOUT_LOG="${OUT}/server-stdout.txt"

SERVER_PID=""
WORKER_PID=""

cleanup() {
  if [[ -n "${SERVER_PID}" ]] && kill -0 "${SERVER_PID}" 2>/dev/null; then
    kill "${SERVER_PID}" 2>/dev/null || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
  if [[ -n "${WORKER_PID}" ]] && kill -0 "${WORKER_PID}" 2>/dev/null; then
    kill "${WORKER_PID}" 2>/dev/null || true
    sleep 0.1
    kill -KILL "${WORKER_PID}" 2>/dev/null || true
    wait "${WORKER_PID}" 2>/dev/null || true
  fi
  rm -rf "${SCRATCH}"
}
trap cleanup EXIT

require() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 1
  }
}

require curl
require jq
require python3

mkdir -p "${OUT}" "${HOME_DIR}/.codex/archived_sessions"
rm -f \
  "${LEDGER}" \
  "${LOG}" \
  "${STDOUT_LOG}" \
  "${OUT}/"*.json \
  "${OUT}/"*.txt

cargo build --release --bin curb >"${OUT}/build-release.txt" 2>&1

sh -c "while :; do sleep 1; done # ${MARKER}" &
WORKER_PID="$!"

python3 - "$CONFIG" "$LEDGER" "$STATE_DIR" "$MARKER" "$HOME_DIR" "$ROOT" <<'PY'
import json
import pathlib
import sys
from datetime import datetime, timezone

config_path, ledger_path, state_dir, marker, home_dir, repo_root = sys.argv[1:]
config = f"""version: 1
profile: dogfood-headless-enforcement
mode: enforcement

service:
  scan_interval: 1s
  policy_interval: 1s
  heartbeat_interval: 60s
  min_confidence: 50
  state_dir: {state_dir}

usage:
  enabled: true
  scan_interval: 1s
  lookback: 5m
  window: 5m
  warn_turn_tokens: 100
  kill_turn_tokens: 200
  grace_period: 1s
  escalate_supervised: false

defaults:
  warn_after: 30s
  kill_after: 60s
  ack_extension: 30s
  max_extensions: 1
  kill_grace_period: 1s
  cooldown_after_kill: 15m
  min_lifetime: 0s
  max_run_gap: 20s
  allow_app_root_kill: false

agents:
  - id: dogfood-synthetic-worker
    label: Dogfood Synthetic Worker
    family: codex
    kind: process
    match:
      process_names:
        - sh
        - bash
        - dash
      require_command_regex:
        - "{marker}"
      command_regex:
        - "{marker}"

alerts:
  local_notifications: false

ledger:
  path: {ledger_path}
  include_prompt_content: false
"""
pathlib.Path(config_path).write_text(config)

at = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
rows = [
    {"timestamp": at, "type": "session_meta", "payload": {"id": "dogfood-enforcement", "cwd": repo_root}},
    {
        "timestamp": at,
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "last_token_usage": {
                    "input_tokens": 320,
                    "cached_input_tokens": 0,
                    "output_tokens": 20,
                    "reasoning_output_tokens": 0,
                    "total_tokens": 340,
                },
                "total_token_usage": {"total_tokens": 340},
                "model_context_window": 258400,
            },
        },
    },
]
log_path = pathlib.Path(home_dir) / ".codex" / "archived_sessions" / "dogfood-enforcement.jsonl"
log_path.write_text("\n".join(json.dumps(row, separators=(",", ":")) for row in rows) + "\n")
PY

./target/release/curb validate-config "${CONFIG}" >"${OUT}/validate-config.txt"

CURB_LOG_FORMAT=json ./target/release/curb serve \
  --headless \
  --addr "${ADDR}" \
  --config "${CONFIG}" \
  --home "${HOME_DIR}" \
  >"${STDOUT_LOG}" \
  2>"${LOG}" &
SERVER_PID="$!"

for _ in {1..50}; do
  if curl -fsS "http://${ADDR}/v1/live" >"${OUT}/live.json" 2>/dev/null; then
    break
  fi
  sleep 0.1
done
curl -fsS "http://${ADDR}/v1/ready" >"${OUT}/ready.json"

TOKEN="$(cat "${STATE_DIR}/api.token")"
AUTH_HEADER="Authorization: Bearer ${TOKEN}"

curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/health" >"${OUT}/health-authenticated.json"
curl -fsS -X POST -H "${AUTH_HEADER}" "http://${ADDR}/v1/service/rescan" >"${OUT}/rescan.json"

for _ in {1..50}; do
  curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/sessions" >"${OUT}/sessions.json"
  if jq -e '.[] | select(.key == "codex:dogfood-enforcement" and .can_stop == true and .pid != null)' "${OUT}/sessions.json" >"${OUT}/selected-session.json"; then
    break
  fi
  sleep 0.1
done

jq -e '.key == "codex:dogfood-enforcement" and .alert == "kill" and .can_stop == true and .pid != null and .process_started_at != null and (.owner | length > 0)' \
  "${OUT}/selected-session.json" >"${OUT}/selected-session-check.txt"

jq '{
  confirm: true,
  scope: "tree",
  reason: "curb dogfood synthetic enforcement",
  expected: {
    pid: .pid,
    started_at: .process_started_at,
    owner: .owner,
    executable: .executable,
    bundle_id: .bundle_id,
    team_id: .team_id
  }
}' "${OUT}/selected-session.json" >"${OUT}/stop-request.json"

ENCODED_KEY="$(python3 - <<'PY'
import urllib.parse
print(urllib.parse.quote("codex:dogfood-enforcement", safe=""))
PY
)"

curl -i -X POST \
  -H "${AUTH_HEADER}" \
  -H "Content-Type: application/json" \
  --data @"${OUT}/stop-request.json" \
  "http://${ADDR}/v1/sessions/${ENCODED_KEY}/stop" \
  >"${OUT}/stop-response.txt"

grep -q "HTTP/1.1 200 OK" "${OUT}/stop-response.txt"
wait "${WORKER_PID}" 2>/dev/null || true
printf 'worker_pid=%s\nworker_reaped=true\n' "${WORKER_PID}" >"${OUT}/worker-exit.txt"
WORKER_PID=""

curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/events?limit=20" >"${OUT}/events-after-stop.json"
jq -s -e 'map(.type) | index("manual_stop_started") and index("manual_stop_completed")' \
  "${LEDGER}" >"${OUT}/ledger-event-check.txt"

kill "${SERVER_PID}" 2>/dev/null || true
wait "${SERVER_PID}" 2>/dev/null || true
SERVER_PID=""

python3 scripts/parse-observability-smoke.py "${LOG}" >"${OUT}/parse-observability-smoke.txt"
jq -e 'select(.event == "stop_decision" and .fields.status == 200)' "${LOG}" >"${OUT}/stop-decision-log.json"

if rg -n "${TOKEN}|${MARKER}|Authorization|Bearer|curb dogfood synthetic enforcement|prompt|response|screenshot|keystroke" "${LOG}" >"${OUT}/redaction-check.txt"; then
  echo "unexpected sensitive material in ${LOG}" >&2
  exit 1
fi
printf 'ok: no token, marker, reason, auth header, prompt, response, screenshot, or keystroke terms in NDJSON\n' >"${OUT}/redaction-check.txt"

cat >"${OUT}/README.md" <<EOF
# Headless Enforcement Dogfood

Date: ${DATE}

Purpose: prove that Curb can run as a headless sidecar and successfully stop a
harmless, repo-spawned synthetic worker through the protected API while
preserving the termination safety boundary.

Commands:

\`\`\`sh
bash scripts/dogfood-headless-enforcement.sh ${OUT}
\`\`\`

Evidence:

- \`config.yaml\`: enforcement config with one marker-gated synthetic process
  agent, private scratch state, and local notification delivery disabled.
- \`build-release.txt\`: release binary build.
- \`live.json\`, \`ready.json\`, \`health-authenticated.json\`: headless API
  probes.
- \`selected-session.json\`: selected stop candidate, including PID,
  start-time, owner, executable identity, alert state, and \`can_stop: true\`.
- \`stop-request.json\`: stale-state stop request sent back to the service.
- \`stop-response.txt\`: \`HTTP/1.1 200 OK\` stop response.
- \`worker-exit.txt\`: direct child was reaped after Curb stopped it.
- \`events-after-stop.json\`: protected ledger API response after the stop.
- \`ledger-event-check.txt\`: manual stop started/completed events were present.
- \`headless-enforcement.ndjson\`: structured JSON logs.
- \`parse-observability-smoke.txt\`: parser accepted the NDJSON artifact and
  required runtime policy fields on \`usage_scan\` and \`watcher_tick\`.
- \`stop-decision-log.json\`: structured \`stop_decision\` status 200 log.
- \`redaction-check.txt\`: token, marker, operator reason, auth header, prompt,
  response, screenshot, and keystroke terms were absent from NDJSON.

Safety notes:

- The only configured agent matcher requires the unique marker
  \`${MARKER}\`, so real agent processes cannot match this config.
- The usage log is synthetic metadata only, under a temporary HOME.
- State and token files are under a private temporary directory deleted on exit.
- Termination still uses the production stop path: fresh process capture,
  expected PID/start-time/owner/executable validation, sealed
  \`TerminationTarget\`, then platform process-tree termination.

Residual risk:

- This is local macOS evidence. Hosted CI proof and Windows-specific behavior
  remain separate gates.
EOF

echo "wrote ${OUT}"
