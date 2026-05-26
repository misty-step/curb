---
id: 004-actionable-alerts-and-acknowledgement-ux
title: Make Curb alerts actionable without watching a terminal
status: ready
lifecycle_stage: Evidence
acceptance:
    - Warning, grace, watch-only, would-kill, killed, and failed-kill events produce clear local notifications.
    - Notifications or follow-up commands expose acknowledgement and extension actions.
    - `curb runs` and `curb status` show the current action needed for each active run.
    - Tests cover alert-mode behavior without termination and enforcement-mode grace behavior.
evidence_required:
    - go test ./...
    - go test -race ./...
    - manual notification smoke or documented unsupported-notification fallback
---

## Problem

Terminal output is not a sufficient alerting surface for a runaway-agent
watchdog. The operator may be away from the terminal, in another app, or relying
on Curb specifically because they do not want to babysit a process.

## Desired UX

At warning time, the user should see:

- agent family;
- cwd/workspace label where available;
- elapsed runtime;
- policy threshold;
- what happens next;
- acknowledgement command or native action;
- whether the process is killable, watch-only, or ambiguous.

## Candidate Commands

```sh
curb status
curb runs --active
curb ack <run-id> --extend 30m
curb explain <run-id>
```

## Design Notes

- Keep the default CLI small.
- Put low-level inspection behind advanced help.
- Local notifications should degrade gracefully when permissions are missing.
- The ledger should record whether the user was actually notified.
