# ADR 0002: Structured Observability Contract

Status: accepted

Date: 2026-06-04

## Context

Headless and dogfood runs need machine-readable evidence for startup, requests,
readiness, usage scans, source-health errors, notification attempts, stop
decisions, stop rejections, and shutdown. The append-only ledger remains the
product audit record; process logs are operator diagnostics.

Operators and agents also need strong privacy guarantees. Curb must not log API
tokens, prompt text, response text, screenshots, keystrokes, file contents, raw
provider log payloads, or sensitive config values.

## Decision

`CURB_LOG_FORMAT=json` emits schema-versioned NDJSON to stderr. Schema v1 uses
stable top-level fields:

- `schema_version`
- `timestamp`
- `level`
- `component`
- `event`
- `outcome`
- optional `duration_ms`, `request_id`, `session_key`, `reason`, and `fields`

All emitted events must be registered in `src/observability.rs`. The registry
is a code-level governance point, not just documentation. Backwards-compatible
changes may add optional fields or event names. Removing required fields,
renaming fields, or changing event meaning requires a new schema version and
fixture/parser updates.

Readiness logs and `/v1/ready` should prefer bounded degraded responses over
blocking. Slow readiness is an operator signal, not something to hide behind a
generic success response.

## Consequences

Agents can parse Curb logs without scraping prose or terminal UI. Dogfood
evidence can include exact NDJSON artifacts plus parser output. Observability
remains local by default; adding remote export is a separate product decision
and must not move enforcement authority away from the endpoint.

## Verification

```sh
CURB_LOG_FORMAT=json cargo run -- watch --once --config configs/curb.example.yaml 2>/tmp/curb.ndjson
python3 scripts/parse-observability-smoke.py /tmp/curb.ndjson
cargo test --bin curb observability -- --nocapture
scripts/validate.sh
```

The observability tests prove that:

- emitted events are registered;
- request logs redact query strings and session keys;
- source-health errors are sanitized;
- stop and notification events do not expose operator-supplied reasons;
- runtime scan events include counts, durations, and sanitized failures.

## Related

- [Observability](../observability.md)
- [Observability Dogfood Runbook](../runbooks/observability-dogfood.md)
- `backlog.d/026-structured-observability.md`
- `backlog.d/032-readiness-latency-and-observability-completion.md`
