# Headless Observability Dogfood

Date: 2026-06-04

Purpose: run Curb as a headless sidecar against real local provider metadata for
a timed observability window, without enabling termination or writing service
state into the worktree.

Commands:

```sh
CURB_DOGFOOD_SECONDS=20 bash scripts/dogfood-headless-observability.sh evidence/dogfood/2026-06-04-headless-observability
```

Environment:

- Address: `127.0.0.1:61331`
- Config: `config.yaml`
- Mode: visibility
- Home scanned: `/Users/phaedrus`
- Scratch state: private temporary directory deleted on exit
- Ledger artifact: `ledger.ndjson`
- Structured log: `headless-observability.ndjson`

Evidence:

- `build-release.txt`: release binary build.
- `validate-config.txt`: generated scratch-state config validates.
- `usage-since-24h.txt` and `usage-since-24h.json`: provider source-health
  baseline before the headless run.
- `live.json`, `ready-initial.json`, `ready-initial.status`,
  `ready-final.json`, `ready-final.status`, `health-authenticated.json`:
  headless API probes.
- `overview-initial.json`, `overview-final.json`, `sessions-initial.json`,
  `sessions-final.json`, `events-final.json`: protected API evidence after
  the timed window.
- `parse-observability-smoke.txt`: parser accepted the NDJSON artifact and
  required runtime policy fields.
- `event-summary.json` and `event-summary.txt`: event counts from the timed
  run; the script requires at least one startup `usage_scan`, at least two
  repeated `watcher_tick` events, and final readiness HTTP 200.
- `redaction-check.txt`: token, auth header, prompt, response, screenshot, and
  keystroke terms were absent from NDJSON.

Safety notes:

- The generated config keeps `mode: visibility`, so this run cannot terminate
  processes.
- Local notifications are disabled for the dogfood config.
- Provider ingestion remains metadata-only; raw prompt/response content is not
  captured.
