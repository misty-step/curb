---
id: 009-route-cli-through-service-boundary
title: Thin CLI over the service application boundary
status: done
lifecycle_stage: Context
acceptance:
    - `cmd/curb/main.go` shrinks materially; command handlers delegate to `internal/service` or small `cmd/curb/*` files for composition only.
    - Dashboard, daemon, watch, config mutation, and ack paths do not duplicate read-model or policy logic owned by `service`.
    - `cmdUsage` surfaces config load errors instead of swallowing them.
    - Advanced inspection commands remain available but do not bypass the service boundary for mutable actions.
evidence_required:
    - go test ./...
    - go test -race ./...
    - go build -o /tmp/curb-darwin ./cmd/curb
---

## Goal

Contributors can change policy and UI read models in one place (`internal/service`)
without hunting through an ~1,900-line CLI file for parallel logic.

## Grooming Status

Dependent on `backlog.d/008-route-runtime-truth-through-service.md`. This item
must not become a cosmetic file split. Its first acceptance gate is removal of
duplicate read-model, correlation, and actionability logic from CLI paths.

## Lifecycle Stage

Context — strengthens the documented deep-module boundary between daemon core
and clients.

## Non-Goals

- Do not change user-visible command names or default UX (`curb`, `curb app`, etc.).
- Do not move provider log parsing out of `internal/usage` in this item.
- Do not build a separate Tauri shell (future per `docs/application-architecture.md`).

## Authority Order

1. `docs/application-architecture.md` — service owns stateful concerns; CLI is a client
2. `docs/contributor-guide.md` — `cmd/curb` owns CLI composition only
3. `internal/service`

## Problem

`cmd/curb/main.go` currently owns command routing, config presets, dashboard
printing, usage reporting, daemon serving, watch loops, ack parsing, run
summaries, and duplicated view helpers. The architecture doc says HTTP/CLI/UI
should speak only to service-owned views and actions, but much of the CLI
reaches into `usage`, `watchdog`, and `usagewatch` directly.

## Alternatives

### Minimal viable

Extract packages under `cmd/curb/` (`watch.go`, `usage.go`, `print.go`) without
behavior change. Route `cmdWatch` and `cmdDashboard` through `service` APIs.

**Failure mode:** file split only; boundary violation persists in some commands.

### Ideal

`cmd/curb` is flag parsing + stdout formatting. All snapshot, config update,
ack, and scan operations call `service.Service` methods. Shared JSON/text
renderers consume `service.Snapshot` and related views only.

**Failure mode:** larger initial refactor; pays down debt for items 007 and 008.

## Repo Anchors

- `cmd/curb/main.go` (~1,881 lines)
- `internal/service/service.go`, `model.go`, `actions.go`, `config.go`
- `internal/api/api.go` — reference client that already uses the boundary

## Implementation Sequence

1. Complete item 008 so the CLI has a stable service read model to consume.
2. List CLI commands that bypass `service` (watch, dashboard, usage, scan, ack, runs).
3. Add missing service methods where API already has equivalents.
4. Move watch/daemon onto shared service runner (coordinates with item 007).
5. Split `main.go` into focused files only after logic has moved behind service APIs.
6. Fix `cmdUsage` config error handling as a drive-by correctness fix.

## Risks and Rollback

- **Risk:** refactor-only PR is hard to review. Mitigate with behavior-preserving
  tests before moves.
- **Rollback:** incremental merges per command group.

## Public-Safe Risk

None.

## What Was Built

- Reduced `cmd/curb/main.go` from roughly 1,985 lines to 120 lines by moving
  daemon, watch, usage, inspection, config, and formatting code into focused
  `cmd/curb/*_cli.go` files.
- Routed `curb dashboard` through `internal/service.SnapshotSince` instead of
  rebuilding the service read model in the CLI.
- Kept `curb watch` on `internal/service.Run` from the enforcement unification
  slice.
- Fixed `curb usage` to return usage-reader errors rather than printing a
  partial report.
- Added CLI coverage that verifies `curb usage --config missing.yaml` surfaces
  the config load error.

## Acceptance Evidence

- `go test ./...`
- `go test -race ./...`
- `go build -o /tmp/curb-darwin ./cmd/curb`
