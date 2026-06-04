# Stop Rejection Dogfood

Date: 2026-06-04

Purpose: prove a safe stop-rejection path through the headless API and
structured logs without terminating any process.

## Environment

- Command cwd: `/Users/phaedrus/.codex/worktrees/066e/curb`
- Binary: `./target/release/curb`
- Config: `configs/curb.example.yaml`
- Mode: visibility/watch behavior from the example config, not enforcement
- Clean run address: `127.0.0.1:18767`
- Log format: `CURB_LOG_FORMAT=json`

Generated `.curb/` service state included an API token, ledger, and usage
cache. It was removed after evidence capture.

## Commands

```sh
cargo build --release --bin curb > evidence/dogfood/2026-06-04-stop-rejection/build-release.txt 2>&1
CURB_LOG_FORMAT=json ./target/release/curb serve --headless --addr 127.0.0.1:18767 --config configs/curb.example.yaml 2> evidence/dogfood/2026-06-04-stop-rejection/stop-rejection.ndjson
curl -fsS http://127.0.0.1:18767/v1/live > evidence/dogfood/2026-06-04-stop-rejection/live-clean.json
curl -fsS http://127.0.0.1:18767/v1/ready > evidence/dogfood/2026-06-04-stop-rejection/ready-clean.json
curl -i http://127.0.0.1:18767/v1/health > evidence/dogfood/2026-06-04-stop-rejection/health-unauthenticated-clean.txt
curl -fsS -H "Authorization: Bearer $(cat .curb/api.token)" http://127.0.0.1:18767/v1/health > evidence/dogfood/2026-06-04-stop-rejection/health-authenticated-clean.json
curl -fsS -H "Authorization: Bearer $(cat .curb/api.token)" http://127.0.0.1:18767/v1/sessions > /tmp/curb-sessions.json
jq '.[0]' /tmp/curb-sessions.json > evidence/dogfood/2026-06-04-stop-rejection/selected-session-clean.json
curl -i -X POST -H "Authorization: Bearer $(cat .curb/api.token)" -H 'Content-Type: application/json' --data @evidence/dogfood/2026-06-04-stop-rejection/stop-request-clean.json "http://127.0.0.1:18767/v1/sessions/<encoded-session-key>/stop" > evidence/dogfood/2026-06-04-stop-rejection/stop-rejected-clean.txt
python3 scripts/parse-observability-smoke.py evidence/dogfood/2026-06-04-stop-rejection/stop-rejection.ndjson > evidence/dogfood/2026-06-04-stop-rejection/parse-stop-rejection-observability-smoke.txt
```

The stop URL used the selected session's URL-encoded `key`; the raw key was not
kept as a separate artifact. The stop request used the supported wire scope
`tree`.

## Results

- `live-clean.json`: public `/v1/live` returned live.
- `ready-clean.json`: public `/v1/ready` returned ready.
- `health-unauthenticated-clean.txt`: protected `/v1/health` returned
  `401 Unauthorized` without a token.
- `health-authenticated-clean.json`: protected `/v1/health` returned
  `{"ok":true,"app":"curb","api_version":1}` with the startup-emitted token.
- `selected-session-clean.json`: captured one live session with
  `can_stop: false` in non-enforcement mode.
- `stop-request-clean.json`: records the stop payload sent with `scope:"tree"`
  and the session's current identity evidence.
- `stop-rejected-clean.txt`: the stop request returned `409 Conflict` with
  `{"error":"enforcement mode is required"}`.
- `stop-rejection.ndjson`: parser accepted 17 structured events, including
  `stop_rejection` with status `409`, the templated stop route
  `/v1/sessions/{session_key}/stop`, health checks, readiness, watcher ticks,
  and shutdown.

## Safety Assertions

- No process was terminated: the config was not enforcement mode and the stop
  write path rejected before termination.
- Structured logs did not include the operator stop reason, API token,
  authorization header, or raw session key.
- Generated `.curb/` token/cache/ledger state was removed after capture.

## Residual Risk

- This proves safe rejection, not successful enforcement termination.
- A future enforcement dogfood should use a harmless synthetic subprocess and
  prove `stop_decision`/termination behavior without touching real agent roots.
