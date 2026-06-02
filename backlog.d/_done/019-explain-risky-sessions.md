# Make Curb explain every risky session

Priority: P1
Status: done
Estimate: M

## Goal
Turn the dashboard into an operator cockpit for one question: what is spending
right now, why is it risky, and what can safely happen next?

## Non-Goals
- Adding new provider adapters.
- Migrating to Tauri or changing packaging.
- Changing enforcement thresholds or termination policy.

## Oracle
- [x] Selecting a session shows a per-turn timeline from the existing session
      turns API, including token breakdown, model/source, timestamps, and cost
      where available.
- [x] The selected session explains alert and correlation evidence: matched
      process identity, PID/start-time seal, owner/executable/app identity,
      watch-only or enforceable reason, and current ack/stop affordance.
- [x] First-run readiness is surfaced in the product flow by consuming the
      onboarding, notification-health, and platform-capability endpoints instead
      of leaving readiness as hidden API surface.
- [x] UI selectors/read models cover the selected-session explanation state.
- [x] `scripts/validate.sh` passes.

## Notes
Closeout: implemented in `7363ea3` with focused UI/API tests and worker
browser smoke evidence.

Proper grooming found a product-shape mismatch: `docs/application-design.md`
and the API already point toward turn inspection and readiness, but the first
screen remains mostly a session list plus settings. This ticket makes Curb more
useful before expanding providers or packaging.
