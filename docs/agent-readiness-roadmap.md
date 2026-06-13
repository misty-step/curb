# Agent Readiness Roadmap

Date: 2026-06-12

This roadmap captures the current readiness posture after the first dogfood,
headless, and structured-observability slices. It is evidence for backlog
ordering, not a replacement for the pre-merge gate.

## Current Scorecard

| Pillar | Level | Evidence | Main gap |
|---|---:|---|---|
| Style and validation | L4 candidate | `.editorconfig`, `cargo fmt`, `cargo clippy -D warnings`, ESLint, TypeScript strict mode, `scripts/check-fast.sh`, `scripts/check-product-principles.sh`, `scripts/validate.sh`, repo-managed pre-commit hook installation, and hosted PR CI all run the gate ladder. | Keep watching for gate runtime and flakes on later branches. |
| Build and CI | L4 candidate | `.github/workflows/ci.yml` has a named fast-feedback Ubuntu lane, full Linux/macOS validation, Windows smoke, dependency audit, and macOS coverage with an 84% Rust line floor. Node-backed official actions are pinned to Node 24-compatible majors (`checkout@v5`, `setup-node@v6`, `upload-artifact@v6`). Latest verified behavior-bearing `master` run `27470901295` passed every job after PR #9 was squash-merged. | Keep watching hosted runtime warnings from third-party cache actions. |
| Testing | L4 candidate | Rust unit/integration tests, instrumented real-process E2E tests, CLI tests, UI Vitest tests, API/UI contract fixtures, mandatory deterministic dashboard browser smoke, demo dry-run, hosted Windows smoke, and coverage exist. | Coverage is now above the floor but still has weak files worth targeting in future hardening. |
| Documentation | L4 candidate | `AGENTS.md`, `README.md`, `docs/product-principles.md`, `docs/contributor-guide.md`, `docs/dogfooding.md`, `docs/release-evidence.md`, `docs/observability.md`, `docs/refactor-map.md`, `docs/adr/`, `docs/runbooks/`, and `.harness-kit/agent-readiness.yaml` describe product doctrine, workflows, decisions, runbooks, readiness contracts, and canonical release proof. | Keep the release evidence index current as new proof packets land. |
| Dev environment | L3 | `.editorconfig`, `.node-version`, `rust-toolchain.toml`, `Cargo.lock`, `ui/package-lock.json`, `scripts/check-setup.sh`, `scripts/install-git-hooks.sh`, and local scripts pin the basics. | No devcontainer. |
| Code quality and architecture | L3 | `curb-core` owns policy/runtime, the binary owns CLI/API/web, termination safety is behind platform targets, and API/service/runtime/usage/config/platform/usagewatch/ledger/binary-shell/observability facades plus write-path persistence/projection/identity validation and overview-delta projection have been split into deep use-case modules. | Remaining pressure is residual presenter/UI surfaces and any final facade simplification after hosted CI proof. |
| Observability | L3 | `CURB_LOG_FORMAT=json` emits versioned NDJSON for startup, requests, readiness, source-health, usage scans, watcher ticks, policy outcome counts, notifications, stop decisions, and shutdown; `/v1/live` and `/v1/ready` exist; active-session, timed headless-observability, stop-rejection, successful headless-enforcement, two-hour long sidecar, and live browser QA dogfood produced parsed NDJSON. | Long dogfood found operator-visible source-health failures and transient `watcher_runtime: cache busy` readiness degradation while live/health probes stayed available. |
| Security and governance | L4 candidate | Strict config validation rejects prompt capture; token files are private; CI has coverage, validation, dependency audit, `SECURITY.md`, `CODEOWNERS`, and mandatory offline secret scan. Hosted run `27470901295` passed dependency audit and coverage. | Keep review and merge ownership explicit on release branches. |

Overall: **L3 Standardized with several L4 candidates. Hosted CI proof is green
on `master`, the two-hour sidecar dogfood removed the "no long run" blocker,
the live browser QA packet covers the operator dashboard path, and
`docs/release-evidence.md` is the current proof index. Remaining L4 blockers are
operator-facing recovery for long-run readiness/source-health failures, review
ownership, and remaining deep-module polish.**

## Evidence Snapshot

- Current release evidence index: `docs/release-evidence.md` is the canonical
  map for current proof packets, historical retained evidence, and cleanup
  disposition.
- Full local gate: `scripts/validate.sh` passed after the headless,
  observability, UI stop-confirmation, and dogfood-oracle hardening slices. The
  June 4, 2026 run covered `scripts/check-fast.sh`, the desktop shell check, and
  the demo 006 dry-run.
- Hosted CI proof: draft PR #1
  `https://github.com/misty-step/curb/pull/1` at head
  `2da127dc119e68aed2078b0cd39e0695900e34d7` passed GitHub Actions run
  `https://github.com/misty-step/curb/actions/runs/26931762206` on
  June 4, 2026:
  `fast feedback (ubuntu)`
  `https://github.com/misty-step/curb/actions/runs/26931762206/job/79452737951`,
  `full validate (ubuntu-latest)`
  `https://github.com/misty-step/curb/actions/runs/26931762206/job/79452737949`,
  `full validate (macos-latest)`
  `https://github.com/misty-step/curb/actions/runs/26931762206/job/79452737960`,
  `windows smoke`
  `https://github.com/misty-step/curb/actions/runs/26931762206/job/79452737944`,
  `dependency audit`
  `https://github.com/misty-step/curb/actions/runs/26931762206/job/79452738000`,
  and `coverage`
  `https://github.com/misty-step/curb/actions/runs/26931762206/job/79452737946`.
- Hosted failure root cause preserved: initial run
  `https://github.com/misty-step/curb/actions/runs/26931405010` failed because
  CI installed UI packages but not the Playwright Chromium binary used by
  `ui/scripts/smoke-dashboard.mjs`; coverage also reported 82.78% against the
  84% floor. The follow-up fix installed Chromium in hosted UI gate jobs and
  added behavior tests for API backend adapters without lowering thresholds.
- Latest behavior-bearing `master` proof: PR #9 squash-merged as
  `ed7e2fb83cfbd52d54358e1bfed27df908e4a334`. GitHub Actions run
  `https://github.com/misty-step/curb/actions/runs/27470901295` passed
  `fast feedback (ubuntu)`, `full validate (ubuntu-latest)`,
  `full validate (macos-latest)`, `windows smoke`, `dependency audit`, and
  `coverage` on June 13, 2026. The same run did not emit the old Node.js 20
  action-runtime warning; unrelated Node `punycode` deprecation warnings remain
  from the cache action path.
- Red hosted runs are preserved as context, not current breakage: older master
  runs `27037199553`, `26960092690`, and `26838533371` failed before the
  current readiness tranche repairs; the current shipped `master` proof is
  green.
- Local coverage proof: `cargo llvm-cov --workspace --summary-only
  --fail-under-lines 84` passed on June 4, 2026 with TOTAL line coverage
  84.60%; `src/api.rs` rose to 94.78% line coverage.
- Local pre-commit feedback: `scripts/install-git-hooks.sh` installs the
  versioned `scripts/git-hooks/pre-commit` template into the current checkout,
  and `scripts/check-setup.sh` syntax-checks both hook scripts.
- Rendered dashboard smoke: `cd ui && npm run smoke` is now mandatory through
  `scripts/check-fast.sh`, which means it also runs in `scripts/validate.sh`
  and the GitHub `fast feedback` job. The deterministic smoke covers desktop
  and narrow viewports, opens a stoppable row, verifies the `Stop requires`
  PID/start-time/owner/executable checklist and `Stop now` affordance, and
  asserts the action strip, stop checklist, row actions, readiness panel, and
  drawer stay inside the viewport.
- Secret scan: `python3 scripts/check-secrets.py` is now mandatory through
  `scripts/check-fast.sh`, checking tracked and untracked non-ignored text
  files for high-confidence secret material.
- Dependency audit: `.github/workflows/ci.yml` defines a dedicated
  `dependency audit` job running `scripts/check-dependency-audit.sh --online`
  for RustSec and npm advisory checks. Local `--offline` mode covers cached
  RustSec without making `scripts/validate.sh` registry-dependent.
- Local advisory audit proof: on June 4, 2026,
  `scripts/check-dependency-audit.sh --offline` passed against 1105 cached
  RustSec advisories and 187 Rust crate dependencies, and
  `bash scripts/check-dependency-audit.sh --online` passed with fresh RustSec
  data, crates.io index update, and npm audit.
- ADR/runbook trail: `docs/adr/` records accepted decisions for headless service
  semantics, structured observability, and termination-boundary safety, while
  `docs/runbooks/` gives copy-paste sidecar and observability dogfood paths.
- File-size pressure after extraction passes: `write_path.rs` 250 LOC plus
  `write_path/ack_store.rs` 102 LOC, `write_path/ledger_events.rs` 115 LOC,
  `write_path/stop_identity.rs` 68 LOC, and `write_path/tests.rs` 360 LOC,
  `config.rs` 662 LOC plus `config/preset.rs` 97 LOC and
  `config/policy_merge.rs` 39 LOC, `platform.rs` 232 LOC plus
  `platform/target.rs` 117 LOC,
  and `usagewatch.rs` 383 LOC,
  `src/main.rs` 421 LOC, `src/observability.rs` 508 LOC plus
  `observability/event.rs` 85 LOC and `observability/registry.rs` 96 LOC, and
  `ledger.rs` 418 LOC plus `ledger/taxonomy.rs` 338 LOC, and
  `service/snapshot_model.rs` 494 LOC plus `service/delta.rs` 112 LOC, and the
  original API/service/runtime/usage/config/observability/ledger facades are
  now smaller use-case front doors.
- CI: named `fast feedback (ubuntu)`, full Linux/macOS validation, focused
  Windows smoke, and macOS Rust coverage are defined locally.
- UI typing: `ui/tsconfig.json` uses `strict: true`.
- Observability smoke: `/tmp/curb-observability-clean.ndjson` parsed as pure
  NDJSON with `usage_scan`, `server_started`, `api_request`,
  `readiness_check`, and `health_check`.
- Readiness latency follow-up: backlog 032 records the root-cause smoke and
  bounded-readiness fix as complete; hosted CI and fresh dogfood should keep
  watching the probe timings.
- Active-session dogfood: `evidence/dogfood/2026-06-03-active-agent/` captured
  non-zero Codex, Claude, and Pi provider events plus parsed headless NDJSON.
  It also found and fixed a `curb usage` default-home discovery bug.
- Runbook dogfood: `evidence/dogfood/2026-06-04-runbook-headless/` verified the
  release-build headless sidecar runbook with public live/ready probes,
  headless root behavior, protected health auth, parsed NDJSON, and redaction
  checks.
- Stop-rejection dogfood: `evidence/dogfood/2026-06-04-stop-rejection/` verified
  a safe non-enforcement stop request returning `409 Conflict`, emitting
  `stop_rejection`, templating the session route, and avoiding token, reason,
  or raw session-key leakage.
- Headless enforcement dogfood:
  `evidence/dogfood/2026-06-04-headless-enforcement/` verified a release-build
  headless server stopping a uniquely marked synthetic worker through the
  protected stop API. The evidence includes public/protected probes, selected
  PID/start-time/owner/executable identity, `HTTP/1.1 200 OK`, raw
  `manual_stop_started` and `manual_stop_completed` ledger entries,
  `stop_decision` status 200 in NDJSON, worker reaping, required runtime policy
  fields on `usage_scan`/`watcher_tick`, and a redaction check.
- Destructive-action UI proof: the in-app Browser verified the selected
  stoppable dashboard row exposes the `Stop requires` checklist with PID,
  process start time, owner, executable, and `Stop now` with no console
  warnings/errors and no desktop overflow across the action surfaces; `cd ui &&
  npm run smoke` refreshes desktop/narrow screenshots and repeats the same
  checklist and overflow assertions under `ui/artifacts/smoke-dashboard/`.
- Two-step stop confirmation: the dashboard now arms destructive stops before
  posting to the protected API. `ui/src/components/sessionActions.tsx` owns the
  stop/ack action strip and inline `Confirm stop` state, `ui/src/App.test.tsx`
  proves no `/stop` POST occurs on the first click, and the deterministic
  dashboard smoke checks `Stop now`, `Confirm stop`, `Cancel`, and narrow/desktop
  overflow for the action strip.
- Live browser QA proof: `evidence/dogfood/2026-06-12-live-dashboard-qa/`
  verified first launch/recovery, active row selection, readiness, ack,
  settings save/revert, notification test, stale stop rejection, confirmed
  synthetic stop, API failure recovery, desktop/narrow screenshots, console
  capture, viewport overflow checks, observability parser acceptance, and NDJSON
  redaction checks against a real served dashboard with scratch state and
  synthetic metadata-only Codex usage.
- Platform target extraction: `curb-core/src/platform/target.rs` now owns sealed
  target construction, identity comparison, supervisor escalation, and
  child-first tree scoping behind the unchanged `Snapshot` facade.
- Timed headless-observability dogfood:
  `evidence/dogfood/2026-06-04-headless-observability/` verified the release
  headless sidecar against real local provider metadata in visibility mode. The
  20-second proof captured 20 NDJSON events, one startup `usage_scan`, seven
  repeated `watcher_tick` events, final readiness HTTP 200, 267 sessions, zero
  stoppable sessions, accepted parser output, and a clean redaction check.
- Longer local headless-observability dogfood:
  `evidence/dogfood/2026-06-04-headless-observability-3min/` verified the same
  sidecar path over a 180-second local window. It captured 72 NDJSON events,
  one startup `usage_scan`, 59 repeated `watcher_tick` events, final readiness
  HTTP 200, zero source-health errors, policy outcome counts on watcher ticks,
  accepted parser output, and a clean redaction check.
- Headless dogfood oracle hardening: `scripts/dogfood-headless-observability.sh`
  now validates positive integer duration, records `duration_seconds` and
  `expected_watcher_tick_min`, requires watcher ticks to scale with the requested
  window, and checks NDJSON for token/auth, prompt/response, screenshot,
  keystroke, file-content, raw-provider, and payload markers.
- Long-running sidecar dogfood:
  `evidence/dogfood/2026-06-12-long-sidecar/` ran a release-built
  `curb serve --headless` sidecar for 7,200 seconds with private runtime state
  outside the worktree and periodic live/ready/health/overview snapshots. It
  captured 3,168 NDJSON events, 1,506 `watcher_tick` events, final
  `/v1/ready` HTTP 200, 18 ready samples, 7 degraded samples, protected health
  staying HTTP 200, source-health failures for an oversized Claude JSONL line
  and provider session reads, RSS 48,144-135,020 KB, max sampled probe latency
  1.269932s, parser acceptance, and a token-specific redaction check. The
  wrapper threshold was corrected from one tick per five seconds to one per six
  seconds before this replacement run.
- Observability/config deepening: `src/observability/event.rs` owns the
  versioned log event schema and sanitization, `src/observability/registry.rs`
  owns event registration/path-template/outcome helpers, and
  `curb-core/src/config/{preset,policy_merge}.rs` own preset mechanics and
  agent-policy merge mechanics behind the unchanged `Config` facade.
- Ledger taxonomy deepening: `curb-core/src/ledger/taxonomy.rs` owns the closed
  `LedgerEvent` wire-string taxonomy plus alert/view classification, leaving
  `curb-core/src/ledger.rs` focused on append-only NDJSON persistence,
  hash-chain state, metadata enrichment, scrubbing, and readback.
- Service delta deepening: `curb-core/src/service/delta.rs` owns overview
  change detection across snapshots while `curb-core/src/service/snapshot_model.rs`
  stays focused on snapshot/session/agent row projection behind the unchanged
  `service::annotate_overview_delta` export.

## Ordered Work

1. Make long-run readiness/source-health recoverable for operators: preserve
   cheap live/health probes, avoid prolonged opaque `watcher_runtime: cache
   busy` readiness, and summarize provider source-health failures without log
   spelunking.
2. Continue behavior-preserving deep-module extractions in small milestones:
   remaining presenter/UI surfaces and any final binary shell pressure,
   following `docs/refactor-map.md`. Fresh critic feedback rates more internal
   taxonomy-style splits below browser-verified operator-flow work.
3. Continue UI polish through browser-verified operator flows, especially
   repeated real-session dogfood and narrow-viewport action states.

## Refactor Guardrails

- Do not rewrite around async, a framework, or new architecture vocabulary until
  small extractions prove value.
- Do not change wire formats without contract fixtures.
- Do not move provider parsing into enforcement or UI code.
- Do not weaken the termination invariant: production termination never accepts
  a bare PID.
- Every refactor milestone needs public behavior tests and a fresh critic before
  the next extraction.
