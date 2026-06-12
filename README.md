# Curb

**A local watchdog for AI coding agents.** Curb watches how much every agent on
your machine is spending — tokens, per turn, since your last input — and warns
you, or stops it, when a run goes off the rails. One local service owns usage
ingestion, process correlation, notifications, policy, enforcement, and an
append-only audit ledger; the CLI and embedded dashboard are thin clients of it.
Everything stays on your machine, and no prompt or response content is ever read
or stored.

Curb measures one thing: **tokens an agent has spent since your last input** —
the runaway signal. When that turn spend crosses your warn line, Curb tells you;
in enforce mode, when it crosses your kill line, Curb stops the correlated
worker after a short grace period. Wall-clock runtime is not the signal: an
agent can idle for hours spending nothing, or burn a budget in one autonomous
loop.

Each agent is described by three facts: how much it has spent this turn
(`turn_tokens`), whether it is `working` or `idle`, and its `alert` level
(`ok`, `warn`, or `kill`). There are two modes — **Watch** (warn only) and
**Enforce** (warn, then stop runaways). No prompt or response content is ever
read or stored.

## Supported agents

Curb reads token usage for the agents that expose per-turn metadata locally:

- **Codex** — `~/.codex/sessions` (live) and `~/.codex/archived_sessions`.
- **Claude Code** — `~/.claude/projects`.
- **Pi** — `~/.pi/agent/sessions`.

Provider expansion starts from source evidence, not brand names. OpenCode,
Antigravity CLI, and GrokBuild are researched in
[docs/provider-adapter-research.md](docs/provider-adapter-research.md), but they
are not usage-metered until a provider-specific metadata parser ships. An agent
with no local token ledger cannot be metered until such a source exists.

Turn spend counts fresh work only: uncached input + cache-creation + output +
reasoning. Cached/re-read context is excluded for both providers, so a turn's
many tool calls do not re-count the context the model re-reads each call.

The implementation is active. The most useful entry points are:

- [docs/product-principles.md](docs/product-principles.md) - product philosophy, vision, and engineering principles.
- [docs/user-guide.md](docs/user-guide.md) - user operating guide.
- [docs/contributor-guide.md](docs/contributor-guide.md) - contributor architecture and verification guide.
- [docs/application-design.md](docs/application-design.md) - canonical dashboard architecture and UI design.
- [docs/release-evidence.md](docs/release-evidence.md) - current release proof index and historical evidence disposition.
- [docs/dogfooding.md](docs/dogfooding.md) - release-build dogfood guide and next grooming loop.
- [docs/adr/README.md](docs/adr/README.md) - accepted architecture decisions for headless mode, observability, and termination safety.
- [docs/runbooks/headless-sidecar.md](docs/runbooks/headless-sidecar.md) - headless server-side integration runbook.
- [docs/runbooks/observability-dogfood.md](docs/runbooks/observability-dogfood.md) - structured log capture and triage runbook.
- [docs/local-agent-watchdog-spec.md](docs/local-agent-watchdog-spec.md) - watchdog product spec.
- [SPEC.md](SPEC.md) - launch implementation specification.

## Rust Implementation

Rust is the primary Curb implementation. Its modules are intentionally deep:
strict config loading, append-only ledger handling, platform process
identity/termination-target safety, usage metadata reading, service read models,
session actions, the loopback API, embedded dashboard serving, and automatic
usage-policy watching.

Build and run from source:

```sh
cargo build --release --bin curb
./target/release/curb install
curb app
```

Useful development commands:

```sh
cargo test
cargo run -- init --config /tmp/curb/config.yaml
cargo run -- config reasonable
cargo run -- validate-config configs/curb.example.yaml
cargo run -- usage --all
cargo run -- dashboard
cargo run -- doctor
cargo run -- watch --once
cargo run -- status
cargo run -- runs
cargo run -- ack codex:session-id --extend 30m
cargo run -- serve
cargo run -- app
```

The normal product surface is intentionally small: configure Curb, then run the
local app or watcher.

```sh
curb config
curb app
curb watch
curb scan
curb dashboard
curb usage --since 24h
curb tail
curb status
curb runs --state attention
curb ack codex:session-id --extend 30m
curb config set --mode alert --warn-turn-tokens 1000000 --kill-turn-tokens 3000000
```

The default pre-merge gate is Rust-primary:

```sh
scripts/validate.sh
```

It checks the embedded UI assets, Rust formatting, clippy, Rust tests, the
synthetic demo dry-run, UI typecheck/lint/test, and the deterministic
fixture-backed dashboard browser smoke.

For faster agent iteration, run the gate ladder in this order:

```sh
scripts/check-setup.sh
scripts/install-git-hooks.sh
scripts/check-fast.sh
scripts/validate.sh
```

`scripts/check-setup.sh` verifies the local Rust/Node prerequisites and example
config without running the full suite, and syntax-checks the repo-managed Git
hook scripts. `scripts/install-git-hooks.sh` installs a pre-commit hook into the
current checkout; the hook runs `scripts/check-fast.sh` before a commit so
agents get local feedback before CI. `scripts/check-fast.sh` runs the
high-signal merge checks that normally catch code, type, lint, stale embed,
secret, contract, and rendered dashboard regressions before the slower desktop
and demo checks in `scripts/validate.sh`.

CI runs a named `fast feedback (ubuntu)` lane for `scripts/check-fast.sh`, the
full gate on Linux and macOS, and a focused `windows smoke` job for Rust
compilation, example config validation, notification capability behavior, and
Windows termination-command construction. It also runs a dedicated
`dependency audit` job for RustSec and npm advisory checks.
The rendered dashboard smoke is mandatory through `scripts/check-fast.sh`.
Run `cd ui && npm run smoke` directly when iterating on UI. By default it
starts Vite, serves committed API fixtures from `contracts/api/`, and writes
screenshots plus `manifest.json` under `ui/artifacts/smoke-dashboard/`.

`curb dashboard` shows live agent workers and recent usage in one terminal view.
`curb app` serves the built dashboard and opens it in your browser. If a
compatible service is already running on the configured loopback address, it
opens that dashboard instead of asking you to manage ports or paste tokens.
`curb serve` serves the localhost API and dashboard on loopback for advanced
clients and service-style launches.
`curb usage` reads local Codex and Claude metadata logs and summarizes sessions,
models, and token usage without printing or storing prompt or response content.
`curb tail` streams new local usage events as agents report token metadata. Use
`curb tail --since 1h --interval 2s` for an operator view, or
`curb tail --once` in scripts and demos.
`curb status`, `curb runs`, and `curb ack` use usage session keys such as
`codex:session-id`; legacy ledger run ids remain event metadata, not the action
handle.

The built UI is embedded in the Rust binary. `curb app` is the normal launch
path; `cd ui && npm run dev` is only needed while developing the frontend. The
dashboard is one list of working agents, each a spend bar against your warn and
kill lines, with idle agents folded into a count. It is built on a token-based
design system and follows the OS light/dark theme.

`curb watch` runs the policy loop. Each scan rebuilds per-session turn spend
from the provider logs (Codex `user_message` events, Claude typed-input rows,
and Pi user message entries mark turn boundaries), excludes cached/read context
from spend, and compares the current turn against your warn and kill lines. In
enforce mode it stops only a correlated live worker, after grace, and only after
revalidating process identity (PID, start time, owner, executable). It never
stops a watch-only desktop app root.

The generated default config watches agent worker processes such as Codex
Desktop workers, Codex CLI, Claude Code, and Anti-Gravity's `agy` CLI. Process
visibility is separate from usage metering: the Antigravity matcher can show a
live `agy` process, but Curb does not read Antigravity token usage yet. Desktop
applications such as Codex Desktop and Claude Desktop are not enforcement
targets; Curb will not terminate the app root.

Curb creates a durable local `machine_id` in the configured state directory and
adds it to service-owned ledger events. The ledger is local-only in the launch
surface; remote collectors are not configured or used for kill decisions.
