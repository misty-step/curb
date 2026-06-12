# Trim evidence and documentation into release packets

Priority: P1
Status: done
Estimate: M

## Goal
Make Curb's proof trail navigable by separating durable acceptance evidence from
bulky tranche receipts and stale readiness claims.

## Oracle
- [x] Inventory `.harness-kit/traces/`, `evidence/dogfood/`,
      `docs/readiness-tranche-*`, `docs/agent-readiness-roadmap.md`, and active
      backlog references for stale or redundant claims.
- [x] Keep the minimal evidence needed to reproduce safety, hosted CI, headless,
      UI, dependency-audit, and dogfood claims; move or delete only with explicit
      rationale.
- [x] Update docs that still claim hosted CI is green when current `master`
      evidence is red, or link to the repair ticket that owns the discrepancy.
- [x] Produce a release-evidence index that points cold agents to the canonical
      proof packets instead of requiring broad artifact scans.
- [x] Leave `git status --short --untracked-files=all` clean and preserve all
      evidence that is still an acceptance source for open tickets.

## Children
1. Classify every evidence directory and harness trace as canonical, historical, redundant, or stale.
2. Create a small proof index for current release claims and update docs to reference it.
3. Propose deletion or archive candidates in a reviewable diff; do not silently
   remove evidence.
4. Re-run the gate after evidence/doc moves to catch broken links, scripts, and
   embed drift.

## Notes
**Why:** Simplification perspective. The readiness tranche deliberately kept a
large evidence set for review. Now that the branch merged, that volume is
starting to obscure current truth, especially where old docs described stale
hosted CI state instead of the current `master` proof.

Do not delete dogfood evidence just because it is bulky; delete only when
another durable artifact preserves the acceptance proof.

## Closeout

- Added `docs/release-evidence.md` as the canonical current proof index for
  hosted CI, browser-backed UI QA, long sidecar, headless safety, timed
  observability, active provider metadata, and dependency-audit claims.
- Updated `docs/agent-readiness-roadmap.md` from stale PR #2 / run
  `27380787071` language to current PR #5 / run `27435399495` proof, while
  keeping older red runs as historical context.
- Marked `docs/readiness-tranche-closeout.md` and
  `docs/readiness-tranche-commit-plan.md` as historical June 4 artifacts rather
  than current release status.
- Linked `README.md` and `evidence/dogfood/README.md` to the release evidence
  index so cold agents start from the canonical packet map.
- Deleted no dogfood evidence and no harness traces. `.harness-kit/traces/`
  remains historical provider-lane receipt evidence; older dogfood directories
  remain retained context or still-active acceptance sources.
