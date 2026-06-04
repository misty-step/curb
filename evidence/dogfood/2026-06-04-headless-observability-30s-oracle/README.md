# Headless Observability Dogfood

Date: 2026-06-04

Purpose: run Curb as a headless sidecar against real local provider metadata for
a timed observability window, without enabling termination or writing service
state into the worktree.

Commands:

```sh
CURB_DOGFOOD_SECONDS=30 bash scripts/dogfood-headless-observability.sh evidence/dogfood/2026-06-04-headless-observability-30s-oracle
```

Environment:

- Address: `127.0.0.1:64092`
- Config: `config.yaml`
- Mode: visibility
- Home scanned: `<home>`
- Scratch state: private temporary directory deleted on exit
- Ledger artifact: `ledger.ndjson`
- Structured log: `headless-observability.ndjson`

Evidence:

- `build-release.txt`: release binary build.
- `validate-config.txt`: generated scratch-state config validates.
- `usage-since-24h.txt`: provider source-health aggregate baseline before the
  headless run. It intentionally omits per-session IDs and local paths.
- `live.json`, `ready-initial.json`, `ready-initial.status`,
  `ready-final.json`, `ready-final.status`, `health-authenticated.json`:
  headless API probes.
- `overview-initial.json`, `overview-final.json`, `sessions-initial-count.txt`,
  `sessions-final-count.txt`, and `events-final.json`: protected API evidence
  after the timed window. Per-session response dumps are intentionally not
  committed because they contain local project labels.
- `parse-observability-smoke.txt`: parser accepted the NDJSON artifact and
  required runtime policy fields.
- `event-summary.json` and `event-summary.txt`: event counts from the timed
  run; the script requires at least one startup `usage_scan`, duration-scaled
  repeated `watcher_tick` events, and final readiness HTTP 200.
- `redaction-check.txt`: token, auth header, prompt, response, screenshot,
  keystroke, file-content, raw-provider, and payload terms were absent from
  NDJSON.

Safety notes:

- The generated config keeps `mode: visibility`, so this run cannot terminate
  processes.
- Local notifications are disabled for the dogfood config.
- Provider ingestion remains metadata-only; raw prompt/response content is not
  captured.
