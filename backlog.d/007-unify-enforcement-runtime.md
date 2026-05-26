---
id: 007-unify-enforcement-runtime
title: Unify Curb enforcement on service and usagewatch
status: ready
lifecycle_stage: Policy/Eval
acceptance:
    - `curb daemon`, `curb app`, and `curb watch` run the same policy loop when usage monitoring is enabled.
    - Session-based acknowledgement is the only active ack path for usage enforcement; run-based ack remains documented as legacy or is bridged with an explicit migration note.
    - When usage monitoring is disabled, behavior is explicit and consistent across CLI and daemon (either both enforce duration policy or both visibility-only with documented rationale).
    - `watchdog` no longer owns a parallel full enforcement loop for the default product path; it remains available for process matching until matcher extraction lands.
    - Tests prove daemon and CLI watch emit the same ledger event types for equivalent usage-policy scenarios.
evidence_required:
    - go test ./...
    - go test -race ./...
    - go build -o /tmp/curb-darwin ./cmd/curb
    - /tmp/curb-darwin validate-config configs/curb.example.yaml
---

## Goal

Operators get one predictable enforcement story: start Curb any way they normally
would (`curb`, `curb watch`, `curb daemon`, `curb app`) and the same usage policy,
warnings, grace, and kill semantics apply.

## Grooming Status

Blocked behind `backlog.d/008-route-runtime-truth-through-service.md`. Do not
start this broad runtime-loop unification until CLI/API/UI all consume the same
service read model for worker, session, turn, and action state.

## Lifecycle Stage

Policy/Eval — closes the split between legacy runtime watchdog and usage-first
enforcement.

## Non-Goals

- Do not add prompt, response, screenshot, keystroke, or file-content capture.
- Do not remove process matching or desktop watch-only protections (see
  `backlog.d/003-safe-process-identity-calibration.md`).
- Do not require external admin APIs for enforcement.
- Do not redesign the full alert notification UX here (see
  `backlog.d/004-actionable-alerts-and-acknowledgement-ux.md`).

## Authority Order

1. `SPEC.md` — usage-first pivot and operating modes
2. `docs/contributor-guide.md` — module boundaries
3. `docs/application-architecture.md` — service as application boundary
4. `internal/usagewatch`, `internal/service`, `internal/watchdog`

## Problem

Curb currently runs two policy engines:

| Path | Runtime | Policy signal | Ack model |
|------|---------|---------------|-----------|
| `curb watch` (usage on) | `usagewatch.Run` | Token spend | Session ack |
| `curb watch` (usage off) | `watchdog.Run` | Wall-clock runtime | Run ack |
| `curb daemon` / `curb app` | `service.Start` → `usagewatch.Scan` only | Token spend when enabled | Session ack via API |

When usage is enabled, CLI watch and daemon agree. When usage is disabled, the
daemon refreshes snapshots but never runs duration enforcement. `curb ack`
writes run-based ack files while the UI/API use session-based acks. Ledger event
names differ (`policy_warning` vs `usage_warning`, etc.).

This split-brain violates the documented boundary that `service` owns daemon
orchestration and `usagewatch` owns usage policy.

## Alternatives

### Minimal viable

Route `cmdWatch` through `internal/service` the same way `cmdDaemon` does.
Document that duration-only enforcement requires usage to stay enabled or a
follow-up item. Unify ack CLI onto session keys.

**Failure mode:** duration fallback remains dead in daemon until explicitly
revived or removed.

### Ideal

Extract `watchdog.Match` into a matcher-only module. Retire `watchdog.Run`
from product paths. Single `service` loop owns refresh, policy scan, ledger
writes, and notifications. One ack store keyed by session (with optional run
metadata). Config documents the deprecation of duration-primary enforcement.

**Failure mode:** larger refactor; needs migration for operators relying on
run-based ack commands.

## Repo Anchors

- `cmd/curb/main.go` — `cmdWatch` branches on `cfg.Usage.IsEnabled()`
- `internal/service/service.go` — `Start`, `ScanPolicy`, `buildUsageWatch`
- `internal/usagewatch/usagewatch.go` — usage policy loop
- `internal/watchdog/watchdog.go` — legacy run lifecycle
- `internal/service/actions.go` — session ack actions

## Implementation Sequence

1. Complete item 008 so all surfaces share one read model and correlation helper.
2. Inventory behavioral diffs between `cmdWatch` and `service.Start` with tests.
3. Make `cmdWatch` delegate to `service` (or shared runner) instead of calling
   `watchdog.New` / `usagewatch.New` directly.
4. Decide and document usage-disabled behavior for daemon + CLI (enforce,
   visibility-only, or fail config validation).
5. Align `curb ack` with session ack semantics or mark run ack deprecated.
6. Reduce `watchdog` to matcher + optional duration helper; delete duplicate
   loop ownership once tests pass.

## Acceptance Evidence

- Table-driven test: same config + fixtures → same ledger events from watch
  runner and service runner.
- Manual smoke: `curb daemon` warns/kills correlated usage the same way
  `curb watch` does under enforcement preset.
- No regression in visibility/alert modes terminating processes.

## Risks and Rollback

- **Risk:** operators lose duration enforcement if usage-disabled path is cut
  without replacement. Mitigate with explicit config validation or bridged
  duration policy inside `usagewatch`.
- **Risk:** ack migration breaks existing `curb ack <run-id>` scripts. Mitigate
  with compatibility shim reading old ack files.
- **Rollback:** keep watchdog loop behind feature flag until session path is
  proven in production smoke.

## Public-Safe Risk

Low — documents local enforcement semantics only; no private deployment data.
