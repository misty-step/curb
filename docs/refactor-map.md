# Deep-Module Refactor Map

Date: 2026-06-03

This map is the prerequisite for behavior-preserving extraction work in
`backlog.d/028-deep-module-refactor-path.md`. It is not a rewrite plan. Curb's
current high-level shape is sound: `curb-core` owns domain/runtime behavior, the
binary crate owns CLI/API/web composition, and OS actions stay behind
`platform`. The next refactors should deepen existing boundaries, not introduce
a framework or new architectural vocabulary.

## Current Pressure

Live source size:

- `src/api.rs`: 259 lines plus `src/api/routes.rs` at 216 lines,
  `src/api/auth.rs` at 105 lines, `src/api/wire.rs` at 138 lines,
  `src/api/response.rs` at 43 lines, `src/api/token_store.rs` at 72 lines,
  `src/api/http_types.rs` at 132 lines, `src/api/dispatch.rs` at 127 lines,
  `src/api/server.rs` at 82 lines, and `src/api/tests.rs` at 1105 lines after
  the Milestone 1 route, auth, wire, response, token-store, HTTP type,
  dispatch, server-front-door, and public-behavior test-module extractions.
- `curb-core/src/service.rs`: 302 lines plus
  `curb-core/src/service/snapshot_model.rs` at 494 lines,
  `curb-core/src/service/delta.rs` at 112 lines,
  `curb-core/src/service/events_model.rs` at 167 lines,
  `curb-core/src/service/config_model.rs` at 159 lines, and
  `curb-core/src/service/ack_state.rs` at 57 lines, and
  `curb-core/src/service/correlation.rs` at 188 lines, and
  `curb-core/src/service/tests.rs` at 651 lines after the first Milestone 2
  snapshot/session, overview-delta, event/alert, config, acknowledgement-state,
  correlation, and public-behavior test-module extractions.
- `curb-core/src/runtime.rs`: 443 lines plus
  `curb-core/src/runtime/usage_tick.rs` at 60 lines,
  `curb-core/src/runtime/config_store.rs` at 46 lines,
  `curb-core/src/runtime/cache.rs` at 58 lines,
  `curb-core/src/runtime/watcher.rs` at 84 lines,
  `curb-core/src/runtime/readiness.rs` at 77 lines, and
  `curb-core/src/runtime/tests.rs` at 982 lines after the first Milestone 3
  usage-tick, config-store, cache, watcher, readiness, and public-behavior
  test-module extractions.
- `curb-core/src/usage.rs`: 197 lines plus
  `curb-core/src/usage/cache.rs` at 229 lines and
  `curb-core/src/usage/discovery.rs` at 174 lines and
  `curb-core/src/usage/provider.rs` at 147 lines,
  `curb-core/src/usage/events.rs` at 108 lines, and
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
  `curb-core/src/write_path/tests.rs` at 360 lines after the ack-file
  persistence, ledger-event projection, expected-identity validation, and
  public-behavior test-module extractions.
- `curb-core/src/ledger.rs`: 418 lines plus
  `curb-core/src/ledger/taxonomy.rs` at 338 lines after separating append-only
  NDJSON persistence/hash-chain behavior from ledger event taxonomy and
  dashboard alert/view classification.

The line counts are only a signal. The real issue is change amplification:

- `src/api.rs` is now mostly the API facade, backend adapter, and error
  taxonomy. API token file persistence now lives behind `src/api/token_store.rs`,
  HTTP request, response, and header-map containers now live behind
  `src/api/http_types.rs`, route execution now lives behind
  `src/api/dispatch.rs`, the UI/headless/CORS/auth front-door ordering now lives
  behind `src/api/server.rs`, and the public-behavior API oracle now lives
  behind `src/api/tests.rs`.
- `curb-core/src/service.rs` now acts as the service facade and read-model
  schema owner, while dedicated service modules own config projection,
  event/alert projection, acknowledgement reads, process correlation, and
  snapshot/session construction. Service public-behavior tests now live behind
  `curb-core/src/service/tests.rs`; its remaining risk is facade breadth, not
  mixed implementation mechanics.
- `curb-core/src/runtime.rs` now owns the `Runtime` facade, runtime error
  taxonomy, reader/platform/governor ownership, onboarding, notification state,
  event reads, acknowledgement, and stop. Usage-scan and tick ownership has
  moved behind
  `curb-core/src/runtime/usage_tick.rs`; config path/view/persisted update
  ownership has moved behind
  `curb-core/src/runtime/config_store.rs`; snapshot-cache mutex ownership has
  moved behind `curb-core/src/runtime/cache.rs`; readiness check assembly has
  moved behind `curb-core/src/runtime/readiness.rs`; watcher thread/shutdown
  mechanics have moved behind `curb-core/src/runtime/watcher.rs`; runtime
  public-behavior tests now live behind `curb-core/src/runtime/tests.rs`.
- `curb-core/src/usage.rs` already has provider modules and now hides
  incremental cache persistence behind `curb-core/src/usage/cache.rs` and safe
  usage-file discovery behind `curb-core/src/usage/discovery.rs`, while
  provider registration and scan orchestration live behind
  `curb-core/src/usage/provider.rs`. Shared normalized-event helpers live
  behind `curb-core/src/usage/events.rs`, and parser line-size handling lives
  behind `curb-core/src/usage/lines.rs`. Usage parser/reader public-behavior
  tests now live behind `curb-core/src/usage/tests.rs`. The remaining usage
  facade is mostly public data types, public reader methods, provider-specific
  exported parser entrypoints, and tail/file-size constants.
- `src/main.rs` is now closer to a command parser and composition shell.
  Serving, headless/app watcher lifecycle, shutdown handling, and browser-open
  behavior live behind `src/server_cmd.rs`; usage summary and tail presentation
  live behind `src/usage_cli.rs`.
- `curb-core/src/config.rs` still owns the public config schema, YAML load/save,
  defaults application, validation, and the `Config::{apply_preset,
  refresh_agent_policies, policy_for}` facade. The reusable duration
  parser/serializer, built-in default path/agent inventory, private config-file
  storage mechanics, preset mechanics, policy merging, and config behavior tests
  now live behind dedicated config submodules.
- `src/observability.rs` owns the public binary logging emit functions and
  event builders. Stable log schema serialization now lives behind
  `src/observability/event.rs`; event registration, status outcomes, and
  path-template redaction live behind `src/observability/registry.rs`.
- `curb-core/src/platform.rs` still owns the public platform data model,
  snapshots, the system platform trait implementation, and the
  `TerminationTarget` facade. Sealed target construction, process-identity
  comparison, supervisor escalation, and child-first process-tree scoping now
  live behind `curb-core/src/platform/target.rs`. Sysinfo process conversion and
  liveness filtering now live behind `curb-core/src/platform/capture.rs`.
  Notification capability/command/run mechanics now live behind
  `curb-core/src/platform/notification.rs`, termination execution now lives
  behind `curb-core/src/platform/termination.rs`, and platform public-behavior
  tests live behind `curb-core/src/platform/tests.rs`.
- `curb-core/src/usagewatch.rs` now reads as the usage policy state machine:
  warning, grace, would-stop, blocked-stop, termination, resumed-session, and
  terminated-row lifecycle. Ledger payload projection and usage-message
  formatting live behind `curb-core/src/usagewatch/events.rs`, and the
  policy public-behavior oracle lives behind `curb-core/src/usagewatch/tests.rs`.
- `curb-core/src/ledger.rs` owns append-only audit persistence, metadata
  enrichment, sensitive-field scrubbing, and hash-chain reads/writes.
  `curb-core/src/ledger/taxonomy.rs` owns the closed `LedgerEvent` wire-string
  taxonomy plus alert/view classification used by service read models.

## Existing Good Boundaries

Keep these. Refactors should make them clearer, not bypass them.

- `curb-core/src/platform.rs` owns OS facts and sealed `TerminationTarget`
  construction. Production termination must keep accepting only sealed targets,
  never a bare PID.
- `curb-core/src/write_path.rs` owns manual acknowledge and stop write paths.
  UI-provided PID, start time, owner, and executable are confirmation evidence;
  fresh process capture remains the authority. Ack-file mutation mechanics live
  behind `curb-core/src/write_path/ack_store.rs`; snapshot ack reads still live
  behind `curb-core/src/service/ack_state.rs`. Manual ack/stop ledger event
  construction lives behind `curb-core/src/write_path/ledger_events.rs`.
  Expected stop identity validation lives behind
  `curb-core/src/write_path/stop_identity.rs`.
- `curb-core/src/local_enforcer.rs` and `curb-core/src/usagewatch.rs` keep policy
  decisions separate from OS process capture and process termination.
- `curb-core/src/onboarding.rs` owns onboarding and platform capability
  projection.
- `curb-core/src/usage/{codex,claude,pi}.rs` already keep provider wire parsing
  behind provider-specific modules.
- `contracts/api/*.json`, `ui/src/contract.test.ts`, and
  `api_contract_fixtures_match_ui_facing_routes` give a no-behavior-change
  oracle for API/UI payloads.

## Milestone 1: API Endpoint Table

Goal: keep `Server::handle` as the public facade while hiding endpoint parsing,
method validation, and dispatch shape behind a small internal route table.

Proposed modules:

- `src/api/routes.rs`: parse method/path/query into route intent such as
  `Route::Snapshot`, `Route::Session { key }`, or
  `Route::SessionTurns { key, query }`.
- `src/api/wire.rs`: request-body decoders and wire DTO conversions for ack,
  stop, and config update.
- `src/api/auth.rs`: token, cookie, bearer/header auth, local-origin, CORS, and
  unsafe-cookie checks.
- `src/api/response.rs`: JSON success/error response construction and
  `ApiError` to HTTP mapping.
- `src/api/token_store.rs`: API token load/create, empty-token rejection,
  private file creation, and permission repair.
- `src/api/http_types.rs`: API `Request`, `Response`, and `HeaderMap`
  containers, including target splitting, cookie/header lookup, and response
  header merge helpers.
- `src/api/dispatch.rs`: public/protected route execution after
  authentication, including backend calls, write-body decoding, response
  mapping, and route-specific observability events.
- `src/api/server.rs`: `Server` construction, UI/headless mode flags, non-API
  UI fallback, CORS/preflight, public/protected split, authentication, and
  unsafe cookie same-origin enforcement.
- `src/api/tests.rs`: API public-behavior tests, contract fixture equality, and
  fake backend fixtures used to prove auth, routing, wire decoding, headless,
  UI fallback, and mutation-before-validation invariants.

Interface rule: routes are use-case names, not generic strings. Do not split
into one file per endpoint and do not split the `Backend` trait yet; it is the
current binary/web boundary.

Progress: `src/api/routes.rs` now owns public/protected route intent, method
classification, session path decoding, and route query parsing. `src/api/auth.rs`
owns token cookies, bearer/header/cookie authorization, local CORS headers,
same-origin checks, and unsafe-method classification. `src/api/wire.rs` owns
write-body DTOs and decoders for ack, stop, and config update.
`src/api/response.rs` owns JSON success/error response construction and
`ApiError` to HTTP mapping. `src/api/token_store.rs` owns API token load/create,
empty-token rejection, private file creation, and permission repair.
`src/api/http_types.rs` owns request construction, header/cookie lookup,
response construction, and header-map normalization while `api.rs` preserves the
public re-exports. `src/api/dispatch.rs` owns route-intent execution and keeps
endpoint additions away from the security-sensitive `Server::handle` ordering.
`src/api/server.rs` owns that front-door ordering without introducing a
middleware abstraction or changing the public `api::Server` export.
`src/api/tests.rs` keeps the behavior oracle next to the API modules while
letting `api.rs` read as the facade. The remaining Milestone 1 work is a fresh
critic gate after the full validation run.

Behavior oracle:

- API contract fixture equality for `/v1/snapshot`, `/v1/overview`,
  `/v1/sessions/:key`, `/v1/sessions/:key/turns`, `/v1/config`,
  `/v1/onboarding`, `/v1/live`, and `/v1/ready`.
- Auth, CORS, bearer/header/cookie, same-origin unsafe cookie, and protected
  route tests.
- Malformed ack/stop/config payload tests, including unknown fields and unknown
  mode rejection before backend mutation.
- `cd ui && npm run smoke` against fixture-backed dashboard routes.

## Milestone 2: Service Projection Modules

Goal: keep service read models stable while making their construction
discoverable. The exported surface should remain use-case shaped, not a bag of
generic helpers.

Proposed modules:

- `curb-core/src/service/snapshot_model.rs`: `build_snapshot*`,
  `build_sessions`, `build_session_view`, `build_overview`, sorting, and
  explanation text.
- `curb-core/src/service/delta.rs`: `annotate_overview_delta`, snapshot
  change detection, turn identity, agent identity, and source-error deltas.
- `curb-core/src/service/correlation.rs`: `process_matches`, `match_agent`,
  `correlate`, `best_session_for_match`, path-specificity helpers, and
  confidence scoring.
- `curb-core/src/service/events_model.rs`: `event_views`, `alert_views`, event
  category/severity/message projection, and alert action projection.
- `curb-core/src/service/config_model.rs`: `ConfigView`, `ConfigUpdate`,
  `config_view`, and `apply_config_update`.
- `curb-core/src/service/ack_state.rs`: acknowledgement path, read, and active
  ack lookup used by snapshot derivation. Ack mutation stays in `write_path`.

Interface rule: snapshot projection may read ack state to compute UI
actionability, but must not mutate ack files, write the ledger, capture
processes, or terminate anything.

Progress: `curb-core/src/service/config_model.rs` now owns config projection,
config update DTOs, mode parsing, positive-duration validation, and agent config
rows. `curb-core/src/service/events_model.rs` now owns event and alert
projection, ledger view classification, default event/alert messages, alert
limits, and alert-to-session action projection. `curb-core/src/service/ack_state.rs`
now owns the shared session-ack file shape, hashed path, read lookup, and active
ack filtering; ack mutation remains in `write_path`.
`curb-core/src/service/correlation.rs` now owns configured process matching,
regex scoring, cwd/provider correlation, path-specificity safety, and newest
session selection for agent rows. `curb-core/src/service/snapshot_model.rs` now
owns snapshot assembly, session aggregation, turn filtering, session/agent row
shaping, freshness windows, row explanations, and sorting.
`curb-core/src/service/delta.rs` now owns overview delta annotation and the
closed snapshot-diff calculations for new sessions, new turn spend, new alerts,
agent starts/ends, and source-error changes. `curb-core/src/service/tests.rs`
owns the service public-behavior
oracle for snapshot actionability, alert/event projection, correlation,
freshness, turn spend, and path handling. The remaining Milestone 2 work is a
fresh critic gate after full validation. The latest Pi critic approved this
delta extraction as low risk but rated future internal hygiene splits lower
than remaining UI/presenter polish.

Behavior oracle:

- Snapshot/actionability tests for watch, alert, enforce, uncorrelated,
  supervised, acknowledged, old high-usage, resumed, and terminated sessions.
- Event and alert view tests for limit/order/category/action projection.
- Project/path correlation tests, including Windows path handling and root/path
  prefix rejection.
- API fixture equality and UI contract tests.

## Milestone 3: Runtime Ownership Modules

Goal: make `Runtime` a facade over explicit service-owner components rather than
the place where every lifecycle concern accumulates.

Proposed modules:

- `curb-core/src/runtime/cache.rs`: snapshot cache, rescan, cache invalidation,
  and previous/next delta annotation.
- `curb-core/src/runtime/readiness.rs`: bounded readiness checks over config,
  ledger, usage reader directory, platform capability, notification state, and
  snapshot-cache state.
- `curb-core/src/runtime/watcher.rs`: `WatcherHandle`, shutdown condvar,
  periodic usage tick loop, and observer callback.
- `curb-core/src/runtime/config_store.rs`: config path ownership, config view,
  validation, save, and cache clearing.
- `curb-core/src/runtime/usage_tick.rs`: usage scan, governor/enforcer
  interaction, ledger append, notification state, and scan-result projection.

Interface rule: the public `Runtime<P>` methods should stay stable. Extracted
runtime modules should hide mutex/condvar/cache mechanics instead of exposing
them to API, CLI, or UI callers.

Progress: `curb-core/src/runtime/readiness.rs` now owns readiness check
construction for config, ledger, usage-reader state, platform capabilities,
notifications, and snapshot-cache state. `Runtime::readiness()` remains the
public facade and provides only facts: config, notification result, termination
capability, and a small cache-status enum. `curb-core/src/runtime/watcher.rs`
now owns `WatcherHandle`, shutdown condvar, the periodic tick thread, observer
callback delivery, and scan-failure stderr reporting.
`curb-core/src/runtime/cache.rs` now owns the snapshot cache mutex, cached
snapshot reads, refresh with previous/next delta annotation, invalidation, and
readiness cache-status projection. `curb-core/src/runtime/config_store.rs` now
owns config path, config view projection, persisted config updates, validation,
and in-memory config replacement. `curb-core/src/runtime/usage_tick.rs` now owns
usage scan orchestration, usage lookback math, process capture, policy-session
building, governor/enforcer interaction, scan-failure ledger recording, and
fallback rescan projection. `curb-core/src/runtime/tests.rs` owns the runtime
public-behavior oracle for snapshot caching, readiness, onboarding,
notification, config update, acknowledgement, stop, usage scan, and watcher
behavior. Remaining Milestone 3 work is a fresh critic gate after full
validation.

Behavior oracle:

- `snapshot_uses_cache_until_explicit_rescan`.
- `update_config_persists_validated_config_and_clears_snapshot_cache`.
- `readiness_reports_degraded_until_initial_snapshot_exists`.
- `readiness_reports_busy_runtime_without_blocking_on_cache`.
- `usage_watcher_handle_shuts_down_without_waiting_for_scan_interval`.
- Usage scan tests for alert mode, enforcement grace, ack suppression, PID reuse,
  and source-health failures.
- Live headless smoke: `/v1/live`, `/v1/ready`, protected route auth, and
  registered NDJSON `usage_scan`/`shutdown` events.

## Milestone 4: Usage Reader Cache And Discovery

Goal: keep `Reader::scan_since` as the simple public interface while hiding the
mechanics of safe file discovery, cache persistence, and tail reads.

Proposed modules:

- `curb-core/src/usage/cache.rs`: `ReaderState`, `CachedFile`,
  `PersistedReaderState`, cache load/save, prefix hash validation, append-only
  replacement checks, and provider-neutral `provider_state`.
- `curb-core/src/usage/discovery.rs`: provider root discovery, one-level versus
  recursive JSONL collection, canonical root checks, symlink rejection, and
  usage-file size checks.
- `curb-core/src/usage/provider.rs`: `Provider`, `ProviderRoot`, `Layout`,
  provider registry, `scan_provider`, and source-health reporting.
- `curb-core/src/usage/events.rs`: dedupe, sorting, since filtering, and
  normalized `Event` helpers.

Interface rule: provider modules own parser wire structs and metadata-only
conversion. Shared usage code owns orchestration and cache safety. Do not move
provider parsing into process enforcement, runtime, service, API, or UI.

Progress: `curb-core/src/usage/cache.rs` now owns `ReaderState`, `CachedFile`,
persisted cache load/save, prefix-hash append validation, missing-file pruning,
provider-neutral cached state, and the cached read transaction.
`curb-core/src/usage/discovery.rs` now owns modified-time filtering, one-level
and recursive JSONL discovery, canonical-root checks, symlink rejection, and
full usage-file size guards. `curb-core/src/usage/provider.rs` now owns provider
registration, provider root layout descriptors, provider scan orchestration,
source-health isolation, cache pruning calls, and provider report construction.
`curb-core/src/usage/events.rs` now owns dedupe keys, event deduplication,
timestamp sorting, since filtering, user-input boundary event construction, and
timestamp parsing. `curb-core/src/usage/lines.rs` now owns bounded JSONL line
reads and oversized-line errors.
`curb-core/src/usage/tests.rs` owns the usage public-behavior oracle for
metadata-only parser extraction, cache safety, provider failure isolation,
symlink/root escape rejection, line/file size limits, live Codex tail reads, and
provider state persistence. `Reader` remains the public facade, and provider
modules still own parser wire structs and metadata-only event conversion.
Remaining Milestone 4 work is a fresh critic/gate pass and deciding whether
public provider-specific parser entrypoints should stay colocated with `Reader`.

Behavior oracle:

- Provider parser tests prove token extraction without prompt or response
  content.
- Reader tests prove provider failure isolation, cache hydration, append reads,
  same-path replacement rejection, symlink/root escape rejection, line/file size
  limits, live Codex tail reads, and provider state persistence.
- `scripts/validate.sh` remains green.

## Milestone 5: Binary Shell Composition

Goal: keep the binary crate as a thin consumer of `curb-core` by moving
command-specific lifecycle and presentation mechanics behind use-case modules.

Modules:

- `src/main.rs`: CLI command declaration, advanced-help text, argument
  normalization, and top-level dispatch.
- `src/server_cmd.rs`: `watch`, `serve`, and `app` lifecycle: loopback bind
  validation, API token setup, runtime construction, headless/UI selection,
  shutdown handling, initial scan, observed watcher, and platform browser open.
- `src/usage_cli.rs`: `usage` and `tail` presentation: provider usage report
  shaping, session summaries, duration labels, and tail loop output.

Interface rule: command modules may compose existing `curb-core`, API, HTTP,
and observability boundaries, but must not introduce new policy decisions, wire
schemas, provider parsers, or process termination paths.

Progress: `src/server_cmd.rs` now owns serve/app/watch lifecycle mechanics and
`src/usage_cli.rs` owns usage/tail output shaping. `src/main.rs` remains the
public command table plus dispatch. This is intentionally a binary-shell split,
not a rewrite of CLI parsing or runtime behavior.

Behavior oracle:

- CLI tests for `serve`/`daemon`/`api` aliases, headless help, non-loopback
  rejection, `watch --once`, `usage`, and `tail --once`.
- Full `scripts/validate.sh` remains green.

## Milestone 6: Config Schema Facade

Goal: keep `Config::load`, `Config::save`, `Config::validate`, and
`Config::apply_preset` as the public schema facade while moving reusable
mechanics and default inventories out of the facade.

Modules:

- `curb-core/src/config/duration.rs`: `HumanDuration`, CLI duration parsing,
  YAML serialization/deserialization, and duration formatting.
- `curb-core/src/config/defaults.rs`: default home/state directory discovery
  and built-in process-agent inventory.
- `curb-core/src/config/storage.rs`: private config-file writes and Unix
  directory/file permission repair.
- `curb-core/src/config/preset.rs`: preset mutation mechanics for the stable
  `Config::apply_preset` facade.
- `curb-core/src/config/policy_merge.rs`: agent policy override and refresh
  mechanics for the stable `Config::{policy_for,refresh_agent_policies}`
  facade.
- `curb-core/src/config/tests.rs`: config public-behavior tests for defaults,
  save/load round trips, preset behavior, validation failures, and duration
  parsing.

Interface rule: config submodules may help construct and persist configuration,
but runtime policy decisions still belong in service/runtime/usagewatch; process
identity and termination safety still belong behind `platform`.

Progress: duration, defaults, storage, preset mechanics, policy merging, and
tests are extracted. The remaining config facade is mostly public schema, YAML
load/save, defaults application, validation, and the stable policy/preset
facade methods.

Behavior oracle:

- Config tests for example load defaults, private local defaults, YAML
  round-trip, save-before-replace validation, prompt capture rejection,
  unknown egress field rejection, duplicate ids, desktop-app watch-only
  defaults, supervised worker escalation, and composite duration parsing.
- `scripts/validate.sh` remains green.

## Milestone 7: Platform Notification And Test Boundary

Goal: keep `Platform`, `SystemPlatform`, `Snapshot`, and sealed
`TerminationTarget` as the public platform facade while moving notification
mechanics and the large behavior oracle out of the production surface.

Modules:

- `curb-core/src/platform/notification.rs`: notification capability detection,
  OS-specific notification command construction, command execution, PATH lookup,
  and AppleScript escaping.
- `curb-core/src/platform/capture.rs`: sysinfo process conversion, live-status
  filtering, PID conversion, path normalization, command-line projection, and
  timestamp conversion. It may project metadata needed for process identity; it
  must not capture prompt, response, screenshot, keystroke, or file content.
- `curb-core/src/platform/termination.rs`: process-tree termination execution,
  soft/hard termination sequencing, PID liveness checks, and OS-specific
  termination command construction.
- `curb-core/src/platform/tests.rs`: platform behavior tests for termination
  identity sealing, child-first termination scope, live capture/termination,
  notification command safety, liveness filtering, platform command paths, and
  identity-seal properties.

Interface rule: notification code may build and run notification commands only.
Termination code may execute only process trees from a sealed
`TerminationTarget`; it must not construct targets, inspect process identity, or
make policy decisions. Production termination remains sealed behind
`TerminationTarget` built from pid plus start time, owner, and executable/app
identity.

Progress: notification, capture, termination, and platform tests are extracted.
The remaining platform facade is mostly public data types, process snapshots,
sealed target construction, identity predicates, and SystemPlatform
composition.

Behavior oracle:

- Platform tests for termination identity requirements, PID reuse rejection,
  child-first scope, live process capture/termination, notification capability,
  notification command argument boundaries, process liveness, absolute
  termination command paths, and property-tested identity seals.
- `scripts/check-termination-boundary.sh` proves production termination remains
  sealed behind `TerminationTarget` and OS kill/taskkill commands stay isolated
  to `platform/termination.rs`.
- `scripts/validate.sh` remains green.

## Milestone 8: Usage Policy Event Projection

Goal: keep `UsageWatch::scan` as the policy state-machine facade while moving
ledger payload/message projection and the large behavior oracle out of the
production surface.

Modules:

- `curb-core/src/usagewatch/events.rs`: usage-warning message formatting,
  compact token/id rendering, and metadata-only ledger data projection for
  warnings, blocked stops, would-stop decisions, grace, and termination results.
- `curb-core/src/usagewatch/tests.rs`: usage policy behavior tests for warning,
  watch mode, blocked uncorrelated/supervised stops, grace, termination,
  PID-reuse rejection, supervised escalation, resumed sessions, and killed-row
  aging.

Interface rule: event projection may serialize policy facts but must not decide
whether to warn, block, start grace, terminate, or suppress. Those decisions stay
inside `UsageWatch`.

Progress: event projection and tests are extracted. The remaining usagewatch
facade is mostly public policy data types, the `Enforcer` abstraction, the
state-machine maps, and scan/evaluate/retention logic.

Behavior oracle:

- Usagewatch tests for auto-kill after grace, watch mode never terminating,
  supervised worker blocking/escalation, killed-session suppression/resume/age
  out, uncorrelated block, alert-mode would-stop, and PID-reuse rejection.
- `scripts/validate.sh` remains green.

## Milestone 9: Manual Write-Path Persistence, Projection, And Identity

Goal: keep `write_path::Service` as the manual acknowledge/stop orchestrator
while hiding session-ack file mutation, audit-ledger event projection, and
expected stop identity validation behind small focused modules.

Modules:

- `curb-core/src/write_path/ack_store.rs`: ack-file write, idempotent delete,
  and rollback-to-previous-ack mechanics, including hashed ack path use, JSON
  serialization, and Unix directory/file permissions.
- `curb-core/src/write_path/ledger_events.rs`: session-ack and manual-stop
  ledger `Event` construction, including data payloads, mode, agent id, reason
  message handling, and manual stop result metadata.
- `curb-core/src/write_path/stop_identity.rs`: request-shape validation for
  UI-provided expected identity and fresh-process comparison before sealed
  termination-target construction.

Interface rule: ack persistence may mutate ack files and restore prior ack
state after ledger failure. Ledger projection may serialize write-path facts to
closed `LedgerEvent` variants. Expected-identity validation may compare
UI-provided confirmation evidence against a freshly captured process. None of
these modules may decide session actionability, build UI read models, capture
process snapshots, or construct termination targets.

Progress: ack persistence, ledger event projection, expected-identity
validation, and public-behavior tests are extracted. The remaining write-path
facade still owns manual ack/stop orchestration, duration clamping, ledger
append ordering, fresh process capture, correlation checks, sealed
termination-target construction, and platform termination.

Behavior oracle:

- Write-path tests prove ack persistence suppresses actionability, acknowledged
  sessions cannot be manually stopped, process-capture failure does not
  terminate or append stop ledger events, stale expected identity blocks
  termination, watch-only/uncorrelated/alert-mode sessions cannot be stopped,
  and correlated enforcement stops terminate the sealed process tree.
- Runtime tests prove ack-file rollback restores the previous state when the
  session-ack ledger append fails.
- Write-path and runtime tests prove `session_ack_received`,
  `manual_stop_started`, and `manual_stop_completed` event order and result
  metadata remain stable.
- Write-path and runtime tests prove stale expected identity blocks termination
  and fresh usage/process identity is revalidated before manual stop.
- `scripts/validate.sh` remains green.

## Milestone 10: Ledger Persistence and Taxonomy

Goal: keep `Ledger::{open,open_with_options,append}` and `ledger::read` as the
append-only audit persistence facade while moving event taxonomy and dashboard
classification out of the persistence module.

Modules:

- `curb-core/src/ledger.rs`: event record shape, append-only NDJSON writes,
  hash-chain state, metadata enrichment, sensitive-field scrubbing, readback,
  and persistence tests.
- `curb-core/src/ledger/taxonomy.rs`: closed `LedgerEvent` wire-string
  taxonomy, `as_str`/`parse`, event-view classification, alert classification,
  and taxonomy tests.

Interface rule: taxonomy may classify ledger event names, but it must not open
files, append records, hash records, scrub payloads, or decide runtime policy.

Progress: taxonomy and classification tests are extracted. Persistence remains
behind the unchanged `Ledger` facade.

Behavior oracle:

- Ledger tests prove hash-chain continuation, metadata enrichment, sensitive
  field redaction, after-append hook ordering, and taxonomy wire compatibility.
- Service tests prove event/alert read models still classify ledger events.
- Usagewatch/write-path tests prove emitted `LedgerEvent` variants still drive
  policy and manual-stop ledger flows without changing safety behavior.

## Milestone Gates

Each milestone must satisfy all of these before the next extraction begins:

1. No public wire shape changes unless fixtures are intentionally updated.
2. No production termination API accepts a bare PID.
3. No prompt, response, screenshot, keystroke, or file-content capture is added.
4. Tests assert public behavior, not internal call counts.
5. `scripts/check-fast.sh` passes for the milestone.
6. `scripts/validate.sh` passes before the milestone is claimed complete.
7. A fresh critic receives only the diff, this map, and the relevant backlog
   oracle and returns no blocking gap.

## Explicit Non-Extractions

- Do not introduce async or a web framework to make the refactor look cleaner.
- Do not split into one module per route, field, event, or helper.
- Do not split `Backend` until the route/wire/auth extraction proves the real
  pain is the trait surface rather than route mechanics.
- Do not move provider parser details upward into runtime, service, API, or UI.
- Do not move ack mutation into snapshot projection.
- Do not move config persistence into HTTP route code.
- Do not create semantic wrappers around `Runtime` that only forward methods.
