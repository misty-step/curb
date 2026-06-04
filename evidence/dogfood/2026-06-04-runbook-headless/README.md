# Headless Runbook Dogfood

Date: 2026-06-04

Purpose: verify the new headless sidecar and observability dogfood runbooks
against a real release-build service run.

## Environment

- Command cwd: `/Users/phaedrus/.codex/worktrees/066e/curb`
- Binary: `./target/release/curb`
- Config: `configs/curb.example.yaml`
- Address: `127.0.0.1:18765`
- Log format: `CURB_LOG_FORMAT=json`
- State path emitted by service: `.curb/`

Generated `.curb/` state included an API token, ledger, and usage cache. It was
removed after safe evidence capture so no token value remains in the worktree.

## Commands

```sh
cargo build --release --bin curb > evidence/dogfood/2026-06-04-runbook-headless/build-release.txt 2>&1
CURB_LOG_FORMAT=json ./target/release/curb serve --headless --addr 127.0.0.1:18765 --config configs/curb.example.yaml 2> evidence/dogfood/2026-06-04-runbook-headless/headless.ndjson
curl -fsS http://127.0.0.1:18765/v1/live > evidence/dogfood/2026-06-04-runbook-headless/live.json
curl -fsS http://127.0.0.1:18765/v1/ready > evidence/dogfood/2026-06-04-runbook-headless/ready.json
curl -i http://127.0.0.1:18765/ > evidence/dogfood/2026-06-04-runbook-headless/root.txt
curl -i http://127.0.0.1:18765/v1/health > evidence/dogfood/2026-06-04-runbook-headless/health-unauthenticated.txt
curl -fsS -H "Authorization: Bearer $(cat .curb/api.token)" http://127.0.0.1:18765/v1/health > evidence/dogfood/2026-06-04-runbook-headless/health-authenticated.json
curl -fsS -H "Authorization: Bearer $(cat .curb/api.token)" http://127.0.0.1:18765/v1/overview > evidence/dogfood/2026-06-04-runbook-headless/overview-authenticated.json
python3 scripts/parse-observability-smoke.py evidence/dogfood/2026-06-04-runbook-headless/headless.ndjson > evidence/dogfood/2026-06-04-runbook-headless/parse-observability-smoke.txt
```

## Results

- `live.json`: public `/v1/live` returned `{"status":"live","app":"curb","api_version":1}`.
- `build-release.txt`: release build completed successfully for `curb`.
- `ready.json`: public `/v1/ready` returned `ready` with config, ledger,
  usage reader, platform capabilities, notifications, and watcher runtime all
  `ok`.
- `root.txt`: root returned `404` with `{"error":"headless server","app":"curb","ui":false}`.
- `health-unauthenticated.txt`: protected `/v1/health` without a token returned
  `401 Unauthorized`.
- `health-authenticated.json`: protected `/v1/health` with the startup-emitted
  `.curb/api.token` returned `{"ok":true,"app":"curb","api_version":1}`.
- `headless.ndjson`: parser accepted 18 structured events, including
  `config_loaded`, `server_started`, `usage_scan`, `watcher_tick`,
  `readiness_check`, `health_check`, `api_request`, and `shutdown`.
- Redaction check found no `Authorization`, `Bearer`, prompt, response,
  screenshot, keystroke, or API token value in `headless.ndjson`.

## Observations

- The runbook path works for a release-build headless sidecar.
- The service startup output names the token file as `.curb/api.token`, so the
  runbook should tell operators to use the emitted token path instead of a
  literal `token-file`.
- Watcher ticks reported two working sessions, zero warn/kill sessions, zero
  source errors, about 3014 usage events, and 27 processes.

## Residual Risk

- This is local proof only; hosted CI proof on a pushed branch remains open.
- The run did not exercise an actual stop decision or stop rejection. That still
  belongs in future enforcement dogfood.
