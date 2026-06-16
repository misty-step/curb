# Long-Running Headless Sidecar Dogfood

Date: 2026-06-15

Purpose: run Curb as a release-built headless sidecar for a realistic operator
window using private state outside the worktree, while keeping enforcement off
and recording periodic operational snapshots.

Commands:

```sh
CURB_LONG_DOGFOOD_SECONDS=7200 \
CURB_LONG_DOGFOOD_SNAPSHOT_SECONDS=300 \
bash scripts/dogfood-long-sidecar.sh evidence/dogfood/2026-06-15-long-sidecar-refresh
```

Environment:

- Build SHA: `090cfcf2d2540a90d4de31740ad99bf09aa0ca14`
- Branch/worktree: `deliver/042-refresh-long-sidecar-proof` / `<redacted-local-path>`
- OS: `Darwin 25.5.0 arm64`
- Address: `127.0.0.1:60957`
- Config: `config.yaml`
- Mode: visibility
- Home scanned: `<redacted-local-path>`
- State path: private temporary directory under `<redacted-local-path>`, outside the repo
- Ledger artifact: `ledger.ndjson`
- Structured log: `headless-sidecar.ndjson`

Evidence:

- `build-release.txt`: release binary build.
- `validate-config.txt`: generated outside-worktree state config validates.
- `usage-since-24h.txt` and `usage-since-24h.raw.txt`: provider
  source-health aggregate baseline before the headless run, including the exit
  status. The summary intentionally omits per-session IDs and local paths except
  when the CLI reports a source-health error path.
- `live.json`, `ready-initial.json`, `ready-final.json`,
  `health-unauthenticated.status`, and `health-authenticated.json`: public
  and protected API probes.
- `snapshots/`: periodic live, ready, health, and overview probes.
- `ready-samples.tsv`: readiness status samples by timestamp.
- `probe-latency.tsv`: live/health/overview latency samples.
- `resource-samples.tsv`: server RSS/CPU samples.
- `overview-initial.json`, `overview-final.json`,
  `sessions-initial-count.txt`, `sessions-final-count.txt`, and
  `events-final.json`: protected API evidence after the operator window.
- `parse-observability-smoke.txt`: parser accepted the NDJSON artifact and
  required runtime policy fields.
- `long-run-summary.json` and `long-run-summary.txt`: source-health,
  readiness, watcher-tick, latency, and resource drift summary.
- `redaction-check.txt`: runtime API token when supplied, auth header, prompt,
  response, screenshot, keystroke, file-content, raw-provider, and payload
  terms were absent from NDJSON.
- `path-redaction.txt`: local path prefixes were removed from committed
  evidence artifacts.
- `session-redaction.txt`: session IDs were removed from committed text
  evidence artifacts.

Safety notes:

- The generated config keeps `mode: visibility`, so this run cannot terminate
  processes.
- Local notifications are disabled for the dogfood config.
- Provider ingestion remains metadata-only; raw prompt/response content is not
  captured.
- Per-session response dumps are intentionally not committed because they can
  contain local project labels.

Operator notes:

- Startup: the generated config validated with private state outside the
  worktree, the release binary served `127.0.0.1:60957`, /v1/live returned HTTP 200,
  and the initial /v1/ready returned HTTP 503
  (degraded) with watcher_runtime reason
  `snapshot unavailable`.
- Readiness samples: the 7200-second window produced
  25 periodic readiness samples: ready=25
  (200=25). Final /v1/ready returned HTTP 200
  (ready); /v1/live and protected /v1/health stayed available
  during sampled probes.
- Provider roots discovered: the run started with 56 sessions
  and ended with 75 sessions. Source-health emitted
  16 error events: 16 for codex. The
  preflight usage scan output is captured in `usage-since-24h.txt`.
- Notification capability: final overview reported notifications
  disabled: local notifications are disabled in Curb policy.
- False positives: no policy warnings, `would_stop`, stop attempts, stop
  completions, or stop rejections appeared in the visibility-mode watcher
  ticks.
- False negatives: no ledger events were written during the run, so this
  packet does not prove alert delivery or enforcement-mode stop behavior.
- Process-correlation surprises: final overview reported
  `1093 processs captured` and `889 processs with identity evidence`; session
  counts are captured at the start and end to separate normal provider churn
  from actionable source-health breakage.
- Resource/latency drift: RSS ranged from 19860 KB to 53764 KB, max
  sampled CPU was 91.7%, max watcher policy duration was
  74524 ms, and max sampled probe latency was
  4.633927 seconds.

Follow-up ranking:

| Rank | Item | Evidence | Decision |
|---:|---|---|---|
| 1 | Keep refreshed readiness proof as the release baseline | All 25 periodic readiness samples were `ready` and HTTP 200 after startup. | Replace the old readiness-degradation blocker in release docs; continue watching probe latency and source-health errors. |
| 2 | Keep provider source-health failures actionable | 16 source-health error events and the preflight source-health output are captured in this packet. | Treat repeated provider failures as recovery-cockpit evidence, not readiness failure; open parser/source-specific work only when the sanitized recovery state is not enough to act. |
| 3 | Keep `scripts/dogfood-long-sidecar.sh` as the long-run harness | The wrapper produced release build, config validation, snapshots, parser output, summary, and redaction proof for 1571 events and 1441 watcher ticks. | Do not add a repo-local QA/dogfood skill yet; defer until browser-backed live operator workflow evidence adds another repeatable procedure. |
