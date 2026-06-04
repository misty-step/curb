#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DATE="$(date -u +%Y-%m-%d)"
OUT="${1:-evidence/dogfood/${DATE}-headless-observability}"
DURATION="${CURB_DOGFOOD_SECONDS:-60}"
if ! [[ "${DURATION}" =~ ^[0-9]+$ ]] || [[ "${DURATION}" -lt 1 ]]; then
  echo "CURB_DOGFOOD_SECONDS must be a positive integer, got ${DURATION}" >&2
  exit 2
fi
EXPECTED_WATCHER_TICKS="$(( DURATION / 5 ))"
if [[ "${EXPECTED_WATCHER_TICKS}" -lt 2 ]]; then
  EXPECTED_WATCHER_TICKS=2
fi
PORT="${CURB_DOGFOOD_PORT:-$(python3 - <<'PY'
import socket

with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)}"
ADDR="127.0.0.1:${PORT}"
HOME_DIR="${CURB_DOGFOOD_HOME:-${HOME}}"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/curb-dogfood-observability.XXXXXX")"
STATE_DIR="${SCRATCH}/state"
CONFIG="${OUT}/config.yaml"
LEDGER="${OUT}/ledger.ndjson"
LOG="${OUT}/headless-observability.ndjson"
STDOUT_LOG="${OUT}/server-stdout.txt"

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
require jq
require python3

mkdir -p "${OUT}" "${STATE_DIR}"
rm -f \
  "${LOG}" \
  "${STDOUT_LOG}" \
  "${LEDGER}" \
  "${OUT}/"*.json \
  "${OUT}/"*.txt

cargo build --release --bin curb >"${OUT}/build-release.txt" 2>&1

python3 - "$CONFIG" "$STATE_DIR" "$LEDGER" <<'PY'
from pathlib import Path
import sys

config_path, state_dir, ledger = sys.argv[1:]
source = Path("configs/curb.example.yaml").read_text()
source = source.replace("  state_dir: .curb", f"  state_dir: {state_dir}")
source = source.replace("alerts:\n  local_notifications: true", "alerts:\n  local_notifications: false")
source = source.replace("  path: .curb/runs.ndjson", f"  path: {ledger}")
Path(config_path).write_text(source)
PY

./target/release/curb validate-config "${CONFIG}" >"${OUT}/validate-config.txt"
./target/release/curb usage --since 24h --home "${HOME_DIR}" \
  | sed -n '1,3p' >"${OUT}/usage-since-24h.txt"

CURB_LOG_FORMAT=json ./target/release/curb serve \
  --headless \
  --addr "${ADDR}" \
  --config "${CONFIG}" \
  --home "${HOME_DIR}" \
  >"${STDOUT_LOG}" \
  2>"${LOG}" &
SERVER_PID="$!"

for _ in {1..100}; do
  if curl -fsS "http://${ADDR}/v1/live" >"${OUT}/live.json" 2>/dev/null; then
    break
  fi
  sleep 0.1
done

curl -fsS "http://${ADDR}/v1/live" >"${OUT}/live.json"
curl -sS -o "${OUT}/ready-initial.json" -w "%{http_code}\n" \
  "http://${ADDR}/v1/ready" >"${OUT}/ready-initial.status" || true

TOKEN="$(cat "${STATE_DIR}/api.token")"
AUTH_HEADER="Authorization: Bearer ${TOKEN}"

curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/health" >"${OUT}/health-authenticated.json"
curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/overview" >"${OUT}/overview-initial.json"
curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/sessions" \
  | jq 'length' >"${OUT}/sessions-initial-count.txt"

sleep "${DURATION}"

curl -sS -o "${OUT}/ready-final.json" -w "%{http_code}\n" \
  "http://${ADDR}/v1/ready" >"${OUT}/ready-final.status"
grep -q '^200$' "${OUT}/ready-final.status"
curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/overview" >"${OUT}/overview-final.json"
curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/sessions" \
  | jq 'length' >"${OUT}/sessions-final-count.txt"
curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/events?limit=50" >"${OUT}/events-final.json"

kill "${SERVER_PID}" 2>/dev/null || true
wait "${SERVER_PID}" 2>/dev/null || true
SERVER_PID=""

python3 scripts/parse-observability-smoke.py "${LOG}" >"${OUT}/parse-observability-smoke.txt"

python3 - "${LOG}" "${OUT}/event-summary.json" "${OUT}/event-summary.txt" "${DURATION}" "${EXPECTED_WATCHER_TICKS}" <<'PY'
from collections import Counter
import json
from pathlib import Path
import sys

log_path, json_path, text_path = map(Path, sys.argv[1:4])
duration_seconds = int(sys.argv[4])
expected_watcher_ticks = int(sys.argv[5])
events = [json.loads(line) for line in log_path.read_text().splitlines() if line.strip()]
counts = Counter(event["event"] for event in events)
durations = [
    event.get("duration_ms", 0)
    for event in events
    if event["event"] in {"usage_scan", "watcher_tick"}
]
summary = {
    "duration_seconds": duration_seconds,
    "event_count": len(events),
    "counts": dict(sorted(counts.items())),
    "expected_watcher_ticks": expected_watcher_ticks,
    "max_policy_duration_ms": max(durations) if durations else 0,
}
json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
text_path.write_text(
    f"duration_seconds={summary['duration_seconds']}\n"
    f"events={summary['event_count']}\n"
    f"usage_scan={counts.get('usage_scan', 0)}\n"
    f"watcher_tick={counts.get('watcher_tick', 0)}\n"
    f"expected_watcher_tick_min={summary['expected_watcher_ticks']}\n"
    f"max_policy_duration_ms={summary['max_policy_duration_ms']}\n"
)
if counts.get("usage_scan", 0) < 1:
    raise SystemExit("expected at least one usage_scan event")
if counts.get("watcher_tick", 0) < expected_watcher_ticks:
    raise SystemExit(
        f"expected at least {expected_watcher_ticks} watcher_tick events "
        f"for {duration_seconds}s window"
    )
PY

if rg -F -n "${TOKEN}" "${LOG}" >"${OUT}/redaction-check.txt"; then
  echo "unexpected sensitive material in ${LOG}" >&2
  exit 1
fi
if rg -i -n "Authorization|Bearer|prompt|response|screenshot|keystroke|file[-_ ]?content|file contents|raw provider|raw_provider|provider payload|payload" "${LOG}" >"${OUT}/redaction-check.txt"; then
  echo "unexpected sensitive material in ${LOG}" >&2
  exit 1
fi
printf 'ok: no token, auth header, prompt, response, screenshot, keystroke, file-content, raw-provider, or payload terms in NDJSON\n' >"${OUT}/redaction-check.txt"

cat >"${OUT}/README.md" <<EOF
# Headless Observability Dogfood

Date: ${DATE}

Purpose: run Curb as a headless sidecar against real local provider metadata for
a timed observability window, without enabling termination or writing service
state into the worktree.

Commands:

\`\`\`sh
CURB_DOGFOOD_SECONDS=${DURATION} bash scripts/dogfood-headless-observability.sh ${OUT}
\`\`\`

Environment:

- Address: \`${ADDR}\`
- Config: \`config.yaml\`
- Mode: visibility
- Home scanned: \`${HOME_DIR}\`
- Scratch state: private temporary directory deleted on exit
- Ledger artifact: \`ledger.ndjson\`
- Structured log: \`headless-observability.ndjson\`

Evidence:

- \`build-release.txt\`: release binary build.
- \`validate-config.txt\`: generated scratch-state config validates.
- \`usage-since-24h.txt\`: provider source-health aggregate baseline before the
  headless run. It intentionally omits per-session IDs and local paths.
- \`live.json\`, \`ready-initial.json\`, \`ready-initial.status\`,
  \`ready-final.json\`, \`ready-final.status\`, \`health-authenticated.json\`:
  headless API probes.
- \`overview-initial.json\`, \`overview-final.json\`,
  \`sessions-initial-count.txt\`, \`sessions-final-count.txt\`, and
  \`events-final.json\`: protected API evidence after the timed window.
  Per-session response dumps are intentionally not committed because they
  contain local project labels.
- \`parse-observability-smoke.txt\`: parser accepted the NDJSON artifact and
  required runtime policy fields.
- \`event-summary.json\` and \`event-summary.txt\`: event counts from the timed
  run; the script requires at least one startup \`usage_scan\`, duration-scaled
  repeated \`watcher_tick\` events, and final readiness HTTP 200.
- \`redaction-check.txt\`: token, auth header, prompt, response, screenshot,
  keystroke, file-content, raw-provider, and payload terms were absent from
  NDJSON.

Safety notes:

- The generated config keeps \`mode: visibility\`, so this run cannot terminate
  processes.
- Local notifications are disabled for the dogfood config.
- Provider ingestion remains metadata-only; raw prompt/response content is not
  captured.
EOF

echo "headless observability dogfood ok: ${OUT}"
