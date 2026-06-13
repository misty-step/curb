# Dogfooding Curb

Curb's next proof is real local use. The merged code has green local and remote
gates, but product confidence now comes from running the release binary during
normal agent work and recording what is useful, confusing, or wrong.

## Current Build

Build the release CLI:

```sh
cargo build --release --bin curb
```

Run the local app:

```sh
./target/release/curb app
```

For a headless/server-style run:

```sh
CURB_LOG_FORMAT=json ./target/release/curb serve --headless --addr 127.0.0.1:8765 2> curb.ndjson
./target/release/curb watch
```

For repeatable timed observability evidence, prefer the repo script:

```sh
CURB_DOGFOOD_SECONDS=60 bash scripts/dogfood-headless-observability.sh
```

For browser-backed live UI evidence, use the advisory QA script:

```sh
bash scripts/qa-live-dashboard.sh evidence/dogfood/$(date +%F)-live-dashboard-qa
```

This starts a real `curb serve` endpoint with scratch state and synthetic
metadata-only Codex usage, then drives the served dashboard with Playwright. It
captures desktop and narrow screenshots, browser console errors, viewport
overflow checks, ack, settings save/revert, notification test, stale stop
rejection, confirmed synthetic stop, and API failure recovery. Keep it advisory
until repeated runs show it is stable enough for `scripts/check-fast.sh` or
`scripts/validate.sh`.

For a stronger local sidecar proof, run a longer window into a unique evidence
directory:

```sh
CURB_DOGFOOD_SECONDS=180 bash scripts/dogfood-headless-observability.sh evidence/dogfood/$(date +%F)-headless-observability-3min
```

`curb serve --headless` keeps the local API/runtime available without serving
the embedded web UI. It exposes unauthenticated liveness/readiness probes at
`/v1/live` and `/v1/ready`; protected API routes such as `/v1/health`,
`/v1/overview`, session actions, config updates, and stop requests still require
the API token and loopback binding.

Structured logs use the schema in `docs/observability.md`. Attach the validated
NDJSON artifact to the dogfood evidence directory when a run is meant to support
backlog ranking or release confidence.

## What To Watch

- Does startup choose the expected config and state directory?
- Does `curb usage --since 24h` find real Codex, Claude, and Pi metadata without
  showing prompt or response content?
- Does the app clearly distinguish active, warn, stop, watch-only,
  uncorrelated, idle-high, and idle sessions?
- Do notifications report truthfully when they are disabled or unavailable?
- Does enforcement remain scoped to correlated worker processes, never desktop
  app roots?
- Are false positives, false negatives, or process-correlation surprises easy to
  understand from the UI and ledger?

## Olympus Readiness

Curb is effectively modular enough for Olympus when run headless on Linux.
Olympus can treat Curb as a governor core or sidecar: initialize it on a Sprite,
feed policy sessions from Olympus run state, and use Olympus-owned stop tokens
for cooperative run or lane stops. The first integration should stay in Olympus
adapter code rather than making Curb depend on Olympus internals.

## Next Grooming Session

After the first real dogfood session, use
`backlog.d/023-post-closeout-grooming-and-dogfood.md` to shape the next tranche:
refactoring, stronger gates, Windows proof, release/install flow, user-like QA,
and Olympus adapter readiness.

The current groomed tranche starts with
`backlog.d/024-dogfood-evidence-matrix.md`, then moves through headless server
mode, structured observability, quality gates/API contracts, and deep-module
refactoring. Keep that order unless new dogfood evidence shows a higher-risk
failure.

Current local headless evidence includes
`evidence/dogfood/2026-06-04-headless-observability-3min/`, a 180-second
visibility-mode run with repeated watcher ticks, final readiness HTTP 200,
parser acceptance, and NDJSON redaction checks. Treat this as local dogfood
evidence, not hosted or multi-hour deployment proof.

Current long sidecar evidence includes
`evidence/dogfood/2026-06-12-long-sidecar/`, a 7,200-second release-built
`curb serve --headless` run with private runtime state outside the worktree,
periodic live/ready/health/overview snapshots, final readiness HTTP 200, parser
acceptance, and redaction checks. It also found recurring provider
source-health errors and intermittent `watcher_runtime: cache busy` readiness
degradation while `/v1/live` and protected health stayed available.

The current refreshed packet is
`evidence/dogfood/2026-06-12-long-sidecar-refresh/`. It is verified by
`scripts/verify-long-sidecar-evidence.py`, which recomputes the summary,
requires final readiness HTTP 200, requires all sampled live/health/overview
probes to be HTTP 200, checks NDJSON redaction, and enforces at least 90% of
the ideal six-second watcher cadence so long scans remain visible without
making the oracle depend on perfect scheduler timing.

For the next long operator window, use:

```sh
CURB_LONG_DOGFOOD_SECONDS=7200 CURB_LONG_DOGFOOD_SNAPSHOT_SECONDS=300 \
  bash scripts/dogfood-long-sidecar.sh evidence/dogfood/$(date +%F)-long-sidecar
```

Keep the wrapper as the current long-dogfood path; a repo-local QA/dogfood
skill is not justified until browser-backed live operator workflow evidence
adds another repeatable procedure.

The headless observability script now fails weak timed runs: it validates
`CURB_DOGFOOD_SECONDS`, requires watcher ticks to scale with the requested
window, and checks NDJSON for token/auth, prompt/response, screenshot,
keystroke, file-content, raw-provider, and payload markers.

Dogfood evidence should live under `evidence/dogfood/YYYY-MM-DD-<short-slug>/`
and include enough source-health, notification, startup, UI, and safety notes to
justify the next backlog ranking without relying on memory.
