# Curb User Guide

Curb watches local AI agent processes and local agent usage logs. It records a
local ledger, summarizes token/model usage where agents expose it, warns when
recent usage exceeds policy, and can stop correlated live agent processes when
enforcement is explicitly configured.

## Install From Source

```sh
cargo build --release --bin curb
./target/release/curb install
```

The same Rust source builds on macOS, Linux, and Windows. Use the standard Rust
target workflow for cross-builds when the target toolchain is installed:

```sh
cargo build --release --target x86_64-unknown-linux-gnu --bin curb
cargo build --release --target x86_64-pc-windows-msvc --bin curb
```

## Start With The Normal Product Flow

Build once:

```sh
cargo build --release --bin curb
```

Then run the local app:

```sh
curb app
```

On first run, Curb creates a per-user config at the platform config location
and starts watching. The default configuration watches CLI agent processes,
not desktop app roots.

Normal commands:

```sh
curb config
curb dashboard
curb app
curb serve
curb usage --since 24h
curb tail
curb watch
curb runs
```

`curb usage` is the safest first command after installation. It scans recent
local Codex and Claude metadata logs and prints a compact table of sessions,
models, calls, and token totals. Curb does not print prompt or response content.
The top of the report tells you whether current usage is `OK`, `ACTIVE`,
`WATCH`, or `ACTION`; historical expensive sessions appear as `idle-high` so
they are visible without looking like live runaway spend.

```sh
curb dashboard
curb usage
curb usage --since 24h
curb usage --all --limit 25
curb usage --json
curb tail
```

Session statuses:

- `active` - usage happened inside the current rolling window and is within policy.
- `warn` - active usage is over a warning threshold.
- `stop` - active usage is over a stop threshold.
- `uncorrelated` - usage crossed a threshold, but no live process matched it.
- `watch-only` - usage crossed a threshold, but the matched process is not an enforcement target.
- `idle-high` - the session was expensive historically but is not currently spending.
- `idle` - no usage in the current rolling window.

Use presets for the common flows:

- `curb config observe` - record only.
- `curb config reasonable` - notify only, warn after 90 minutes.
- `curb config aggressive` - enforcement, warn after 30 seconds and kill
  after 60 seconds. This is for deliberate local testing.

For custom thresholds:

```sh
curb config set --mode enforcement --warn-after 2m --kill-after 4m --grace 30s --scan 5s
curb config set --warn-turn-tokens 1000000 --kill-turn-tokens 3000000 --usage-window 15m
```

The local app policy panel edits the same first-class policy fields. For
managed devices, edit the YAML config directly and run `curb validate-config`
before starting the watcher.

`curb config` shows the active config path, action, scan interval, policy,
configured agents, and local ledger path. The local API also exposes the
durable endpoint `machine_id` for dashboards and future export receivers. In an
interactive terminal it prompts for the common setup.

## Local UI API

`curb app` is the normal local UI entrypoint:

```sh
curb app --addr 127.0.0.1:8765
```

It starts the same local service as `curb serve`, serves the embedded
dashboard, opens it in your browser, and authenticates the dashboard with a
same-origin HttpOnly cookie. The normal app path does not ask you to paste an
API token.

`curb serve` serves the same local API and dashboard without opening a browser:

```sh
curb serve --addr 127.0.0.1:8765
```

The daemon runs usage policy scans, warning notifications, and enforcement from
the same service that serves the UI API. It binds only to loopback. The
embedded `curb app` dashboard receives a same-origin HttpOnly cookie
automatically, so normal UI use does not require pasting a token.

Advanced CLI and development clients use the per-user bearer token stored at
`<state_dir>/api.token` with `0600` permissions to call endpoints such as
`/v1/snapshot`, `/v1/overview`, `/v1/agents`, `/v1/sessions`, `/v1/events`,
`/v1/alerts`, `/v1/config`, `/v1/notifications/health`, and
`/v1/notifications/test`
with:

```sh
curl -H "Authorization: Bearer $(cat <state_dir>/api.token)" \
  http://127.0.0.1:8765/v1/overview
```

The dashboard notification test uses `/v1/notifications/test`, which calls the
same local notification path as policy alerts. If local notifications are
disabled, the test is not delivered and the UI reports that policy state instead
of pretending notifications work.

The UI can still be run from source for frontend development:

```sh
cd ui
npm install
npm run dev
```

Open `http://127.0.0.1:5173` and connect. If you run the frontend from Vite,
use the advanced connection drawer to paste the daemon token from
`<state_dir>/api.token`, or use the demo mode.

## Enforcement Boundary

Curb treats usage as the primary enforcement signal. Runtime limits are still
kept in config for legacy run ledgers, but the default watcher does not run a
separate duration-based kill loop when usage monitoring is disabled. With
`usage.enabled: false`, `curb watch`, `curb serve`, and `curb app` all refresh
visibility only. Curb currently enforces the latest metadata-reported turn size
and displays rolling-window activity for context.

Curb can track process agents such as `codex`, `claude`, `claude-code`, and
Anti-Gravity's `agy` CLI, but process visibility is not the same as usage
metering. Usage policy currently comes from supported Codex, Claude, and Pi
metadata adapters; Antigravity is process-visible only until a metadata-only
usage adapter ships. Desktop applications such as Codex Desktop and Claude
Desktop are watch-only when explicitly configured and are not killed by the
standard presets. This keeps the kill switch focused on runaway worker
processes instead of closing an entire desktop app.

## Start With The Repo Example

Use the example policy first:

```sh
curb validate-config configs/curb.example.yaml
curb watch --once --config configs/curb.example.yaml
curb watch --config configs/curb.example.yaml
```

Visibility mode never kills processes. It discovers configured agents and writes `.curb/runs.ndjson`.

## Common Commands

```sh
curb --help
curb status --config configs/curb.example.yaml
curb watch --once --config configs/curb.example.yaml
curb runs --config configs/curb.example.yaml
curb runs --json --config configs/curb.example.yaml
curb ack codex:session-id --config configs/curb.example.yaml --extend 30m --reason "still supervising"
curb doctor --config configs/curb.example.yaml
```

Usage enforcement uses session acknowledgements. Use the selected session's
Extend button in the local UI to record a bounded acknowledgement; Curb will
project the session as acknowledged and will not escalate that session again
until the acknowledgement expires. A session can be acknowledgeable even when
it is not stoppable, for example in alert mode, when the matched process is
watch-only, or when Curb cannot safely correlate usage to a live process.

Usage enforcement acknowledges sessions with keys such as `codex:session-id`;
ledger run ids are event metadata, not the action handle.

In enforcement mode, a selected `stop-pending` session may also show a
destructive Stop button. That button is not an arbitrary PID killer. The UI
shows the correlated PID, process start time, owner, executable or app identity,
and process-tree scope, then sends that evidence back to the service as a stale
state check. The service performs a fresh process capture, rebuilds the
session-to-process correlation, rejects watch-only app roots, rejects stale or
changed identity, builds a sealed `TerminationTarget`, and only then asks the
platform adapter to stop the process tree.

## Configure Agents

Agents are matched by stable identifiers first and weaker signals second:

1. bundle ID or signing metadata
2. executable/app path
3. process name
4. command-line regex
5. parent process name

Example:

```yaml
agents:
  - id: codex-cli
    label: Codex CLI
    family: codex
    kind: process
    match:
      process_names: [codex]
      command_regex: ["/codex(\\s|$)", "@openai/codex"]
    policy:
      warn_after: 90m
      kill_after: 120m
```

Use `exclude_process_names`, `exclude_command_regex`, and `exclude_parent_command_regex` to keep helper,
renderer, or app-dispatched subprocesses from becoming independent runs.

## Enforcement

Set `mode: enforcement` only after visibility and alert behavior are understood.
Usage enforcement compares `usage.warn_turn_tokens` and
`usage.kill_turn_tokens` to the latest token-usage turn for the session, not
the session's cumulative lifetime total. It terminates only after:

1. a session has recent token activity over the configured latest-turn limit,
2. Curb correlates that usage session to a live agent process by provider and working directory,
3. the usage grace period elapses,
4. the correlated process is still live,
5. PID, start time, owner, executable/app identity, and process-tree scope are
   revalidated immediately before termination.

Alert and visibility modes emit `usage_would_terminate` instead of killing.

## Privacy

Curb does not record prompts, responses, screenshots, keystrokes, or file
contents. Usage readers extract only metadata such as timestamp, provider,
session id, model, working directory, and token counters.

The local ledger is the source of audit truth. The launch config does not define
remote forwarding or alert webhooks; remote collectors are future export work,
not part of process policy or termination.
