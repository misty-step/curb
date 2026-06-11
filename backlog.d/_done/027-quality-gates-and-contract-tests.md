# Tighten quality gates and API/UI contracts

Priority: P1
Status: ready
Estimate: M

## Goal

Raise Curb's agent-readiness by making failures faster, more user-like, and
more contract-driven across CI, UI, API, and cross-platform paths.

## Context

The current gate is strict and passed locally, but a cold run took more than
twelve minutes after restoring UI dependencies. CI proves Linux/macOS plus a
focused Windows smoke. Shared API fixtures guard the Rust/UI contract, and the
deterministic browser smoke now runs inside the fast and full pre-merge gates.

This ticket is now split into implementation slices:

- `030-api-ui-contract-drift-guard.md` owns API/UI fixtures and drift checks.
- `031-fast-feedback-and-cross-platform-gates.md` owns fast/full gate split,
  Windows proof, and browser smoke classification.

## Oracle

- [x] Define the merge policy for each new gate before implementation:
      mandatory pre-merge gate, advisory nightly/report-only gate, or manual
      smoke. Mandatory gates must fail PRs and local `scripts/validate.sh` where
      practical.
- [x] Split CI into fast and full lanes while preserving `scripts/validate.sh`
      as the full local pre-merge gate.
- [x] Add a Windows CI or smoke job that proves the platform-specific compile
      and runtime paths that already exist, including the Windows termination
      command construction and notification capability behavior.
- [x] Promote `ui/scripts/smoke-dashboard.mjs` into an automated gate, with
      deterministic local data and artifacts when it fails.
- [x] Add shared API contract fixtures or generated schema checks so Rust
      service read models and `ui/src/types.ts` cannot silently drift.
- [x] Add strict malformed-payload and unknown-enum tests at the API boundary
      and corresponding UI client assertions for operator-visible failures.
- [x] Add a local setup/onboarding smoke command that restores the declared UI
      toolchain, detects Node version drift, and proves a first-run safe path.
- [x] Stabilize or instrument the real-process enforcement E2E tests so a gate
      failure like `terminated_session_is_not_rekilled_on_the_next_scan`
      produces enough timing/process evidence to distinguish product regression
      from OS scheduling flake, and so repeated local runs are reliable.
- [x] Documentation states which gates are mandatory for merge, which are
      report-only, and what exact command reproduces each failure locally.

## Non-Goals

- Do not lower coverage, lint, or file-length gates.
- Do not add a new frontend test framework.
- Do not remove the committed `web/dist` embed check.

## Suggested Proof

```sh
scripts/validate.sh
cargo test -p curb-core --test e2e_enforcement -- --nocapture
cd ui && npm run smoke
gh run view --json jobs,conclusion
rg -n "mandatory|report-only|manual smoke" .github scripts docs README.md
```
