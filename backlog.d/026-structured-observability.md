# Structured observability for headless and dogfood runs

Priority: P0
Status: ready
Estimate: M

## Goal

Make Curb observable as a local service: structured JSON logs, request/runtime
events, timing evidence, and machine-readable health for agent operators.

## Context

The current runtime has useful ledger events and health endpoints, but runtime
diagnostics still rely on ad hoc `println!`/`eprintln!` output and broad API
success/failure checks. Headless deployments need logs and health signals that
agents and humans can parse without reading source code.

## Oracle

- [x] Define a versioned observability schema document before implementation.
      It must name the schema version, required fields, optional fields, field
      types, allowed cardinality, redaction rules, and backwards-compatible
      change policy.
- [x] Define a canonical event registry for at least startup, config load, API
      request, health check, readiness check, usage scan, watcher tick,
      notification attempt, stop decision, stop rejection, source-health error,
      and shutdown.
- [x] Add a structured logging layer for startup, config load, API requests,
      usage scans, watcher ticks, notification attempts, stop decisions, and
      runtime errors.
- [x] Support a documented machine-readable format, preferably NDJSON, with
      stable fields: timestamp, level, component, event, outcome, duration_ms
      when applicable, session_key when applicable, and sanitized reason.
- [x] API request logs include method, path template, status, duration, and a
      request id without logging tokens, prompts, responses, file contents, or
      sensitive config values.
- [x] Usage scan and watcher logs expose source-health errors, scan duration,
      event counts, process count, stop decisions, and grace/revalidation
      outcomes.
- [x] Add tests or smoke scripts that parse emitted logs as JSON and assert the
      required fields for at least startup, health request, usage scan failure,
      and stop rejection/success paths.
- [x] Tests fail if an emitted event is missing from the registry, emits fields
      outside the schema without an explicit extension point, or logs sensitive
      content.
- [x] Document how to run dogfood/headless sessions with JSON logs and where to
      attach the resulting artifact.
- [x] Add `scripts/dogfood-headless-observability.sh` so a timed headless
      visibility-mode dogfood run validates real provider metadata, repeated
      watcher ticks, final readiness, parser acceptance, and log redaction.

## Non-Goals

- Do not replace the append-only ledger with process logs.
- Do not add external telemetry export by default.
- Do not log prompt, response, screenshot, keystroke, or file-content data.

## Suggested Proof

```sh
CURB_LOG_FORMAT=json cargo run -- watch --once --config configs/curb.example.yaml 2> /tmp/curb.ndjson
python - <<'PY'
import json
for line in open('/tmp/curb.ndjson'):
    if line.strip():
        obj = json.loads(line)
        assert {'schema_version', 'timestamp', 'level', 'component', 'event'} <= set(obj)
print('ok')
PY
scripts/validate.sh
```
