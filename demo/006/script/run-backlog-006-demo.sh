#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
ARTIFACT_DIR="$ROOT/demo/006/artifacts"
MODE="all"
DRY_RUN=0

usage() {
  cat <<'USAGE'
usage: run-backlog-006-demo.sh [--dry-run] [--mode observe|warn|ack|enforce|all]

Print or run a safe Curb demo against a synthetic sleep worker. The default
path is dry-run friendly and never launches real model agents.
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
  observe|warn|ack|enforce|all) ;;
  *)
    echo "invalid mode: $MODE" >&2
    exit 2
    ;;
esac

mkdir -p "$ARTIFACT_DIR"

run() {
  printf '+ %s\n' "$*"
  if [[ "$DRY_RUN" -eq 0 ]]; then
    "$@"
  fi
}

note() {
  printf '\n# %s\n' "$*"
}

curb_bin="$ARTIFACT_DIR/curb-demo"
demo_home="$ARTIFACT_DIR/home"
demo_config="$ARTIFACT_DIR/curb.demo.yaml"
demo_ledger="$ARTIFACT_DIR/runs.ndjson"

note "Safety preflight"
printf 'demo_home=%s\n' "$demo_home"
printf 'demo_config=%s\n' "$demo_config"
printf 'demo_ledger=%s\n' "$demo_ledger"
printf 'worker=synthetic sleep only\n'
printf 'privacy=no prompts, responses, screenshots, keystrokes, or file contents\n'

note "Build the exact binary under test"
run go build -o "$curb_bin" "$ROOT/cmd/curb"

note "Create an isolated demo config"
cat >"$demo_config" <<YAML
version: 1
profile: backlog-006-demo
mode: alert
service:
  scan_interval: 1s
  policy_interval: 1s
  heartbeat_interval: 5s
  min_confidence: 50
  state_dir: $ARTIFACT_DIR/state
usage:
  enabled: false
defaults:
  warn_after: 4s
  kill_after: 8s
  ack_extension: 10s
  max_extensions: 1
  kill_grace_period: 2s
  cooldown_after_kill: 1s
  min_lifetime: 0s
  max_run_gap: 1s
  allow_app_root_kill: false
agents:
  - id: synthetic-sleep
    label: Synthetic Sleep
    family: synthetic
    kind: process
    match:
      process_names:
        - sleep
      command_regex:
        - "\\bsleep\\b"
    policy:
      warn_after: 4s
      kill_after: 8s
      ack_extension: 10s
      max_extensions: 1
alerts:
  local_notifications: false
ledger:
  path: $demo_ledger
  include_prompt_content: false
YAML
printf '+ wrote %s\n' "$demo_config"

note "Observe"
run env HOME="$demo_home" "$curb_bin" validate-config "$demo_config"
run env HOME="$demo_home" "$curb_bin" scan --config "$demo_config" --json

note "Warn and acknowledge"
printf '+ start synthetic worker: sleep 120\n'
printf '+ run watcher in alert mode until warning appears in %s\n' "$demo_ledger"
printf '+ acknowledge the reported run id with: %s ack <run-id> --config %s --extend 10s --reason demo\n' "$curb_bin" "$demo_config"

note "Enforce"
printf '+ switch demo config mode to enforcement\n'
printf '+ start a fresh sleep worker, wait for grace, verify only that sleep PID exits\n'
printf '+ inspect evidence with: %s runs --config %s --json --all\n' "$curb_bin" "$demo_config"

note "Storyboard"
printf '+ render storyboard from demo/006/storyboard.md\n'
printf '+ optional Remotion source: demo/remotion\n'

if [[ "$DRY_RUN" -eq 1 ]]; then
  printf '\ndry-run complete; no model agents or desktop apps were launched or stopped\n'
fi
