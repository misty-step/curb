#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
ARTIFACT_ROOT="$ROOT/demo/006/artifacts"
DRY_RUN=0
MODE="all"

usage() {
  cat <<'USAGE'
usage: run-backlog-006-demo.sh [--dry-run] [--mode alert|enforce|all]

Runs a controlled Curb QA/demo against synthetic Codex usage metadata and a
single harmless sleep worker. It never launches real model agents and never
targets desktop applications.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --mode)
      MODE="${2:-}"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$MODE" in
  alert|enforce|all) ;;
  *)
    echo "invalid mode: $MODE" >&2
    exit 2
    ;;
esac

timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
run_dir="$ARTIFACT_ROOT/live-$timestamp"
curb_bin="$run_dir/curb"
home_dir="$run_dir/home"
work_dir="$run_dir/work"
state_dir="$run_dir/state"
worker_exe="$run_dir/curb-demo-worker"

workers=()
watchers=()
cleanup() {
  for pid in "${watchers[@]:-}"; do
    kill "$pid" >/dev/null 2>&1 || true
    wait "$pid" >/dev/null 2>&1 || true
  done
  for pid in "${workers[@]:-}"; do
    kill "$pid" >/dev/null 2>&1 || true
    wait "$pid" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT

log() {
  printf '%s\n' "$*"
}

run_cmd() {
  local outfile="$1"
  shift
  log "+ $*" | tee "$outfile"
  if [[ "$DRY_RUN" -eq 0 ]]; then
    "$@" >>"$outfile" 2>&1
  fi
}

write_usage_fixture() {
  local session_id="$1"
  local total_tokens="$2"
  local fixture="$home_dir/.codex/archived_sessions/$session_id.jsonl"
  local now
  now="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  cat >"$fixture" <<JSONL
{"timestamp":"$now","type":"session_meta","payload":{"id":"$session_id","cwd":"$work_dir"}}
{"timestamp":"$now","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":$total_tokens},"total_token_usage":{"total_tokens":$total_tokens},"model_context_window":258400}}}
JSONL
}

write_config() {
  local path="$1"
  local ledger="$2"
  local mode="$3"
  cat >"$path" <<YAML
version: 1
profile: backlog-006-demo
mode: $mode
service:
  scan_interval: 1s
  policy_interval: 1s
  heartbeat_interval: 5s
  min_confidence: 50
  state_dir: $state_dir
usage:
  enabled: true
  scan_interval: 1s
  lookback: 1h
  window: 1m
  warn_turn_tokens: 1000
  kill_turn_tokens: 1500
  grace_period: 2s
defaults:
  warn_after: 1h
  kill_after: 2h
  ack_extension: 10s
  max_extensions: 1
  kill_grace_period: 2s
  cooldown_after_kill: 1s
  min_lifetime: 0s
  max_run_gap: 1s
  allow_app_root_kill: false
agents:
  - id: codex-synthetic-sleep
    label: Codex Synthetic Sleep
    family: codex
    kind: process
    match:
      process_names:
        - sleep
        - curb-demo-worker
      require_command_regex:
        - "$worker_exe"
      command_regex:
        - "$worker_exe"
alerts:
  local_notifications: false
ledger:
  path: $ledger
  include_prompt_content: false
YAML
}

start_worker() {
  (cd "$work_dir" && "$worker_exe" 120) >/dev/null 2>&1 &
  local pid=$!
  workers+=("$pid")
  printf '%s\n' "$pid"
}

process_running() {
  local pid="$1"
  local stat
  stat="$(ps -p "$pid" -o stat= 2>/dev/null | tr -d '[:space:]' || true)"
  [[ -n "$stat" && "$stat" != Z* ]]
}

wait_for_ledger_event() {
  local ledger="$1"
  local event="$2"
  local timeout_seconds="$3"
  local start
  start="$(date +%s)"
  while true; do
    if [[ -f "$ledger" ]] && grep -q "\"type\":\"$event\"" "$ledger"; then
      return 0
    fi
    if (( $(date +%s) - start >= timeout_seconds )); then
      return 1
    fi
    sleep 1
  done
}

run_watch_until() {
  local config="$1"
  local ledger="$2"
  local event="$3"
  local outfile="$4"
  env HOME="$home_dir" "$curb_bin" watch --config "$config" >"$outfile" 2>&1 &
  local pid=$!
  watchers+=("$pid")
  if ! wait_for_ledger_event "$ledger" "$event" 25; then
    {
      echo "timed out waiting for $event"
      echo
      cat "$outfile" 2>/dev/null || true
      echo
      cat "$ledger" 2>/dev/null || true
    } >&2
    return 1
  fi
  kill "$pid" >/dev/null 2>&1 || true
  wait "$pid" >/dev/null 2>&1 || true
}

if [[ "$DRY_RUN" -eq 1 ]]; then
  cat <<EOF
dry-run: would create $run_dir
dry-run: would build $curb_bin
dry-run: would run alert-mode would-stop and enforcement termination demos
dry-run: only target process would be a synthetic sleep worker launched from $work_dir
EOF
  exit 0
fi

mkdir -p "$run_dir" "$home_dir/.codex/archived_sessions" "$work_dir" "$state_dir"
ln -sfn "$run_dir" "$ARTIFACT_ROOT/latest"

run_cmd "$run_dir/00-build.txt" go build -o "$curb_bin" "$ROOT/cmd/curb"
ln -sf /bin/sleep "$worker_exe"

if [[ "$MODE" == "alert" || "$MODE" == "all" ]]; then
  alert_config="$run_dir/alert.yaml"
  alert_ledger="$run_dir/alert.ndjson"
  write_usage_fixture "curb-demo-alert" 2000
  write_config "$alert_config" "$alert_ledger" alert
  alert_worker="$(start_worker)"
  run_cmd "$run_dir/01-alert-config.txt" env HOME="$home_dir" "$curb_bin" validate-config "$alert_config"
  run_cmd "$run_dir/02-alert-dashboard.json" env HOME="$home_dir" "$curb_bin" dashboard --config "$alert_config" --json
  run_watch_until "$alert_config" "$alert_ledger" usage_would_terminate "$run_dir/03-alert-watch.txt"
  if process_running "$alert_worker"; then
    echo "alert worker still alive as expected" >"$run_dir/04-alert-result.txt"
    kill "$alert_worker" >/dev/null 2>&1 || true
    wait "$alert_worker" >/dev/null 2>&1 || true
  else
    echo "alert worker was stopped unexpectedly" >"$run_dir/04-alert-result.txt"
    exit 1
  fi
fi

if [[ "$MODE" == "enforce" || "$MODE" == "all" ]]; then
  enforce_config="$run_dir/enforcement.yaml"
  enforce_ledger="$run_dir/enforcement.ndjson"
  rm -f "$home_dir/.codex/archived_sessions/"*.jsonl
  write_usage_fixture "curb-demo-enforce" 2000
  write_config "$enforce_config" "$enforce_ledger" enforcement
  enforce_worker="$(start_worker)"
  run_cmd "$run_dir/05-enforce-config.txt" env HOME="$home_dir" "$curb_bin" validate-config "$enforce_config"
  run_watch_until "$enforce_config" "$enforce_ledger" usage_termination_completed "$run_dir/06-enforce-watch.txt"
  if process_running "$enforce_worker"; then
    echo "enforcement worker is still alive unexpectedly" >"$run_dir/07-enforce-result.txt"
    exit 1
  else
    echo "enforcement worker stopped as expected" >"$run_dir/07-enforce-result.txt"
  fi
fi

cat >"$run_dir/REPORT.md" <<EOF
# Curb Controlled QA Demo

- Artifact directory: \`$run_dir\`
- Binary: \`$curb_bin\`
- HOME: \`$home_dir\`
- Worker: synthetic \`sleep\` process launched from \`$work_dir\`
- Usage source: synthetic Codex token-count metadata under isolated HOME
- Privacy: no prompts, responses, screenshots, keystrokes, or file contents

## Results

$(if [[ -f "$run_dir/04-alert-result.txt" ]]; then cat "$run_dir/04-alert-result.txt"; fi)
$(if [[ -f "$run_dir/07-enforce-result.txt" ]]; then cat "$run_dir/07-enforce-result.txt"; fi)

## Evidence

- Alert watch output: \`03-alert-watch.txt\`
- Alert ledger: \`alert.ndjson\`
- Enforcement watch output: \`06-enforce-watch.txt\`
- Enforcement ledger: \`enforcement.ndjson\`
EOF

cat >"$run_dir/demo.html" <<EOF
<!doctype html>
<meta charset="utf-8">
<title>Curb Controlled QA Demo</title>
<style>
body{font:14px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace;margin:32px;background:#f6f8fb;color:#142033}
section{background:white;border:1px solid #d7e0ea;border-radius:8px;padding:16px;margin:16px 0}
pre{white-space:pre-wrap;background:#0f172a;color:#dbeafe;padding:16px;border-radius:6px;overflow:auto}
.ok{color:#047857;font-weight:700}
</style>
<h1>Curb Controlled QA Demo</h1>
<p class="ok">Synthetic alert and enforcement demo completed.</p>
<section><h2>Report</h2><pre>$(sed 's/&/\&amp;/g; s/</\&lt;/g' "$run_dir/REPORT.md")</pre></section>
<section><h2>Alert Watch</h2><pre>$(if [[ -f "$run_dir/03-alert-watch.txt" ]]; then sed 's/&/\&amp;/g; s/</\&lt;/g' "$run_dir/03-alert-watch.txt"; fi)</pre></section>
<section><h2>Enforcement Watch</h2><pre>$(if [[ -f "$run_dir/06-enforce-watch.txt" ]]; then sed 's/&/\&amp;/g; s/</\&lt;/g' "$run_dir/06-enforce-watch.txt"; fi)</pre></section>
EOF

log "demo complete: $run_dir"
log "latest: $ARTIFACT_ROOT/latest"
