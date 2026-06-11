# Trim evidence and documentation into release packets

Priority: P1
Status: pending
Estimate: M

## Goal
Make Curb's proof trail navigable by separating durable acceptance evidence from
bulky tranche receipts and stale readiness claims.

## Oracle
- [ ] Inventory `.harness-kit/traces/`, `evidence/dogfood/`,
      `docs/readiness-tranche-*`, `docs/agent-readiness-roadmap.md`, and active
      backlog references for stale or redundant claims.
- [ ] Keep the minimal evidence needed to reproduce safety, hosted CI, headless,
      UI, dependency-audit, and dogfood claims; move or delete only with explicit
      rationale.
- [ ] Update docs that still claim hosted CI is green when current `master`
      evidence is red, or link to the repair ticket that owns the discrepancy.
- [ ] Produce a release-evidence index that points cold agents to the canonical
      proof packets instead of requiring broad artifact scans.
- [ ] Leave `git status --short --untracked-files=all` clean and preserve all
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
starting to obscure current truth, especially with a red latest `master` CI run.

Do not delete dogfood evidence just because it is bulky; delete only when
another durable artifact preserves the acceptance proof.
