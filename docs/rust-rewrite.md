# Rust Rewrite Plan

Status: active rewrite branch
Date: 2026-05-28

## Intent

Rewrite Curb in Rust without flattening the design into adapters around the Go
code. The current Go implementation is the executable oracle until Rust reaches
feature parity. The Rust implementation should preserve the launch product:
one local endpoint agent owns usage ingestion, process correlation, policy,
notifications, enforcement, and the append-only ledger; CLI and UI surfaces are
thin clients.

## Strategic Shape

The rewrite keeps deep modules and narrow interfaces:

- `src/main.rs`: CLI composition only.
- `src/cli.rs`: first-run config path discovery, user config creation, install
  copy, and small config summary/preset surfaces.
- `src/config.rs`: strict YAML schema, defaults, validation, policy merge.
- `src/ledger.rs`: append-only NDJSON event journal, metadata enrichment,
  sensitive-field redaction, hash chaining, append hooks.
- `src/platform.rs`: process identity, process-tree scope, and sealed
  `TerminationTarget` construction.
- `src/usage.rs`: provider metadata readers, durable parse cache, append-only
  tail reads, replacement detection, per-provider scan errors, and session
  summaries.
- `src/service.rs`: usage-derived snapshot/read-model vocabulary, live worker
  matching, session/process correlation, watch-only classification,
  actionability safety, session acknowledgements, and manual stop-session
  revalidation.
- `src/usagewatch.rs`: automatic usage policy state: warning dedupe,
  acknowledgement suppression, grace windows, stored termination targets,
  notifications, and usage enforcement events.
- `src/runtime.rs`: local daemon orchestration: config updates, usage scans,
  snapshot cache, notification health, and shared runtime state for API and
  background watching.
- `src/api.rs`: loopback API routes, auth, and UI cookie issuance.
- `src/web.rs`: embedded UI assets only.

## Load-Bearing Type Invariants

- Termination must never be represented as a raw PID. Rust platform adapters
  accept only a `TerminationTarget` produced by revalidating a live process
  snapshot.
- A termination target binds PID, process start time, owner, executable/app
  identity, and child-first process-tree scope.
- Visibility and alert mode must not be able to call the terminator.
- Desktop app roots remain watch-only unless configuration explicitly allows
  app-root termination.
- Prompt, response, screenshot, keystroke, and file-content capture are rejected
  by config and redacted from ledger data.

## Migration Sequence

1. Port config, ledger, and platform identity primitives with unit tests.
2. Port provider usage readers for Codex and Claude using existing log fixtures.
3. Port durable usage cache semantics from the Go oracle.
4. Port service read models and session/process correlation.
5. Port acknowledgement and manual stop-session actions.
6. Serve the existing React UI from the Rust daemon.
   Status: Rust embeds `internal/web/dist`, serves the SPA from loopback, keeps
   `/v1/*` protected, and exposes `curb app` as the browser launch path.
7. Port first-run CLI ergonomics.
   Status: Rust now supports `init`, `install`, `config`, config path discovery
   via `CURB_CONFIG`/local/user defaults, and preset updates backed by Rust
   config primitives.
8. Port warnings, notification delivery, grace policy, and automatic
   usage enforcement.
   Status: Rust now has automatic usage scan ticks and `curb serve` starts the
   watcher in-process. Remaining daemon work includes graceful shutdown and
   full CLI ergonomics.
9. Port the safe synthetic demo to use the Rust binary.
10. Remove Go only after Rust passes the behavior oracle and the product demo.

## Validation

`scripts/validate.sh` now runs Rust formatting, clippy, and tests before the Go
and UI oracle checks. Full completion requires the same safe demo guarantees:
alert mode emits `usage_would_terminate` without stopping the synthetic worker,
and enforcement mode emits `usage_termination_completed` after stopping only
that synthetic worker.
