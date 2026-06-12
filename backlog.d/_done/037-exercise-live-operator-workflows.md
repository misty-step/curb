# Exercise live operator workflows with browser-backed QA

Priority: P1
Status: done
Estimate: L

## Goal
Move beyond fixture-only dashboard confidence by proving realistic operator
workflows against a running Curb service and generated evidence artifacts.

## Oracle
- [x] Add or document a browser QA path that drives a live `curb app` or
      `curb serve` endpoint, not only Vite fixtures.
- [x] Cover the core operator flows: first launch, active session selection,
      readiness triage, ack, safe stop rejection, confirmed synthetic stop,
      settings save/revert, notification test, and live API failure/recovery.
- [x] Produce screenshots or videos for desktop and narrow viewports with
      console-error capture and viewport-overflow checks.
- [x] Keep destructive stop proof limited to a harmless synthetic subprocess with
      PID/start-time/owner/executable evidence.
- [x] Wire the live workflow as manual or advisory at first, then decide whether
      any subset belongs in `scripts/check-fast.sh` or `scripts/validate.sh`.

## Closeout
- Added advisory `scripts/qa-live-dashboard.sh` and
  `ui/scripts/live-dashboard-qa.mjs`.
- Evidence:
  `evidence/dogfood/2026-06-12-live-dashboard-qa/manifest.json`,
  `dashboard-desktop.png`, `dashboard-narrow.png`,
  `safe-stop-rejection.json`, `worker-start.json`, `worker-exit.json`,
  `server.ndjson`, `parse-observability-smoke.txt`, and
  `redaction-check.txt`.
- Decision: keep advisory for now; do not wire into `scripts/check-fast.sh` or
  `scripts/validate.sh` until repeated local runs prove it is stable enough for
  mandatory gates.

## Children
1. Reuse `ui/scripts/smoke-dashboard.mjs` assertions where possible, but
   separate fixture smoke from live-service QA.
2. Add a live synthetic fixture runner that starts Curb with private scratch
   state and known metadata-only provider logs.
3. Capture artifact packets under `evidence/dogfood/` and link them from the QA
   docs.
4. Promote only deterministic, low-flake checks into mandatory gates.

## Notes
**Why:** Product QA perspective. The current mandatory smoke is useful and
fixture-backed, while dogfood scripts prove headless APIs. No active item owns
the browser-mediated experience against a real running service after the June 5
UI polish.

Do not add a flaky browser gate to mandatory CI until it has deterministic
startup, cleanup, and artifacts on failure.
