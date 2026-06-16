# Long-Running Headless Sidecar Dogfood Refresh

Date: 2026-06-12

Purpose: rerun the two-hour release-built headless sidecar proof against the
current branch, refresh release evidence, and verify that Curb remains
metadata-only while live and protected health probes stay available.

Commands:

```sh
CURB_LONG_DOGFOOD_SECONDS=7200 \
CURB_LONG_DOGFOOD_SNAPSHOT_SECONDS=300 \
bash scripts/dogfood-long-sidecar.sh evidence/dogfood/2026-06-12-long-sidecar-refresh

python3 scripts/verify-long-sidecar-evidence.py \
  evidence/dogfood/2026-06-12-long-sidecar-refresh \
  --duration-seconds 7200 \
  --redact-local-path-prefix <local-home-prefix>
```

Environment:

- Build SHA: `140d3b1c628bdb72f20a79d165c0d30920888428`
- Branch/worktree: `deliver/040-ci-dogfood-doctrine-next` /
  `<redacted-local-path>`
- OS: `Darwin 25.5.0 arm64`
- Config: `config.yaml`
- Mode: visibility
- Home scanned: `<redacted-local-path>`
- State path: private temporary directory outside the repo
- Ledger artifact: `ledger.ndjson`
- Structured log: `headless-sidecar.ndjson`

Evidence:

- `build-release.txt`: release binary build.
- `validate-config.txt`: generated outside-worktree state config validates.
- `usage-since-24h.txt` and `usage-since-24h.raw.txt`: provider
  source-health baseline before the headless run.
- `live.json`, `ready-initial.json`, `ready-final.json`,
  `health-unauthenticated.status`, and `health-authenticated.json`: public and
  protected API probes.
- `snapshots/`: periodic live, ready, health, and overview probes.
- `ready-samples.tsv`: degraded/readiness transitions by timestamp.
- `probe-latency.tsv`: live/health/overview latency samples.
- `resource-samples.tsv`: server RSS/CPU samples.
- `overview-initial.json`, `overview-final.json`,
  `sessions-initial-count.txt`, `sessions-final-count.txt`, and
  `events-final.json`: protected API evidence after the operator window.
- `parse-observability-smoke.txt`: parser accepted the NDJSON artifact and
  required runtime policy fields.
- `long-run-summary.json` and `long-run-summary.txt`: source-health,
  readiness, watcher-tick, latency, and resource drift summary.
- `redaction-check.txt`: auth header, prompt, response, screenshot, keystroke,
  file-content, raw-provider, and payload terms were absent from NDJSON.
- `path-redaction.txt`: local path prefixes were removed from committed
  evidence artifacts.
- `verification.txt`: final readiness/probes, usage scan, watcher tick floor,
  and redaction checks passed.

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
  worktree, `/v1/live` returned HTTP 200, and initial `/v1/ready` returned HTTP
  503 only because `watcher_runtime` had no first snapshot yet.
- Degraded-readiness transitions: the 7,200-second window produced 25 periodic
  readiness samples: `ready=17`, `degraded=8` (`200=17`, `503=8`). Final
  `/v1/ready` returned HTTP 200 (`ready`); sampled `/v1/live`, `/v1/health`,
  and `/v1/overview` probes stayed HTTP 200.
- Watcher cadence: the refreshed verifier accepts at least 90% of the ideal
  six-second watcher cadence. This run emitted 1,110 watcher ticks against an
  ideal 1,200 and minimum 1,080, so it passed while still leaving scan overhead
  visible.
- Provider roots discovered: the run started with 199 sessions and ended with
  192 sessions. Source-health emitted 1,117 error events: 1,111 for Claude and
  6 for Codex. The preflight usage scan returned exit status 1 because one
  Claude JSONL line exceeded the configured 1 MiB safety cap.
- Notification capability: final overview reported notifications disabled:
  `local notifications are disabled in Curb policy`.
- False positives: no policy warnings, `would_stop`, stop attempts, stop
  completions, or stop rejections appeared in the visibility-mode watcher
  ticks.
- False negatives: no ledger events were written during the run, so this packet
  does not prove alert delivery or enforcement-mode stop behavior.
- Process-correlation surprises: final overview reported `1107 processs
  captured` and `895 processs with identity evidence`; session counts are
  captured at the start and end to separate normal provider churn from
  actionable source-health breakage.
- Resource/latency drift: RSS ranged from 66,136 KB to 219,864 KB, max sampled
  CPU was 93.4%, max watcher policy duration was 54,702 ms, and max sampled
  probe latency was 10.657177 seconds.

Follow-up ranking:

| Rank | Item | Evidence | Decision |
|---:|---|---|---|
| 1 | Bound or snapshot readiness while watcher cache is busy | Eight sampled `/v1/ready` 503 responses reported degraded readiness while `/v1/live` and protected health stayed 200. | Keep routed to the loopback transport/readiness milestone. |
| 2 | Make provider source-health failures actionable | 1,117 source-health error events and the preflight oversized Claude JSONL failure require log reading today. | Keep routed to the operator recovery surface. |
| 3 | Keep `scripts/dogfood-long-sidecar.sh` plus `scripts/verify-long-sidecar-evidence.py` as the long-run harness | The wrapper produced release build, config validation, snapshots, parser output, summary, verifier output, and redaction proof. | Do not add a repo-local QA/dogfood skill yet; the harness script is enough until another repeatable procedure appears. |
