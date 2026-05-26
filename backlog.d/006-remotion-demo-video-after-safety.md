---
id: 006-remotion-demo-video-after-safety
title: Produce a Remotion demo after Curb safety semantics are solid
status: ready
lifecycle_stage: Feedback
acceptance:
    - A short demo script exists showing observe, warn, acknowledge, and enforcement behavior without risking desktop app termination.
    - The demo uses synthetic or controlled agent processes, not live expensive model sessions.
    - Remotion source or storyboard can render a buyer-facing walkthrough once process identity and alert UX are stable.
    - The demo references the evidence ledger and explains what Curb does not capture.
evidence_required:
    - go test ./...
    - demo script dry-run
    - rendered local preview or storyboard artifact
---

## Problem

Curb will benefit from a demo video, but producing the video before safety and
alert semantics are trustworthy risks polishing the wrong behavior.

## Desired Demo Arc

1. Operator starts Curb in visibility mode.
2. A controlled fake or local sleep-based agent process appears.
3. Curb records it and shows elapsed runtime.
4. Alert mode sends a warning.
5. Operator acknowledges and extends.
6. Enforcement mode terminates only the controlled worker process after grace.
7. `curb runs` shows the evidence ledger.

## Design Notes

- Remotion should visualize the product clearly; it should not become the
  implementation driver.
- Avoid real prompt, response, screenshot, or keystroke capture in demo assets.
- Keep the video suitable for an enterprise AI-ops/control conversation.
