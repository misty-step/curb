# Headless Enforcement Dogfood

Date: 2026-06-04

Purpose: prove that Curb can run as a headless sidecar and successfully stop a
harmless, repo-spawned synthetic worker through the protected API while
preserving the termination safety boundary.

Commands:

```sh
bash scripts/dogfood-headless-enforcement.sh evidence/dogfood/2026-06-04-headless-enforcement
```

Evidence:

- `config.yaml`: enforcement config with one marker-gated synthetic process
  agent, private scratch state, and local notification delivery disabled.
- `build-release.txt`: release binary build.
- `live.json`, `ready.json`, `health-authenticated.json`: headless API
  probes.
- `selected-session.json`: selected stop candidate, including PID,
  start-time, owner, executable identity, alert state, and `can_stop: true`.
- `stop-request.json`: stale-state stop request sent back to the service.
- `stop-response.txt`: `HTTP/1.1 200 OK` stop response.
- `worker-exit.txt`: direct child was reaped after Curb stopped it.
- `events-after-stop.json`: protected ledger API response after the stop.
- `ledger-event-check.txt`: manual stop started/completed events were present.
- `headless-enforcement.ndjson`: structured JSON logs.
- `parse-observability-smoke.txt`: parser accepted the NDJSON artifact and
  required runtime policy fields on `usage_scan` and `watcher_tick`.
- `stop-decision-log.json`: structured `stop_decision` status 200 log.
- `redaction-check.txt`: token, marker, operator reason, auth header, prompt,
  response, screenshot, and keystroke terms were absent from NDJSON.

Safety notes:

- The only configured agent matcher requires the unique marker
  `curb-dogfood-enforcement-22959-1780541432`, so real agent processes cannot match this config.
- The usage log is synthetic metadata only, under a temporary HOME.
- State and token files are under a private temporary directory deleted on exit.
- Termination still uses the production stop path: fresh process capture,
  expected PID/start-time/owner/executable validation, sealed
  `TerminationTarget`, then platform process-tree termination.

Residual risk:

- This is local macOS evidence. Hosted CI proof and Windows-specific behavior
  remain separate gates.
