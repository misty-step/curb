---
id: 008-route-runtime-truth-through-service
title: Route runtime truth through the service read model
priority: P0
status: done
lifecycle_stage: Policy/Eval
acceptance:
    - Terminal dashboard, API snapshot, browser dashboard, and enforcement all use one service-owned read model for worker, session, turn, and action state.
    - The duplicate `findSessionForMatch` implementation in `cmd/curb/main.go` is removed or made a thin call into the shared correlation helper.
    - Active worker counts are process-owned, not usage-session-owned; two usage sessions on one correlated PID render as one active worker.
    - Uncorrelated usage logs can warn when over policy, but never claim a live Claude/Codex worker row.
    - Tests cover CWD prefix, provider family match, no-match, Codex-dispatched subprocess, and multi-session/single-worker cases.
evidence_required:
    - go test ./...
    - go test -race ./...
    - cd ui && npm test -- --run
    - go test ./internal/service ./internal/usagewatch ./cmd/curb -run 'Correlat|Snapshot|Dashboard'
    - scripts/build-ui.sh --check
---

# Context Packet: Route runtime truth through the service read model

## Goal

Every Curb surface shows the same live workers, usage sessions, token turns, and safe actions from one service-owned source of truth.

## Non-Goals

- Do not add prompt, response, screenshot, keystroke, or file-content capture.
- Do not change provider log parsers except where a fixture is needed to prove correlation behavior.
- Do not remove desktop app root protections or weaken termination identity checks.
- Do not perform a broad file-splitting refactor of `cmd/curb/main.go`; reduce duplication only where needed for runtime truth.
- Do not redesign the visual dashboard layout here; that is item 013.

## Constraints / Invariants

- Visibility and alert modes must never terminate processes.
- PID plus process start time remains the termination identity boundary.
- Runtime owner comes from live process evidence. Usage provider comes from logs. These are related facts, not the same identity.
- Uncorrelated usage remains visible and actionable for acknowledgement, but cannot become an active live-worker row.
- The service read model is the contract consumed by CLI, API, and React.

## Authority Order

1. Tests
2. `docs/contributor-guide.md` module boundaries
3. `internal/usagewatch` correlation and policy decisions
4. `internal/service` read model
5. CLI/UI rendering code

## Repo Anchors

- `internal/usagewatch/usagewatch.go` - `Correlate`, `EvaluateSessionDecision`
- `internal/service/model.go` - `BuildSnapshot`, `AgentView`, `SessionView`
- `cmd/curb/main.go` - duplicate CLI correlation and terminal dashboard renderers
- `ui/src/App.tsx` - operator summary currently renders derived state
- `ui/src/App.test.tsx` - regression tests for worker/session distinction

## Prior Art

- `internal/api/api.go` already treats `internal/service` as the application boundary.
- `internal/platform/platform.go` already hides termination safety behind `TerminationTarget`.
- `scripts/build-ui.sh --check` already proves embedded UI assets are generated from the React source.

## Oracle

- [ ] `go test ./...` passes.
- [ ] `go test -race ./...` passes.
- [ ] `cd ui && npm test -- --run` passes.
- [ ] A fixture with two usage sessions correlated to one PID reports one active worker in CLI JSON and UI tests.
- [ ] A fixture with recent Claude logs but no Claude live process reports unmatched usage, zero active Claude workers, and no Claude row in the operator summary.
- [ ] `cmd/curb/main.go` no longer contains a separate CWD-prefix `findSessionForMatch` implementation.
- [ ] `curb dashboard --json` and `curl /v1/snapshot` return equivalent worker/session/action counts for the same snapshot.

## Implementation Sequence

1. Add service-level regression fixtures for multi-session/single-worker, uncorrelated logs, and provider/runtime-owner divergence.
2. Export or internalize one correlation helper that both service read-model building and CLI renderers use.
3. Replace CLI dashboard/usage correlation calls with the shared service read model.
4. Adjust terminal text renderers to display `Worker`, `Session`, `Turn`, and `Action` facts without recomputing state.
5. Remove duplicate correlation helpers and stale state ladders from CLI code.
6. Run CLI JSON and API snapshot parity checks against the same fixture.

## Risk + Rollout

- Risk: the CLI becomes stricter and stops showing plausible-but-uncorrelated sessions as live workers. That is desired; call it out in release notes.
- Risk: broad service coupling sneaks in. Keep the interface at read-model and action boundaries only.
- Rollback: restore CLI renderer to direct usage-session rendering while leaving service tests intact.

## Dependencies

- Enables item 007 by giving the unified runtime loop one read model to publish.
- Enables item 009 by making the CLI a client of service truth instead of a second implementation.
- Should be completed before browser-level UI polishing or enforcement demo work.

## Grooming Notes

- Carmack and Grug independently ranked this as the first shippable slice.
- This packet intentionally absorbs the first phase of the old `008-consolidate-session-process-correlation` ticket.

## Progress

- 2026-05-26: `usagewatch.BestSessionForMatch` now owns best-session selection
  for a process match, and `cmd/curb` plus `internal/service` use it instead of
  duplicate CWD-prefix loops.
- 2026-05-26: Service snapshot tests now cover multi-session/single-worker
  collapse and uncorrelated Claude logs without a live Claude worker row.

## What Was Built

Completed on 2026-05-26. Runtime truth now routes through the service read model
for the dashboard/API/UI path, with process/session correlation centralized in
`usagewatch.BestSessionForMatch`. The terminal dashboard JSON and daemon
`/v1/snapshot` are covered by a real filesystem usage fixture that proves the
same worker/session/action counts across both surfaces. Service regression tests
cover multi-session/single-worker collapse and uncorrelated Claude logs without
inventing a live Claude worker row.

Evidence:

- `go test ./internal/service ./internal/usagewatch ./cmd/curb -run 'Correlat|Snapshot|Dashboard'`
- `scripts/validate.sh`
- `go test -race ./...`
- `cd ui && npm test -- --run`
- `scripts/build-ui.sh --check`
