# Readiness latency and observability completion

Priority: P0
Status: ready
Estimate: M

## Goal

Finish the structured observability contract and make headless readiness fast,
diagnosable, and safe for server-side integrations.

## Context

The first NDJSON slice emits startup, usage scan, and HTTP request events.
Dogfood evidence proved the JSON shape and redaction rules, but it also exposed
a slow readiness response (`duration_ms=5020`). Headless integrations need
readiness probes that are cheap, truthful, and machine-readable.

Follow-up smoke narrowed the old behavior: `/v1/ready` returns in 0-1ms and
reports fast `degraded` when the runtime cache is busy. Headless startup now
binds before the first usage scan; readiness reports degraded until the
background scan has populated the first snapshot, then returns ready.

## Oracle

- [x] Root-cause the slow `/v1/ready` response with a reproducible local smoke
      that records startup timing, usage-scan timing, and readiness timing.
- [x] Make `/v1/ready` use only bounded, non-blocking checks or explicitly
      report `degraded` with a sanitized reason when a dependency is busy.
- [x] Emit registered NDJSON events for usage scan failures, source-health
      errors, watcher ticks, notification attempts, stop decisions, stop
      rejections, config load, and shutdown.
- [x] Decide whether headless startup should bind before the first usage scan,
      or keep current behavior and document startup scan latency expectations.
- [x] Add tests or smoke scripts that parse emitted logs as JSON and assert
      required fields for startup, health request, usage scan failure, and stop
      rejection/success paths.
- [x] Add redaction tests for source-health errors and stop reasons.
- [x] Update `docs/observability.md` with event examples and latency
      expectations.
- [x] Keep `/v1/live` public, `/v1/ready` public, and protected API routes
      token-gated.
- [x] Add a timed headless dogfood script that records initial degraded
      readiness when present, final readiness HTTP 200, repeated watcher ticks,
      and parser/redaction evidence under `evidence/dogfood/`.

## Non-Goals

- Do not add external telemetry export by default.
- Do not log prompt, response, screenshot, keystroke, file-content, API token,
      or raw provider log content.
- Do not make readiness terminate, rescan, or mutate runtime state.

## Suggested Proof

```sh
CURB_LOG_FORMAT=json target/debug/curb serve --headless --addr 127.0.0.1:0 --config configs/curb.example.yaml 2>/tmp/curb.ndjson
python3 scripts/parse-observability-smoke.py /tmp/curb.ndjson
cargo test --bin curb observability -- --nocapture
scripts/validate.sh
```
