#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DATE="$(date -u +%Y-%m-%d)"
OUT="${1:-evidence/dogfood/${DATE}-live-dashboard-qa}"
PORT="${CURB_LIVE_QA_PORT:-$(python3 - <<'PY'
import socket

with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)}"
ADDR="127.0.0.1:${PORT}"
URL="http://${ADDR}/"
SCRATCH_RAW="$(mktemp -d "${TMPDIR:-/tmp}/curb-live-dashboard-qa.XXXXXX")"
SCRATCH="$(cd "${SCRATCH_RAW}" && pwd -P)"
HOME_DIR="${SCRATCH}/home"
STATE_DIR="${SCRATCH}/state"
WORK_ROOT="${SCRATCH}/work"
CONFIG="${OUT}/config.yaml"
LEDGER="${OUT}/ledger.ndjson"
LOG="${OUT}/server.ndjson"
STDOUT_LOG="${OUT}/server-stdout.txt"
WORKER_EXE="${CURB_LIVE_QA_WORKER:-python3}"
MARKER="curb-live-dashboard-qa-$$-$(date -u +%s)"

SERVER_PID=""

cleanup() {
  if [[ -n "${SERVER_PID}" ]] && kill -0 "${SERVER_PID}" 2>/dev/null; then
    kill "${SERVER_PID}" 2>/dev/null || true
    wait "${SERVER_PID}" 2>/dev/null || true
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
require node
require python3
require rg

mkdir -p "${OUT}" "${HOME_DIR}/.codex/archived_sessions" "${STATE_DIR}" "${WORK_ROOT}/ack" "${WORK_ROOT}/stop"
rm -f \
  "${LOG}" \
  "${STDOUT_LOG}" \
  "${LEDGER}" \
  "${OUT}/"*.json \
  "${OUT}/"*.txt \
  "${OUT}/"*.png

cargo build --release --bin curb >"${OUT}/build-release.txt" 2>&1

python3 - "$CONFIG" "$STATE_DIR" "$LEDGER" "$WORK_ROOT" "$WORKER_EXE" "$HOME_DIR" "$MARKER" <<'PY'
from datetime import datetime, timezone
from pathlib import Path
import json
import sys

config_path, state_dir, ledger, work_root, worker_exe, home_dir, marker = sys.argv[1:]
config = f"""version: 1
profile: live-dashboard-qa
mode: enforcement
service:
  scan_interval: 1s
  policy_interval: 1s
  heartbeat_interval: 5s
  min_confidence: 50
  state_dir: {state_dir}
usage:
  enabled: true
  scan_interval: 1s
  lookback: 1h
  window: 1m
  warn_turn_tokens: 100
  kill_turn_tokens: 200
  grace_period: 1s
defaults:
  warn_after: 1h
  kill_after: 2h
  ack_extension: 10s
  max_extensions: 1
  kill_grace_period: 1s
  cooldown_after_kill: 1s
  min_lifetime: 0s
  max_run_gap: 1s
  allow_app_root_kill: false
agents:
  - id: codex-live-qa-worker
    label: Codex Live QA Worker
    family: codex
    kind: process
    match:
      process_names:
        - python3
        - python
      require_command_regex:
        - "{marker}"
      command_regex:
        - "{marker}"
alerts:
  local_notifications: false
ledger:
  path: {ledger}
  include_prompt_content: false
"""
Path(config_path).write_text(config)

at = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
rows = [
    {"timestamp": at, "type": "session_meta", "payload": {"id": "live-ack", "cwd": f"{work_root}/ack"}},
    {
        "timestamp": at,
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "last_token_usage": {
                    "input_tokens": 220,
                    "cached_input_tokens": 0,
                    "output_tokens": 20,
                    "reasoning_output_tokens": 0,
                    "total_tokens": 240,
                },
                "total_token_usage": {"total_tokens": 240},
                "model_context_window": 258400,
            },
        },
    },
]
log_path = Path(home_dir) / ".codex" / "archived_sessions" / "live-ack.jsonl"
log_path.write_text("\n".join(json.dumps(row, separators=(",", ":")) for row in rows) + "\n")
PY

./target/release/curb validate-config "${CONFIG}" >"${OUT}/validate-config.txt"

CURB_LOG_FORMAT=json ./target/release/curb serve \
  --addr "${ADDR}" \
  --config "${CONFIG}" \
  --home "${HOME_DIR}" \
  >"${STDOUT_LOG}" \
  2>"${LOG}" &
SERVER_PID="$!"

for _ in {1..100}; do
  if curl -fsS "${URL}v1/live" >"${OUT}/live.json" 2>/dev/null; then
    break
  fi
  sleep 0.1
done

curl -fsS "${URL}v1/live" >"${OUT}/live.json"
curl -sS "${URL}v1/ready" >"${OUT}/ready-initial.json"
TOKEN="$(cat "${STATE_DIR}/api.token")"
curl -fsS -H "Authorization: Bearer ${TOKEN}" "${URL}v1/health" >"${OUT}/health-authenticated.json"

CURB_LIVE_QA_URL="${URL}" \
CURB_LIVE_QA_OUT="${OUT}" \
CURB_LIVE_QA_HOME="${HOME_DIR}" \
CURB_LIVE_QA_WORK_ROOT="${WORK_ROOT}" \
CURB_LIVE_QA_WORKER="${WORKER_EXE}" \
CURB_LIVE_QA_MARKER="${MARKER}" \
CURB_LIVE_QA_CONFIG="${CONFIG}" \
node ui/scripts/live-dashboard-qa.mjs

kill "${SERVER_PID}" 2>/dev/null || true
wait "${SERVER_PID}" 2>/dev/null || true
SERVER_PID=""

python3 scripts/parse-observability-smoke.py "${LOG}" >"${OUT}/parse-observability-smoke.txt"

REDACTION_HITS="${OUT}/redaction-check.txt"
: >"${REDACTION_HITS}"
scan_fixed_redaction() {
  local label="$1"
  local value="$2"
  local status
  if [[ -z "${value}" ]]; then
    return
  fi
  if rg -F -n "${value}" "${LOG}" >>"${REDACTION_HITS}"; then
    echo "unexpected ${label} in ${LOG}" >&2
    exit 1
  else
    status="$?"
    if [[ "${status}" -gt 1 ]]; then
      echo "redaction scan failed for ${label} in ${LOG}" >&2
      exit "${status}"
    fi
  fi
}

scan_regex_redaction() {
  local label="$1"
  local pattern="$2"
  local status
  if rg -i -n "${pattern}" "${LOG}" >>"${REDACTION_HITS}"; then
    echo "unexpected ${label} in ${LOG}" >&2
    exit 1
  else
    status="$?"
    if [[ "${status}" -gt 1 ]]; then
      echo "redaction scan failed for ${label} in ${LOG}" >&2
      exit "${status}"
    fi
  fi
}

scan_fixed_redaction "token" "${TOKEN}"
scan_fixed_redaction "marker" "${MARKER}"
scan_regex_redaction \
  "sensitive term" \
  "Authorization|Bearer|prompt|response|screenshot|keystroke|file[-_ ]?content|file contents|raw provider|raw_provider|provider payload|payload"
printf 'ok: no token, marker, auth header, prompt, response, screenshot, keystroke, file-content, raw-provider, or payload terms in NDJSON\n' >"${REDACTION_HITS}"

cat >"${OUT}/README.md" <<EOF
# Live Dashboard QA

Date: ${DATE}

Purpose: drive the browser dashboard against a real \`curb serve\` endpoint
using scratch state, synthetic metadata-only Codex usage, and a harmless
marker-gated \`${WORKER_EXE}\` subprocess.

Command:

\`\`\`sh
bash scripts/qa-live-dashboard.sh ${OUT}
\`\`\`

Evidence:

- \`manifest.json\`: browser action log, viewports, screenshots, console errors,
  and failure list.
- \`dashboard-desktop.png\` and \`dashboard-narrow.png\`: live served UI captures.
- \`safe-stop-rejection.json\`: browser-origin protected API request showing
  stale stop identity is rejected.
- \`worker-exit.json\`: confirms the synthetic worker was stopped after
  browser confirmation.
- \`live.json\`, \`ready-initial.json\`, and \`health-authenticated.json\`:
  live service probes.
- \`server.ndjson\`, \`parse-observability-smoke.txt\`, and
  \`redaction-check.txt\`: runtime observability and redaction proof.

Safety:

- Only synthetic Codex token metadata is written under a temporary HOME.
- The destructive stop path targets only a scratch marker-gated
  \`${WORKER_EXE}\` process with PID/start-time/owner/executable evidence.
- The script is advisory. It is not wired into \`scripts/check-fast.sh\` or
  \`scripts/validate.sh\` until repeated runs prove it is deterministic enough.
EOF

echo "live dashboard QA ok: ${OUT}"
