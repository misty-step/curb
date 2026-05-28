# Rust Rewrite Plan

Status: Rust-primary cutover branch
Date: 2026-05-28

## Intent

Rewrite Curb in Rust without flattening the design into adapters around the Go
code. Rust is now the primary product path; the Go implementation remains
available only as an explicit legacy oracle until it is safe to delete. The Rust
implementation preserves the launch product: one local endpoint agent owns usage
ingestion, process correlation, policy, notifications, enforcement, and the
append-only ledger; CLI and UI surfaces are thin clients.

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
- `src/dashboard.rs`: terminal rendering for the snapshot read model.
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
8. Port terminal visibility surfaces.
   Status: Rust now supports `dashboard`/`dash` text and JSON output backed by
   the same `Snapshot` read model used by the API. Rust also supports `doctor`
   for config, state directory, ledger, process snapshot, and notification
   capability checks. Rust also supports `tail` for streaming recent provider
   usage events from the metadata readers. Rust now supports `status`, `runs`
   (alias `sessions`), and `ack` over usage session keys rather than legacy
   run-ledger ids.
9. Port warnings, notification delivery, grace policy, and automatic
   usage enforcement.
   Status: Rust now has automatic usage scan ticks, `curb watch` runs policy
   scans, and `curb serve`/`curb app` start the watcher in-process while serving
   the loopback API and dashboard.
10. Port the safe synthetic demo to use the Rust binary.
    Status: `demo/006/script/run-backlog-006-demo.sh` now builds the Rust
    binary, passes isolated `--home`/`--config` paths to every usage-scanning
    command, and verifies alert/enforcement outcomes through Rust ledger events.
11. Remove Go after the Rust product surface, package artifacts, and live demo
    no longer need the legacy oracle.
    Status: the default gate and docs are Rust-primary. Go checks live behind
    `scripts/validate-go-oracle.sh` while deletion remains unsafe.

## Validation

`scripts/validate.sh` is Rust-primary. It checks the committed embedded UI
assets, Rust formatting, clippy, Rust tests, the safe synthetic demo dry-run,
and UI typecheck/lint/test. The legacy Go oracle is deliberate and separate:
run `scripts/validate-go-oracle.sh` when comparing migration behavior.

Full completion requires the same safe demo guarantees: alert mode emits
`usage_would_terminate` without stopping the synthetic worker, and enforcement
mode emits `usage_termination_completed` after stopping only that synthetic
worker.
