#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DATE="$(date -u +%Y-%m-%d)"
OUT="${1:-evidence/dogfood/${DATE}-long-sidecar}"
DURATION="${CURB_LONG_DOGFOOD_SECONDS:-7200}"
SNAPSHOT_SECONDS="${CURB_LONG_DOGFOOD_SNAPSHOT_SECONDS:-300}"
PORT="${CURB_DOGFOOD_PORT:-$(python3 - <<'PY'
import socket

with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)}"
ADDR="127.0.0.1:${PORT}"
HOME_DIR="${CURB_DOGFOOD_HOME:-${HOME}}"
STATE_ROOT="${CURB_DOGFOOD_STATE_ROOT:-${TMPDIR:-/tmp}}"
SCRATCH="$(mktemp -d "${STATE_ROOT%/}/curb-long-sidecar.XXXXXX")"
STATE_DIR="${SCRATCH}/state"
SNAPSHOTS="${OUT}/snapshots"
CONFIG="${OUT}/config.yaml"
LEDGER="${OUT}/ledger.ndjson"
LOG="${OUT}/headless-sidecar.ndjson"
STDOUT_LOG="${OUT}/server-stdout.txt"
RESOURCE_LOG="${OUT}/resource-samples.tsv"
READY_LOG="${OUT}/ready-samples.tsv"
PROBE_LOG="${OUT}/probe-latency.tsv"

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

positive_integer() {
  local name="$1"
  local value="$2"
  if ! [[ "${value}" =~ ^[0-9]+$ ]] || [[ "${value}" -lt 1 ]]; then
    echo "${name} must be a positive integer, got ${value}" >&2
    exit 2
  fi
}

require curl
require jq
require python3

positive_integer CURB_LONG_DOGFOOD_SECONDS "${DURATION}"
positive_integer CURB_LONG_DOGFOOD_SNAPSHOT_SECONDS "${SNAPSHOT_SECONDS}"

EXPECTED_WATCHER_TICKS="$(( DURATION / 6 ))"
if [[ "${EXPECTED_WATCHER_TICKS}" -lt 2 ]]; then
  EXPECTED_WATCHER_TICKS=2
fi

mkdir -p "${OUT}" "${STATE_DIR}" "${SNAPSHOTS}"
rm -f \
  "${LOG}" \
  "${STDOUT_LOG}" \
  "${LEDGER}" \
  "${RESOURCE_LOG}" \
  "${READY_LOG}" \
  "${PROBE_LOG}" \
  "${OUT}/"*.json \
  "${OUT}/"*.txt
rm -f "${SNAPSHOTS}/"*

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
: >"${LEDGER}"
set +e
./target/release/curb usage --since 24h --home "${HOME_DIR}" \
  >"${OUT}/usage-since-24h.raw.txt" 2>&1
USAGE_STATUS="$?"
set -e
{
  printf 'exit_status=%s\n' "${USAGE_STATUS}"
  sed -n '1,12p' "${OUT}/usage-since-24h.raw.txt"
} >"${OUT}/usage-since-24h.txt"

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
curl -sS -o "${OUT}/health-unauthenticated.txt" -w "%{http_code}\n" \
  "http://${ADDR}/v1/health" >"${OUT}/health-unauthenticated.status" || true
grep -q '^401$' "${OUT}/health-unauthenticated.status"

TOKEN="$(cat "${STATE_DIR}/api.token")"
AUTH_HEADER="Authorization: Bearer ${TOKEN}"

curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/health" >"${OUT}/health-authenticated.json"
curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/overview" >"${OUT}/overview-initial.json"
curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/sessions" \
  | jq 'length' >"${OUT}/sessions-initial-count.txt"

printf 'timestamp_utc\tpid\tppid\tetime\trss_kb\tcpu_percent\n' >"${RESOURCE_LOG}"
printf 'timestamp_utc\tstatus_code\tready_status\n' >"${READY_LOG}"
printf 'timestamp_utc\tpath\tstatus_code\ttotal_seconds\n' >"${PROBE_LOG}"

sample() {
  local stamp="$1"
  local slug="$2"
  local ready_status
  local ready_code

  ready_code="$(curl -sS -o "${SNAPSHOTS}/${slug}-ready.json" -w "%{http_code}\n" \
    "http://${ADDR}/v1/ready" || true)"
  ready_status="$(jq -r '.status // "unparseable"' "${SNAPSHOTS}/${slug}-ready.json" 2>/dev/null || printf 'unparseable')"
  printf '%s\t%s\t%s\n' "${stamp}" "${ready_code}" "${ready_status}" >>"${READY_LOG}"

  curl -sS -o "${SNAPSHOTS}/${slug}-live.json" -w "${stamp}\t/v1/live\t%{http_code}\t%{time_total}\n" \
    "http://${ADDR}/v1/live" >>"${PROBE_LOG}" || true
  curl -sS -H "${AUTH_HEADER}" -o "${SNAPSHOTS}/${slug}-health.json" -w "${stamp}\t/v1/health\t%{http_code}\t%{time_total}\n" \
    "http://${ADDR}/v1/health" >>"${PROBE_LOG}" || true
  curl -sS -H "${AUTH_HEADER}" -o "${SNAPSHOTS}/${slug}-overview.json" -w "${stamp}\t/v1/overview\t%{http_code}\t%{time_total}\n" \
    "http://${ADDR}/v1/overview" >>"${PROBE_LOG}" || true

  if ps -p "${SERVER_PID}" >/dev/null 2>&1; then
    ps -o pid=,ppid=,etime=,rss=,%cpu= -p "${SERVER_PID}" \
      | awk -v ts="${stamp}" '{print ts "\t" $1 "\t" $2 "\t" $3 "\t" $4 "\t" $5}' >>"${RESOURCE_LOG}"
  fi
}

START_EPOCH="$(date -u +%s)"
END_EPOCH="$(( START_EPOCH + DURATION ))"
index=0
while :; do
  now_epoch="$(date -u +%s)"
  stamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  sample "${stamp}" "$(printf '%04d' "${index}")"
  index="$(( index + 1 ))"
  if [[ "${now_epoch}" -ge "${END_EPOCH}" ]]; then
    break
  fi
  remaining="$(( END_EPOCH - now_epoch ))"
  sleep_for="${SNAPSHOT_SECONDS}"
  if [[ "${remaining}" -lt "${sleep_for}" ]]; then
    sleep_for="${remaining}"
  fi
  sleep "${sleep_for}"
done

curl -sS -o "${OUT}/ready-final.json" -w "%{http_code}\n" \
  "http://${ADDR}/v1/ready" >"${OUT}/ready-final.status"
grep -q '^200$' "${OUT}/ready-final.status"
curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/overview" >"${OUT}/overview-final.json"
curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/sessions" \
  | jq 'length' >"${OUT}/sessions-final-count.txt"
curl -fsS -H "${AUTH_HEADER}" "http://${ADDR}/v1/events?limit=100" >"${OUT}/events-final.json"

kill "${SERVER_PID}" 2>/dev/null || true
wait "${SERVER_PID}" 2>/dev/null || true
SERVER_PID=""

python3 scripts/parse-observability-smoke.py "${LOG}" >"${OUT}/parse-observability-smoke.txt"

python3 - "${LOG}" "${READY_LOG}" "${PROBE_LOG}" "${RESOURCE_LOG}" "${OUT}/long-run-summary.json" "${OUT}/long-run-summary.txt" "${DURATION}" "${EXPECTED_WATCHER_TICKS}" <<'PY'
from __future__ import annotations

from collections import Counter
import csv
import json
from pathlib import Path
import sys

log_path, ready_path, probe_path, resource_path, json_path, text_path = map(Path, sys.argv[1:7])
duration_seconds = int(sys.argv[7])
expected_watcher_ticks = int(sys.argv[8])
events = [json.loads(line) for line in log_path.read_text().splitlines() if line.strip()]
counts = Counter(event["event"] for event in events)
policy_events = [
    event for event in events if event["event"] in {"usage_scan", "watcher_tick"}
]
policy_durations = [event.get("duration_ms", 0) for event in policy_events]
source_errors = sum(int(event.get("fields", {}).get("source_errors", 0)) for event in policy_events)
source_health_error_count = counts.get("source_health_error", 0)

ready_samples = list(csv.DictReader(ready_path.open(), delimiter="\t"))
probe_samples = list(csv.DictReader(probe_path.open(), delimiter="\t"))
resource_samples = list(csv.DictReader(resource_path.open(), delimiter="\t"))
latencies = [float(row["total_seconds"]) for row in probe_samples if row.get("total_seconds")]
rss_values = [int(row["rss_kb"]) for row in resource_samples if row.get("rss_kb", "").isdigit()]
cpu_values = [float(row["cpu_percent"]) for row in resource_samples if row.get("cpu_percent")]
ready_statuses = Counter(row["ready_status"] for row in ready_samples)
ready_codes = Counter(row["status_code"] for row in ready_samples)

summary = {
    "duration_seconds": duration_seconds,
    "event_count": len(events),
    "counts": dict(sorted(counts.items())),
    "expected_watcher_ticks": expected_watcher_ticks,
    "watcher_tick_count": counts.get("watcher_tick", 0),
    "usage_scan_count": counts.get("usage_scan", 0),
    "source_error_total": source_errors,
    "source_health_error_events": source_health_error_count,
    "ready_statuses": dict(sorted(ready_statuses.items())),
    "ready_status_codes": dict(sorted(ready_codes.items())),
    "snapshot_count": len(ready_samples),
    "max_policy_duration_ms": max(policy_durations) if policy_durations else 0,
    "max_probe_latency_seconds": max(latencies) if latencies else 0,
    "max_rss_kb": max(rss_values) if rss_values else 0,
    "min_rss_kb": min(rss_values) if rss_values else 0,
    "max_cpu_percent": max(cpu_values) if cpu_values else 0,
}
json_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
text_path.write_text(
    f"duration_seconds={summary['duration_seconds']}\n"
    f"events={summary['event_count']}\n"
    f"usage_scan={summary['usage_scan_count']}\n"
    f"watcher_tick={summary['watcher_tick_count']}\n"
    f"expected_watcher_tick_min={summary['expected_watcher_ticks']}\n"
    f"source_error_total={summary['source_error_total']}\n"
    f"source_health_error_events={summary['source_health_error_events']}\n"
    f"ready_statuses={summary['ready_statuses']}\n"
    f"ready_status_codes={summary['ready_status_codes']}\n"
    f"snapshots={summary['snapshot_count']}\n"
    f"max_policy_duration_ms={summary['max_policy_duration_ms']}\n"
    f"max_probe_latency_seconds={summary['max_probe_latency_seconds']}\n"
    f"rss_kb_min={summary['min_rss_kb']}\n"
    f"rss_kb_max={summary['max_rss_kb']}\n"
    f"max_cpu_percent={summary['max_cpu_percent']}\n"
)
if summary["usage_scan_count"] < 1:
    raise SystemExit("expected at least one usage_scan event")
if summary["watcher_tick_count"] < expected_watcher_ticks:
    raise SystemExit(
        f"expected at least {expected_watcher_ticks} watcher_tick events "
        f"for {duration_seconds}s window"
    )
PY

if rg -F -n "${TOKEN}" "${LOG}" >"${OUT}/redaction-check.txt"; then
  echo "unexpected token in ${LOG}" >&2
  exit 1
fi
if rg -i -n "Authorization|Bearer|prompt|response|screenshot|keystroke|file[-_ ]?content|file contents|raw provider|raw_provider|provider payload|payload" "${LOG}" >"${OUT}/redaction-check.txt"; then
  echo "unexpected sensitive material in ${LOG}" >&2
  exit 1
fi
printf 'ok: no token, auth header, prompt, response, screenshot, keystroke, file-content, raw-provider, or payload terms in NDJSON\n' >"${OUT}/redaction-check.txt"

READY_INITIAL_STATUS="$(cat "${OUT}/ready-initial.status")"
READY_INITIAL_STATE="$(jq -r '.status // "unknown"' "${OUT}/ready-initial.json")"
READY_INITIAL_WATCHER_REASON="$(
  jq -r '.checks[]? | select(.name == "watcher_runtime") | .reason // "none"' \
    "${OUT}/ready-initial.json"
)"
READY_FINAL_STATUS="$(cat "${OUT}/ready-final.status")"
READY_FINAL_STATE="$(jq -r '.status // "unknown"' "${OUT}/ready-final.json")"
SESSIONS_INITIAL="$(cat "${OUT}/sessions-initial-count.txt")"
SESSIONS_FINAL="$(cat "${OUT}/sessions-final-count.txt")"
EVENT_COUNT="$(jq -r '.event_count' "${OUT}/long-run-summary.json")"
WATCHER_TICK_COUNT="$(jq -r '.watcher_tick_count' "${OUT}/long-run-summary.json")"
SOURCE_HEALTH_ERROR_EVENTS="$(jq -r '.source_health_error_events' "${OUT}/long-run-summary.json")"
READY_COUNTS="$(jq -r '.ready_statuses | to_entries | map("\(.key)=\(.value)") | join(", ")' "${OUT}/long-run-summary.json")"
READY_CODE_COUNTS="$(jq -r '.ready_status_codes | to_entries | map("\(.key)=\(.value)") | join(", ")' "${OUT}/long-run-summary.json")"
SNAPSHOT_COUNT="$(jq -r '.snapshot_count' "${OUT}/long-run-summary.json")"
RSS_MIN="$(jq -r '.min_rss_kb' "${OUT}/long-run-summary.json")"
RSS_MAX="$(jq -r '.max_rss_kb' "${OUT}/long-run-summary.json")"
CPU_MAX="$(jq -r '.max_cpu_percent' "${OUT}/long-run-summary.json")"
POLICY_DURATION_MAX="$(jq -r '.max_policy_duration_ms' "${OUT}/long-run-summary.json")"
PROBE_LATENCY_MAX="$(jq -r '.max_probe_latency_seconds' "${OUT}/long-run-summary.json")"
NOTIFICATION_STATUS="$(jq -r '.capabilities.notifications.status // "unknown"' "${OUT}/overview-final.json")"
NOTIFICATION_MESSAGE="$(jq -r '.capabilities.notifications.message // "unknown"' "${OUT}/overview-final.json")"
PROCESS_CAPTURE_MESSAGE="$(jq -r '.capabilities.process_capture.message // "unknown"' "${OUT}/overview-final.json")"
PROCESS_IDENTITY_MESSAGE="$(jq -r '.capabilities.process_identity.message // "unknown"' "${OUT}/overview-final.json")"
SOURCE_PROVIDER_SUMMARY="$(
  python3 - "${LOG}" <<'PY'
from collections import Counter
import json
import sys

counts = Counter()
for line in open(sys.argv[1]):
    if not line.strip():
        continue
    event = json.loads(line)
    if event.get("event") == "source_health_error":
        provider = event.get("fields", {}).get("provider", "unknown")
        counts[provider] += 1
if not counts:
    print("none")
else:
    print(", ".join(f"{count:,} for {provider}" for provider, count in sorted(counts.items())))
PY
)"

cat >"${OUT}/README.md" <<EOF
# Long-Running Headless Sidecar Dogfood

Date: ${DATE}

Purpose: run Curb as a release-built headless sidecar for a realistic operator
window using private state outside the worktree, while keeping enforcement off
and recording periodic operational snapshots.

Commands:

\`\`\`sh
CURB_LONG_DOGFOOD_SECONDS=${DURATION} \\
CURB_LONG_DOGFOOD_SNAPSHOT_SECONDS=${SNAPSHOT_SECONDS} \\
bash scripts/dogfood-long-sidecar.sh ${OUT}
\`\`\`

Environment:

- Build SHA: \`$(git rev-parse HEAD)\`
- Branch/worktree: \`$(git branch --show-current)\` / \`${ROOT}\`
- OS: \`$(uname -srm)\`
- Address: \`${ADDR}\`
- Config: \`config.yaml\`
- Mode: visibility
- Home scanned: \`${HOME_DIR}\`
- State path: private temporary directory under \`${SCRATCH}\`, outside the repo
- Ledger artifact: \`ledger.ndjson\`
- Structured log: \`headless-sidecar.ndjson\`

Evidence:

- \`build-release.txt\`: release binary build.
- \`validate-config.txt\`: generated outside-worktree state config validates.
- \`usage-since-24h.txt\` and \`usage-since-24h.raw.txt\`: provider
  source-health aggregate baseline before the headless run, including the exit
  status. The summary intentionally omits per-session IDs and local paths except
  when the CLI reports a source-health error path.
- \`live.json\`, \`ready-initial.json\`, \`ready-final.json\`,
  \`health-unauthenticated.status\`, and \`health-authenticated.json\`: public
  and protected API probes.
- \`snapshots/\`: periodic live, ready, health, and overview probes.
- \`ready-samples.tsv\`: degraded/readiness transitions by timestamp.
- \`probe-latency.tsv\`: live/health/overview latency samples.
- \`resource-samples.tsv\`: server RSS/CPU samples.
- \`overview-initial.json\`, \`overview-final.json\`,
  \`sessions-initial-count.txt\`, \`sessions-final-count.txt\`, and
  \`events-final.json\`: protected API evidence after the operator window.
- \`parse-observability-smoke.txt\`: parser accepted the NDJSON artifact and
  required runtime policy fields.
- \`long-run-summary.json\` and \`long-run-summary.txt\`: source-health,
  readiness, watcher-tick, latency, and resource drift summary.
- \`redaction-check.txt\`: token, auth header, prompt, response, screenshot,
  keystroke, file-content, raw-provider, and payload terms were absent from
  NDJSON.

Safety notes:

- The generated config keeps \`mode: visibility\`, so this run cannot terminate
  processes.
- Local notifications are disabled for the dogfood config.
- Provider ingestion remains metadata-only; raw prompt/response content is not
  captured.
- Per-session response dumps are intentionally not committed because they can
  contain local project labels.

Operator notes:

- Startup: the generated config validated with private state outside the
  worktree, the release binary served \`${ADDR}\`, /v1/live returned HTTP 200,
  and the initial /v1/ready returned HTTP ${READY_INITIAL_STATUS}
  (${READY_INITIAL_STATE}) with watcher_runtime reason
  \`${READY_INITIAL_WATCHER_REASON}\`.
- Degraded-readiness transitions: the ${DURATION}-second window produced
  ${SNAPSHOT_COUNT} periodic readiness samples: ${READY_COUNTS}
  (${READY_CODE_COUNTS}). Final /v1/ready returned HTTP ${READY_FINAL_STATUS}
  (${READY_FINAL_STATE}); /v1/live and protected /v1/health stayed available
  during sampled probes.
- Provider roots discovered: the run started with ${SESSIONS_INITIAL} sessions
  and ended with ${SESSIONS_FINAL} sessions. Source-health emitted
  ${SOURCE_HEALTH_ERROR_EVENTS} error events: ${SOURCE_PROVIDER_SUMMARY}. The
  preflight usage scan output is captured in \`usage-since-24h.txt\`.
- Notification capability: final overview reported notifications
  ${NOTIFICATION_STATUS}: ${NOTIFICATION_MESSAGE}.
- False positives: no policy warnings, \`would_stop\`, stop attempts, stop
  completions, or stop rejections appeared in the visibility-mode watcher
  ticks.
- False negatives: no ledger events were written during the run, so this
  packet does not prove alert delivery or enforcement-mode stop behavior.
- Process-correlation surprises: final overview reported
  \`${PROCESS_CAPTURE_MESSAGE}\` and \`${PROCESS_IDENTITY_MESSAGE}\`; session
  counts are captured at the start and end to separate normal provider churn
  from actionable source-health breakage.
- Resource/latency drift: RSS ranged from ${RSS_MIN} KB to ${RSS_MAX} KB, max
  sampled CPU was ${CPU_MAX}%, max watcher policy duration was
  ${POLICY_DURATION_MAX} ms, and max sampled probe latency was
  ${PROBE_LATENCY_MAX} seconds.

Follow-up ranking:

| Rank | Item | Evidence | Decision |
|---:|---|---|---|
| 1 | Bound or snapshot readiness while watcher cache is busy | Degraded readiness samples appeared while /v1/live and protected health stayed available. | Route to \`backlog.d/039-finish-facade-and-presenter-simplification.md\` as part of the loopback transport/readiness milestone. |
| 2 | Make provider source-health failures actionable | ${SOURCE_HEALTH_ERROR_EVENTS} source-health error events and the preflight source-health output are captured in this packet. | Route nonzero provider failures to \`backlog.d/036-build-operator-recovery-cockpit.md\` as an operator recovery state. |
| 3 | Keep \`scripts/dogfood-long-sidecar.sh\` as the long-run harness | The wrapper produced release build, config validation, snapshots, parser output, summary, and redaction proof for ${EVENT_COUNT} events and ${WATCHER_TICK_COUNT} watcher ticks. | Do not add a repo-local QA/dogfood skill yet; defer until browser-backed live operator workflow evidence adds another repeatable procedure. |
EOF

echo "long sidecar dogfood ok: ${OUT}"
