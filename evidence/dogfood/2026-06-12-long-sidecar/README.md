# Long-Running Headless Sidecar Dogfood

Date: 2026-06-12

Purpose: run Curb as a release-built headless sidecar for a realistic operator
window using private state outside the worktree, while keeping enforcement off
and recording periodic operational snapshots.

Commands:

```sh
CURB_LONG_DOGFOOD_SECONDS=7200 \
CURB_LONG_DOGFOOD_SNAPSHOT_SECONDS=300 \
bash scripts/dogfood-long-sidecar.sh evidence/dogfood/2026-06-12-long-sidecar
```

Environment:

- Build SHA: `c22e2c2c91fa24a65a2b41e365f5bd7e31b9f28d`
- Branch/worktree: `deliver/035-long-sidecar-dogfood` / `/Users/phaedrus/Development/curb`
- OS: `Darwin 25.5.0 arm64`
- Address: `127.0.0.1:53423`
- Config: `config.yaml`
- Mode: visibility
- Home scanned: `/Users/phaedrus`
- State path: private temporary directory under `/var/folders/jr/0kj0xfdd4s1ggs921sr2d7f80000gn/T/curb-long-sidecar.iZb5aE`, outside the repo
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
- `redaction-check.txt`: token, auth header, prompt, response, screenshot,
  keystroke, file-content, raw-provider, and payload terms were absent from
  NDJSON.

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
  worktree, the release binary served `127.0.0.1:53423`, `/v1/live` returned
  HTTP 200, and the initial `/v1/ready` returned HTTP 503 only because
  `watcher_runtime` had no first snapshot yet.
- Degraded-readiness transitions: the two-hour window produced 25 periodic
  snapshot samples after the initial probe, with 18 ready samples, 7 degraded
  samples, and final `/v1/ready` HTTP 200. Degraded samples were
  `watcher_runtime: cache busy`; `/v1/live` and protected `/v1/health` stayed
  HTTP 200.
- Provider roots discovered: the run started with 110 sessions and ended with
  122 sessions. Source-health emitted 1,548 error events: 1,507 for Claude and
  41 for Codex. The preflight usage scan also reported an oversized Claude
  JSONL line under `.claude/projects`.
- Notification capability: notifications were intentionally disabled by the
  dogfood policy, and the overview capability reported `notifications:
  disabled`; this was not a platform failure.
- False positives: no policy warnings, `would_stop`, stop attempts, stop
  completions, or stop rejections appeared in the visibility-mode watcher
  ticks.
- False negatives: no ledger events were written during the run, so this
  packet does not prove alert delivery or enforcement-mode stop behavior.
- Process-correlation surprises: process capture and identity evidence stayed
  available in the final overview, but session counts changed during the
  operator window. Future recovery UI should distinguish normal provider
  churn from actionable source-health breakage.
- Resource/latency drift: RSS ranged from 48,144 KB to 135,020 KB, max sampled
  CPU was 94.4%, max watcher policy duration was 19,961 ms, and max sampled
  probe latency was 1.269932 seconds.

Follow-up ranking:

| Rank | Item | Evidence | Decision |
|---:|---|---|---|
| 1 | Bound or snapshot readiness while watcher cache is busy | Seven sampled `/v1/ready` 503 responses reported `watcher_runtime: cache busy` while `/v1/live` and protected health stayed 200. | Route to `backlog.d/039-finish-facade-and-presenter-simplification.md` as part of the loopback transport/readiness milestone. |
| 2 | Make provider source-health failures actionable | 1,548 source-health error events and the preflight oversized Claude JSONL failure require log reading today. | Route to `backlog.d/036-build-operator-recovery-cockpit.md` as an operator recovery state. |
| 3 | Keep `scripts/dogfood-long-sidecar.sh` as the long-run harness | The wrapper produced release build, config validation, snapshots, parser output, summary, and redaction proof. | Do not add a repo-local QA/dogfood skill yet; defer until browser-backed live operator workflow evidence adds another repeatable procedure. |
