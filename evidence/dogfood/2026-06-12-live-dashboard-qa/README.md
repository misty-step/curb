# Live Dashboard QA

Date: 2026-06-12

Purpose: drive the browser dashboard against a real `curb serve` endpoint
using scratch state, synthetic metadata-only Codex usage, and a harmless
marker-gated `python3` subprocess.

Command:

```sh
bash scripts/qa-live-dashboard.sh evidence/dogfood/2026-06-12-live-dashboard-qa
```

Evidence:

- `manifest.json`: browser action log, viewports, screenshots, console errors,
  and failure list.
- `dashboard-desktop.png` and `dashboard-narrow.png`: live served UI captures.
- `safe-stop-rejection.json`: browser-origin protected API request showing
  stale stop identity is rejected.
- `worker-exit.json`: confirms the synthetic worker was stopped after
  browser confirmation.
- `live.json`, `ready-initial.json`, and `health-authenticated.json`:
  live service probes.
- `server.ndjson`, `parse-observability-smoke.txt`, and
  `redaction-check.txt`: runtime observability and redaction proof.

Safety:

- Only synthetic Codex token metadata is written under a temporary HOME.
- The destructive stop path targets only a scratch marker-gated
  `python3` process with PID/start-time/owner/executable evidence.
- The script is advisory. It is not wired into `scripts/check-fast.sh` or
  `scripts/validate.sh` until repeated runs prove it is deterministic enough.
