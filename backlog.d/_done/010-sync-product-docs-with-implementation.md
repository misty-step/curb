---
id: 010-sync-product-docs-with-implementation
title: Align README and entry docs with the shipping Go product
status: done
lifecycle_stage: Intent
acceptance:
    - README describes Curb as an implemented local agent watchdog with build/run commands, not as planning-only artifacts.
    - README lists the primary commands (`curb`, `curb app`, `curb daemon`, `curb usage`) consistently with `docs/user-guide.md`.
    - Contributor guide module list matches actual packages (`api`, `service`, `usagewatch`, `web`, etc.).
    - No doc claims planning-only status where code and tests exist.
evidence_required:
    - manual review of README.md against `go test ./...` and `go build ./cmd/curb`
---

## Goal

A new contributor cloning the repo understands within one README pass that Curb
is a working product with a clear module map and verification commands.

## Lifecycle Stage

Intent — reduces onboarding friction and false "spec only" assumptions.

## Non-Goals

- Do not rewrite `SPEC.md` launch scope in this item.
- Do not document private harness internals beyond existing `AGENTS.md`.
- Do not add marketing copy.

## Authority Order

1. `README.md`
2. `docs/contributor-guide.md`
3. `docs/user-guide.md`
4. Working tree (`go test ./...`, `cmd/curb/main.go` command table)

## Problem

`README.md` still states the repository "currently contains planning
artifacts," while the tree includes a full Go implementation, React UI, daemon
API, usage readers, and passing tests. That contradiction sends new contributors
and agents to specs before code.

## Alternatives

### Minimal viable

Update README opening paragraph, command list, and link to contributor guide.
Add one "Architecture" pointer to `docs/application-architecture.md`.

**Failure mode:** docs drift again without a checklist.

### Ideal

README + contributor guide share a single "verification" block sourced from
`AGENTS.md` / contributor guide; README defers deep architecture to docs.

**Failure mode:** slightly more editing surface.

## Repo Anchors

- `README.md` — stale planning-only claim
- `docs/contributor-guide.md` — authoritative module list
- `AGENTS.md` — agent verification commands

## Implementation Sequence

1. Replace planning-only framing with product summary (visibility → usage → enforcement).
2. Sync command examples with `curb help` output.
3. Cross-check module bullets against `internal/` tree.
4. Add note that specs (`SPEC.md`, watchdog spec) remain authoritative for launch scope.

## Risks and Rollback

- **Risk:** overclaiming maturity. Mitigate by keeping launch/draft labels on
  `SPEC.md` where appropriate.
- **Rollback:** doc-only revert.

## Public-Safe Risk

None.

## What Was Built

Completed on 2026-05-26. README now presents Curb as an implemented Go product
with user and contributor docs as the primary entry points. Contributor docs now
match the current validation gate.
