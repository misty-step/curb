# Hosted proof and readiness tranche closeout

Priority: P0
Status: complete
Estimate: M

## Goal

Turn the current local readiness tranche into trusted, reviewable, hosted proof.
Curb has strong local validation, dogfood evidence, headless observability, and
module-boundary cleanup in this worktree, but the repository cannot claim L4
agent-readiness while the work is an uncommitted detached-HEAD diff with no
hosted CI, Windows runner, dependency-audit, or coverage evidence.

## Context

The current readiness roadmap rates Curb as L3 Standardized. The blocking
pattern is not another speculative feature; it is proving that the local system
holds when packaged as a branch, reviewed as coherent changes, and run through
the remote gates that future agents will rely on.

Local evidence already exists:

- `scripts/validate.sh` passed on June 4, 2026 after the headless,
  observability, UI stop-confirmation, dogfood-oracle hardening, and
  deep-module extraction work.
- `scripts/check-dependency-audit.sh --offline`, `bash
  scripts/check-dependency-audit.sh --online`, and direct
  `scripts/check-dependency-audit.sh --offline` passed locally.
- Headless observability and enforcement dogfood evidence exists under
  `evidence/dogfood/`.

Remaining trust gaps:

- The worktree is dirty and detached, so reviewers cannot reason about the
  tranche as a branch yet.
- The GitHub `fast feedback`, `full validate`, `windows smoke`,
  `dependency audit`, and `coverage` jobs are defined locally but have not been
  observed green for this branch.
- Windows support is configured through a focused smoke, but only hosted runner
  evidence can prove the Windows compile and command-construction paths.
- The current diff mixes code, docs, scripts, contracts, UI assets, and
  evidence; closeout needs an intentional review shape rather than a dump.

## Oracle

- [x] Create or switch to a named branch for the readiness tranche without
      discarding user work. Current branch: `agent-readiness-closeout`.
- [x] Classify the current dirty tree into coherent review groups in
      `docs/readiness-tranche-closeout.md`:
      headless/observability, gates/governance, API/UI contracts, module
      refactors, UI stop-confirmation, dogfood evidence, and generated
      `web/dist` assets.
- [x] Remove, archive, or explicitly keep every untracked artifact. The current
      decision is to keep every untracked artifact for review, documented by
      path pattern in `docs/readiness-tranche-closeout.md`. Evidence
      directories are signal by default, and every current dogfood evidence
      directory has a README with command/mode/safety context.
- [x] Classify the current dirty tree into semantic commit/review groups with
      exact pathspecs and per-group verification in
      `docs/readiness-tranche-commit-plan.md`.
- [x] Re-run the full local gate from the named branch:
      `scripts/validate.sh` passed on `agent-readiness-closeout` on
      June 4, 2026. It covered the UI embed check, Rust fmt/clippy/file-length/
      termination-boundary/secret scan, Rust tests, CLI tests, curb-core tests,
      real-process enforcement E2E tests, UI typecheck/lint/Vitest/browser
      smoke, desktop shell checks, and demo 006 dry-run.
- [x] Re-run the local advisory proof:
      `scripts/check-dependency-audit.sh --offline` passed against 1105 cached
      RustSec advisories and 187 crate dependencies, and
      `bash scripts/check-dependency-audit.sh --online` fetched RustSec,
      updated crates.io, found 0 Rust vulnerabilities, and completed npm audit.
- [x] Push the branch and capture hosted job evidence for:
      `fast feedback (ubuntu)`, `full validate (ubuntu-latest)`,
      `full validate (macos-latest)`, `windows smoke`, `dependency audit`, and
      `coverage`. Draft PR:
      `https://github.com/misty-step/curb/pull/1`. Passing CI run:
      `https://github.com/misty-step/curb/actions/runs/26931762206` at head
      `2da127dc119e68aed2078b0cd39e0695900e34d7`.
- [x] If any hosted job fails, preserve the failure log or URL, root-cause it,
      and fix the product or gate without lowering thresholds.
      Initial hosted run
      `https://github.com/misty-step/curb/actions/runs/26931405010` failed
      because CI installed UI packages but not the Playwright Chromium binary
      required by `ui/scripts/smoke-dashboard.mjs`; coverage also reported
      82.78% against the 84% floor. The fix installed Chromium in hosted UI
      gate jobs and added API backend-adapter coverage without lowering the
      threshold.
- [x] Update `docs/agent-readiness-roadmap.md` with the hosted evidence links,
      exact commands, and any residual waivers.
- [x] Update `.harness-kit/agent-readiness.yaml` only if the remote evidence
      changes the durable readiness contract.
      No profile update was needed; the durable gate inventory was already
      accurate.
- [x] Leave the closeout worktree clean or explicitly document every remaining
      dirty path as user-owned or follow-up work.
      The remaining edits are this closeout evidence update and will be
      committed before final report.

## Non-Goals

- Do not lower coverage, lint, clippy, file-length, termination-boundary,
  secret-scan, dependency-audit, or browser-smoke gates to get green.
- Do not collapse unrelated changes into one review story if splitting them
  would make failures easier to isolate.
- Do not claim Windows product acceptance from local macOS proof.
- Do not remove dogfood evidence just because it is bulky; decide whether it is
  acceptance evidence first.

## Suggested Proof

```sh
git status --short --branch --untracked-files=all
scripts/validate.sh
scripts/check-dependency-audit.sh --offline
bash scripts/check-dependency-audit.sh --online
git push -u origin <branch>
gh pr checks --watch
gh pr view --json mergeable,reviewDecision,statusCheckRollup
```

## Acceptance Source

This ticket is accepted by branch/PR evidence, not by another local roadmap
paragraph. The final closeout report must name the branch, hosted run or PR
URLs, exact local commands, providers or waivers used, accepted/rejected
reviewer findings, and residual unverified paths.
