# Deep-module refactor path for service, API, runtime, usage, and binary shell

Priority: P1
Status: ready
Estimate: L

## Goal

Refactor the broad boundary modules into deeper, simpler interfaces without
changing public behavior or weakening safety invariants.

## Context

Curb already has the right high-level shape: `curb-core` owns domain/runtime
logic, and the binary crate owns CLI/API/web shell. The first extraction pass
has reduced the original API, service, runtime, and usage pressure. The
remaining elegance work is now in the next broad boundary modules:
`curb-core/src/write_path.rs` and any residual usage policy, platform, config,
or binary-shell pressure. They are not failed modules, but they carry multiple
use cases and make future agent work harder to isolate.

Current size pressure from live `wc -l`:

- `src/api.rs`: 259 lines plus `src/api/routes.rs` at 216 lines,
  `src/api/auth.rs` at 105 lines, `src/api/wire.rs` at 138 lines,
  `src/api/response.rs` at 43 lines, `src/api/token_store.rs` at 72 lines,
  `src/api/http_types.rs` at 132 lines, `src/api/dispatch.rs` at 127 lines,
  `src/api/server.rs` at 82 lines, and `src/api/tests.rs` at 1105 lines after
  the first API route, auth, wire, response, token-store, HTTP type, dispatch,
  server-front-door, and public-behavior test-module extractions.
- `curb-core/src/service.rs`: 301 lines plus
  `curb-core/src/service/snapshot_model.rs` at 603 lines,
  `curb-core/src/service/events_model.rs` at 167 lines,
  `curb-core/src/service/config_model.rs` at 159 lines, and
  `curb-core/src/service/ack_state.rs` at 57 lines, and
  `curb-core/src/service/correlation.rs` at 188 lines, and
  `curb-core/src/service/tests.rs` at 651 lines after the first service
  snapshot/session, event/alert, config, acknowledgement-state, correlation,
  and public-behavior test-module extractions.
- `curb-core/src/runtime.rs`: 443 lines plus
  `curb-core/src/runtime/usage_tick.rs` at 60 lines,
  `curb-core/src/runtime/config_store.rs` at 46 lines,
  `curb-core/src/runtime/cache.rs` at 58 lines,
  `curb-core/src/runtime/watcher.rs` at 84 lines,
  `curb-core/src/runtime/readiness.rs` at 77 lines, and
  `curb-core/src/runtime/tests.rs` at 982 lines after the first runtime
  usage-tick, config-store, cache, watcher, readiness, and public-behavior
  test-module extractions.
- `curb-core/src/usage.rs`: 197 lines plus
  `curb-core/src/usage/cache.rs` at 229 lines and
  `curb-core/src/usage/discovery.rs` at 174 lines and
  `curb-core/src/usage/provider.rs` at 147 lines,
  `curb-core/src/usage/events.rs` at 108 lines,
  `curb-core/src/usage/lines.rs` at 40 lines, and
  `curb-core/src/usage/tests.rs` at 746 lines after the cache-state,
  persisted-cache, prefix-hash, cached-read, safe-discovery, root-validation,
  file-size guard, provider registry, provider-scan orchestration, shared event
  helper, parser line-limit, and public-behavior test-module extractions.
- `src/main.rs`: 421 lines plus `src/server_cmd.rs` at 174 lines and
  `src/usage_cli.rs` at 187 lines after the serving/watch lifecycle and
  usage/tail presentation extractions.
- `curb-core/src/config.rs`: 662 lines plus
  `curb-core/src/config/duration.rs` at 126 lines,
  `curb-core/src/config/defaults.rs` at 96 lines,
  `curb-core/src/config/storage.rs` at 56 lines,
  `curb-core/src/config/preset.rs` at 97 lines,
  `curb-core/src/config/policy_merge.rs` at 39 lines, and
  `curb-core/src/config/tests.rs` at 265 lines after the duration,
  default-agent/path, private-file-storage, preset, policy-merge, and
  public-behavior test-module extractions.
- `src/observability.rs`: 508 lines plus `src/observability/event.rs` at 85
  lines and `src/observability/registry.rs` at 96 lines after the structured
  log event/schema and registry/path/outcome helper extractions.
- `curb-core/src/platform.rs`: 232 lines plus
  `curb-core/src/platform/target.rs` at 117 lines,
  `curb-core/src/platform/capture.rs` at 75 lines and
  `curb-core/src/platform/notification.rs` at 107 lines and
  `curb-core/src/platform/termination.rs` at 142 lines and
  `curb-core/src/platform/tests.rs` at 491 lines after the sealed-target
  construction, identity comparison, supervisor escalation walk,
  process-tree scoping, notification command, notification capability,
  notification execution, process-tree termination, OS-specific termination
  command, sysinfo process conversion, liveness filtering, and
  public-behavior test-module extractions.
- `curb-core/src/usagewatch.rs`: 383 lines plus
  `curb-core/src/usagewatch/events.rs` at 92 lines and
  `curb-core/src/usagewatch/tests.rs` at 573 lines after the ledger-event
  payload, usage-message formatting, and public-behavior test-module
  extractions.
- `curb-core/src/write_path.rs`: 250 lines plus
  `curb-core/src/write_path/ack_store.rs` at 102 lines and
  `curb-core/src/write_path/ledger_events.rs` at 115 lines and
  `curb-core/src/write_path/stop_identity.rs` at 68 lines and
  `curb-core/src/write_path/tests.rs` at 360 lines after the
  write-path persistence, ledger-event projection, expected-identity
  validation, and public-behavior test-module extractions.
- `curb-core/src/ledger.rs`: 418 lines plus
  `curb-core/src/ledger/taxonomy.rs` at 338 lines after separating append-only
  NDJSON persistence/hash-chain behavior from ledger event taxonomy and
  dashboard alert/view classification.

Fresh architecture critique scored modularity at roughly 6.5/10: the top-level
direction is sound, but route dispatch/auth/UI behavior, runtime scanner/cache
ownership, service read models/actions, and usage reader cache/provider dispatch
need smaller public surfaces before future agents can safely change them.
The milestone map is now in `docs/refactor-map.md`.

## Oracle

- [x] Produce a refactor map before code changes: current responsibilities,
      proposed extracted modules, public interfaces, and behavior tests that
      preserve each boundary.
- [x] Do not begin broad extraction until `030-api-ui-contract-drift-guard.md`
      has at least snapshot/config/onboarding/live/ready fixtures in place.
- [x] Completed milestones define no-behavior-change contracts: public
      endpoints, API schemas, service read-model behavior, UI-facing state
      names, and safety invariants before and after.
- [x] Extract API routing, auth, wire decoding, response construction, token
      persistence, HTTP request/response/header containers, route dispatch,
      server front-door policy, and the API public-behavior test oracle so
      route additions do not require editing one giant surface.
- [x] Extract service read-model builders from session actions and config
      updates so UI projections remain stable while write paths stay explicit.
- [x] Extract runtime observability/watcher lifecycle from config mutation and
      snapshot cache ownership.
- [x] Extract usage reader orchestration from provider dispatch/cache
      persistence while keeping provider modules deep and metadata-only.
      Cache persistence, safe discovery, provider dispatch, shared event
      helpers, parser line handling, and the usage public-behavior test module
      are extracted.
- [x] Extract binary-shell serving/watch lifecycle and usage/tail presentation
      out of `src/main.rs` without changing CLI behavior or core policy.
- [x] Extract config duration parsing/serialization, built-in defaults,
      private storage helpers, and config behavior tests while keeping
      `Config::{load,save,validate,apply_preset}` as the public facade.
- [x] Extract config preset mechanics and agent-policy merge mechanics while
      keeping `Config::{apply_preset,refresh_agent_policies,policy_for}` as the
      public facade.
- [x] Extract binary observability event schema/sanitization and event
      registry/path-template/outcome helpers while keeping public emit functions
      and log wire shape stable.
- [x] Extract ledger event taxonomy and alert/view classification while keeping
      `Ledger::{open,open_with_options,append}` and `ledger::read` as the
      append-only persistence facade.
- [x] Extract platform notification command/capability/run mechanics and
      platform behavior tests while keeping process identity and termination
      targets sealed behind `platform.rs`.
- [x] Extract platform process-tree termination execution and OS-specific
      termination command construction while keeping `Platform::terminate`
      sealed behind `TerminationTarget`.
- [x] Extract platform sysinfo process conversion and liveness filtering while
      preserving metadata-only process capture output and sealed target
      construction in `platform.rs`.
- [x] Extract platform sealed-target construction, process identity comparison,
      supervisor escalation walk, and child-first process-tree scoping while
      preserving `Snapshot::{termination_target,supervisor_target}` as the
      public facade.
- [x] Extract usagewatch ledger-event payload/message projection and policy
      behavior tests while keeping warning/grace/termination decisions inside
      `UsageWatch`.
- [x] Extract write-path ack-file write/delete/rollback persistence while
      keeping manual acknowledge/stop orchestration and fresh termination
      identity validation inside `write_path::Service`.
- [x] Extract write-path session-ack/manual-stop ledger event projection while
      preserving event types, data payloads, reason-message handling, mode/agent
      attribution, and append ordering.
- [x] Extract write-path expected stop identity validation while preserving
      `InvalidStop` versus `StopConflict` error classes, message strings, call
      ordering, and fresh-process authority before sealed target construction.
- [x] Move write-path public-behavior tests behind
      `curb-core/src/write_path/tests.rs` while preserving the manual ack/stop
      behavior oracle.
- [x] Keep every public action behind small interfaces and prove no production
      termination function accepts a bare PID.
- [x] Completed milestones have tests that exercise public behavior, not
      internal call counts, and `scripts/validate.sh` remains green.
- [x] Completed milestones received fresh critics using only the diff, the
      refactor map, and the no-behavior-change contract before the next
      extraction began.

## Non-Goals

- Do not rewrite Curb around async, a web framework, or new architecture jargon
      before the small extractions prove value.
- Do not move provider parsing into process enforcement or UI code.
- Do not change wire formats unless paired with explicit API contract tests.

## Suggested Proof

```sh
bash scripts/check-file-length.sh
cargo test --workspace
scripts/validate.sh
```
