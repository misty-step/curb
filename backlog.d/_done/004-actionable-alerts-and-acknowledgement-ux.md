---
id: 004-actionable-alerts-and-acknowledgement-ux
title: Make Curb alerts actionable without watching a terminal
status: done
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

## What Was Built

- Usage-watch notifications now cover warning, would-stop, grace, blocked stop,
  failed stop, and completed stop events.
- Notification delivery failures are recorded as `notification_failed` ledger
  events.
- `curb status` and `curb runs` now show the current run action, such as
  `review or ack`, `ack now`, `would stop`, `watch-only`, `stopping`, or
  `review failure`.
- Alert-mode tests prove warning and would-stop notifications are attempted
  without termination.
- Enforcement-mode tests prove grace, completed-stop, and failed-stop
  notification paths.

## Acceptance Evidence

- `go test ./...`
- `go test -race ./...`
- `/tmp/curb-darwin doctor --config configs/curb.example.yaml` reported
  `notifications: ok` on macOS.
