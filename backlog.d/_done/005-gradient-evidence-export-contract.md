---
id: 005-gradient-evidence-export-contract
title: Export Curb runtime facts for Gradient evidence packets
status: done
lifecycle_stage: Evidence
acceptance:
    - Curb exposes a stable public-safe export for run events that Gradient can consume without parsing internal state.
    - The export includes run ID, agent family, matcher confidence, mode, policy profile, warning/ack/termination events, and redacted process identity.
    - The export omits prompt, response, screenshot, keystroke, and file-content data.
    - Docs explain that Curb remains standalone and Gradient integration is optional.
evidence_required:
    - go test ./...
    - go test -race ./...
    - /tmp/curb-darwin runs or equivalent export command fixture
    - gradient validate
---

## Problem

Curb should remain a Unix-style standalone watchdog. Gradient can still use Curb
as a policy/evidence input when a work item needs proof about local agent
runtime, warnings, acknowledgements, or enforcement.

That requires a stable export contract rather than having Gradient scrape
Curb's private ledger format.

## Candidate Export Shape

```json
{
  "schema_version": "curb.run.v1",
  "run_id": "run_...",
  "agent_family": "codex-desktop-worker",
  "mode": "alert",
  "policy_profile": "reasonable",
  "matcher": {
    "confidence": 170,
    "killable": true,
    "reasons": ["executable_path", "command_line", "parent_lineage"]
  },
  "process_identity": {
    "pid": 123,
    "start_time": "2026-05-19T15:55:00Z",
    "executable_summary": "Codex.app worker",
    "cwd_summary": "redacted-or-workspace-label"
  },
  "events": []
}
```

## Non-Goals

- Do not make Gradient required for Curb.
- Do not include raw private prompt or screen data.
- Do not commit machine-specific process paths unless explicitly redacted or
  synthetic.

## Resolution

Retired on 2026-05-26 as stale Gradient integration scope. If Curb later needs
an external evidence export, shape it as a provider-neutral Curb API/export
ticket rather than a repo-local harness integration.
