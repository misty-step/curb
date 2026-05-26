---
id: 012-single-session-status-classifier
title: Centralize session status and action classification
priority: P0
status: ready
lifecycle_stage: Policy/Eval
acceptance:
    - One classifier returns process, usage, action, risk rank, acknowledgement, actionability, and explanation fields for a session.
    - `internal/service/model.go`, `cmd/curb/main.go`, and `ui/src/App.tsx` stop reinterpreting raw status strings independently.
    - Tests cover active, idle, idle-high, uncorrelated, watch-only, acknowledged, warn, stop, and enforcement/actionability cases.
    - UI selector tests consume classifier output instead of hard-coding policy state ladders.
evidence_required:
    - go test ./...
    - go test -race ./...
    - cd ui && npm test -- --run
---

# Context Packet: Centralize session status and action classification

## Goal

Curb computes “what state is this session in, and what can the operator safely do?” exactly once.

## Non-Goals

- Do not change token parsing or provider log ingestion.
- Do not change matcher scoring rules.
- Do not move rendering components around unless needed to remove duplicated state interpretation.
- Do not introduce a new workflow DSL or semantic orchestration layer.

## Constraints / Invariants

- Status axes remain orthogonal: process state, usage state, action state, and enforcement actionability are separate facts.
- A status string from logs or config is never enough to authorize termination.
- Explanations should come from the classifier so CLI, API, and UI use the same language.

## Authority Order

1. Tests
2. `internal/usagewatch` policy decisions
3. `internal/service` read model
4. UI/CLI renderers

## Repo Anchors

- `internal/usagewatch/usagewatch.go` - `EvaluateSessionDecision`
- `internal/service/model.go` - `sessionProcessState`, `sessionUsageState`, `sessionActionState`, `sessionRiskRank`
- `cmd/curb/main.go` - terminal action/status rendering
- `ui/src/App.tsx` - `isConfirmedSpendingSession`, `isSpendingAgent`, operator summary state selection

## Oracle

- [ ] A Go table test covers every classifier state and action combination.
- [ ] There is no separate `sessionRiskRank` ladder outside the classifier.
- [ ] React no longer decides whether a session is active by inspecting raw `process_state` plus token fields directly.
- [ ] CLI, API, and UI tests assert the same explanation text for uncorrelated, watch-only, and stop-pending sessions.

## Implementation Sequence

1. Add a table-driven classifier test from the current state combinations.
2. Introduce a small classifier type in `internal/usagewatch` or `internal/service` with explicit output fields.
3. Replace service read-model helper ladders with classifier output.
4. Update CLI and UI types/selectors to consume classifier fields.
5. Delete duplicated status helpers once tests prove parity.

## Risk + Rollout

- Risk: moving the ladders changes ordering or copy. Preserve current behavior in golden tests before refactoring.
- Rollback: keep the classifier behind service read-model construction while UI remains unchanged.

## Why

Ousterhout review identified status/action classification as the missing deep
module behind recent provider/runtime-owner bugs.
