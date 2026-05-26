---
acceptance:
    - The repository has explicit docs, verification commands, harness guidance, and automation appropriate for agent work.
evidence_required:
    - gradient readiness
    - gradient validate
id: 002-improve-agent-readiness
lifecycle_stage: Policy/Eval
status: done
title: Improve agent readiness from Gradient init scan
---

## Init Scan Findings

- `Add a committed CI or validation entrypoint.`
- `Declare test, lint, or validation commands.`
- `Commit a repo-local agent harness.`

## Resolution

Retired on 2026-05-26. The product now has a single local validation entrypoint
(`scripts/validate.sh`) and contributor docs. Repo-local harness projection was
removed in favor of shared Spellbook/system configuration.
