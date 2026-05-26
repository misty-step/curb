---
id: 015-groom-stale-gradient-bootstrap
title: Resolve stale Gradient bootstrap backlog items
priority: P2
status: done
lifecycle_stage: Intent
acceptance:
    - Items 001 and 002 are either archived as already satisfied, rewritten as current harness work, or moved out of the product backlog.
    - Any remaining Gradient/harness item has a concrete oracle that does not duplicate every other ticket's `gradient validate` evidence.
    - Backlog count and ordering make product work visible ahead of bootstrap residue.
evidence_required:
    - review backlog.d/001-gradient-onboarding.md
    - review backlog.d/002-improve-agent-readiness.md
    - scripts/validate.sh
---

# Context Packet: Resolve stale Gradient bootstrap backlog items

## Goal

The backlog stops treating initial harness adoption as active product work once the repo already has a working product and harness artifacts.

## Non-Goals

- Do not remove Gradient validation from product tickets.
- Do not delete bootstrap tickets without explicit operator approval.
- Do not change product code.

## Constraints / Invariants

- Deletion or archival requires human ratification.
- The product backlog should prioritize Curb user outcomes over harness bookkeeping.

## Repo Anchors

- `backlog.d/001-gradient-onboarding.md`
- `backlog.d/002-improve-agent-readiness.md`
- `AGENTS.md`
- `docs/contributor-guide.md`
- `scripts/validate.sh`

## Oracle

- [ ] Operator decides archive, rewrite, or keep for item 001.
- [ ] Operator decides archive, rewrite, or keep for item 002.
- [ ] No remaining active ticket says only “run Gradient” without product or harness acceptance criteria.

## Implementation Sequence

1. Compare 001 and 002 against the current repo state and validation commands.
2. Present archive/rewrite/keep options for each.
3. Apply the operator-approved moves only.

## Risk + Rollout

- Risk: deleting useful harness context. Avoid silent deletion; preserve history in `_done/` if archived.

## Why

Grug review flagged 001 and 002 as stale bootstrap residue that makes the
product backlog look less focused.

## Resolution

Completed on 2026-05-26. Items 001, 002, and 005 were moved to `_done`, local
Gradient projection files were removed, and product validation now stands alone.
