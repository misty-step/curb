---
id: 014-product-quality-gate
title: Make scripts validate the product quality gate
priority: P1
status: done
lifecycle_stage: Policy/Eval
acceptance:
    - `scripts/validate.sh` runs the product gate.
    - `ui/package.json` exposes `typecheck`, `lint`, `test`, and `build` scripts.
    - The product gate includes Go tests, Go vet, UI tests, UI typecheck, UI lint, and stale embedded UI detection.
    - Contributor docs list one primary command for local pre-merge validation.
evidence_required:
    - scripts/validate.sh
    - go test ./...
    - go vet ./...
    - cd ui && npm run typecheck
    - cd ui && npm run lint
    - cd ui && npm test -- --run
    - scripts/build-ui.sh --check
---

# Context Packet: Make scripts validate the product quality gate

## Goal

One command proves the Go product, React dashboard, and embedded assets are all
current enough to ship.

## Non-Goals

- Do not lower existing strict TypeScript settings.
- Do not introduce internal mocks to make tests easier.
- Do not require network access during validation.
- Do not add heavyweight browser screenshot baselines in this item; item 013 owns browser UX smoke.

## Constraints / Invariants

- `scripts/validate.sh` should fail if product code is broken.
- UI lint must be compatible with React 19 and Vite.
- The quality gate should be runnable on macOS, Linux, and Windows-compatible shells where possible; platform-specific build artifacts remain separate commands.

## Authority Order

1. Executable commands
2. `docs/contributor-guide.md`
3. Package manifests
4. Contributor docs

## Repo Anchors

- `scripts/validate.sh` - product validation entrypoint
- `ui/package.json` - missing lint and typecheck scripts
- `ui/tsconfig.json` - strict TypeScript settings already enabled
- `docs/contributor-guide.md` - command list
- `AGENTS.md` - product verification expectations

## Oracle

- [ ] `cd ui && npm run typecheck` passes.
- [ ] `cd ui && npm run lint` passes.
- [ ] `cd ui && npm test -- --run` passes.
- [ ] `go test ./...` passes.
- [ ] `go vet ./...` passes.
- [ ] `scripts/build-ui.sh --check` passes.
- [ ] `scripts/validate.sh` runs all of the above.

## Implementation Sequence

1. Add UI `typecheck` script as `tsc --noEmit`.
2. Add ESLint or Biome with React-aware rules and a zero-warning lint command.
3. Update `scripts/validate.sh` to run product gates in a clear order.
4. Update contributor docs to make `scripts/validate.sh` the single local pre-merge command.
5. Run the full gate and fix surfaced lint/type issues without changing product behavior.

## Risk + Rollout

- Risk: lint introduces churn. Start with conservative rules: correctness and React hooks before style.
- Risk: lint introduces noisy churn. Keep rules focused on correctness and
  React hooks rather than style preferences.
- Rollback: keep `typecheck` and product tests even if lint config needs a follow-up adjustment.

## Why

Beck review found existing tests are useful but optional; the repository lacks a
single executable quality gate for product changes.

## What Was Built

Completed on 2026-05-26. `scripts/validate.sh` now runs the product gate:
embedded UI freshness, Go tests, Go vet, UI typecheck, UI lint, and UI tests.
The UI package has `typecheck`, `lint`, `check`, `test`, and `build` scripts.
