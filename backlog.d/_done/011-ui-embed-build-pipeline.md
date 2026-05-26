---
id: 011-ui-embed-build-pipeline
title: Single-step UI build into internal/web/dist
status: done
lifecycle_stage: Context
acceptance:
    - One documented command (or `go generate` hook) builds `ui/` and copies output into `internal/web/dist/` for embedding.
    - Contributor guide states when committed dist artifacts must be refreshed.
    - CI or `./scripts/validate.sh` fails when embed dist is stale relative to `ui/src` (or documents explicit opt-out with rationale).
    - `curb app` and `go test ./internal/web/...` serve the freshly built assets in dev workflow.
evidence_required:
    - go test ./...
    - go build -o /tmp/curb-darwin ./cmd/curb
    - documented build command in docs/contributor-guide.md or ui/README.md
---

## Goal

Releasing or developing the dashboard never depends on manually syncing two dist
directories or guessing which embed tree is authoritative.

## Grooming Status

Mostly shipped. `scripts/build-ui.sh`, `scripts/build-ui.sh --check`, and
`docs/contributor-guide.md` now cover the primary embed workflow. Remaining
question: whether `ui/dist/` should stay committed or be ignored in favor of
`internal/web/dist` as the only committed embed target.

## Lifecycle Stage

Context — adapter/build hygiene for the embedded React client.

## Non-Goals

- Do not change UI features or design tokens.
- Do not introduce Tauri packaging.
- Do not commit `ui/node_modules` differently than today.

## Authority Order

1. `internal/web/web.go` — `//go:embed dist/*`
2. `ui/package.json` — Vite build
3. `docs/contributor-guide.md`

## Problem

The product embeds UI from `internal/web/dist/`, while Vite builds to `ui/dist/`.
Both trees appear in the repo. It is easy to edit `ui/src`, build locally to
`ui/dist`, and ship a Go binary still serving stale embedded assets.

## Alternatives

### Minimal viable

Add `make ui` or `scripts/build-ui.sh` that runs `npm run build` in `ui/` and
rsyncs to `internal/web/dist/`. Document in contributor guide.

**Failure mode:** developers forget to run script; stale embed persists.

### Ideal

`go generate` in `internal/web` triggers UI build; validate script compares
hashes or mtimes of key entry files. Optional: drop `ui/dist/` from repo and
only commit `internal/web/dist/`.

**Failure mode:** CI needs Node; acceptable for a product with a React surface.

## Repo Anchors

- `ui/dist/`
- `internal/web/dist/`
- `internal/web/web.go`
- `ui/README.md`

## Implementation Sequence

1. Confirm `internal/web/dist` is the canonical committed embed target.
2. Decide whether `ui/dist/` remains committed or is gitignored.
3. If this decision is accepted, archive this ticket as shipped.

## Risks and Rollback

- **Risk:** CI Node version drift. Pin in script or use `ui/package-lock.json`.
- **Rollback:** script-only; no runtime behavior change.

## Public-Safe Risk

None.

## What Was Built

Completed on 2026-05-26. `scripts/build-ui.sh` is the single embed build path,
`scripts/build-ui.sh --check` detects stale embedded assets, and
`scripts/validate.sh` runs the check before Go and UI tests. `internal/web/dist`
is explicitly unignored because it is the Go embed target; `ui/dist` remains
generated output.
