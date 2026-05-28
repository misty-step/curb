# Curb

Curb is a local endpoint agent for AI-assisted engineering work. One local service
on the machine owns usage ingestion, process correlation, notifications,
policy, enforcement, and the audit ledger; the CLI and embedded UI are thin
clients of that local service.

The design target is simple: create local visibility into agent activity first,
including token usage and model usage where agent logs expose it, then layer
warnings and enforcement on top of those signals. Wall-clock runtime is useful
for stale or stuck processes, but it is not a reliable proxy for spend.

The implementation is active. The most useful entry points are:

- [docs/user-guide.md](docs/user-guide.md) - user operating guide.
- [docs/contributor-guide.md](docs/contributor-guide.md) - contributor architecture and verification guide.
- [docs/application-design.md](docs/application-design.md) - canonical dashboard architecture and UI design.
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
synthetic demo dry-run, and UI typecheck/lint/test.

`curb dashboard` shows live agent workers and recent usage in one terminal view.
`curb app` serves the built dashboard and opens it in your browser. If a
compatible service is already running on the configured loopback address, it
opens that dashboard instead of asking you to manage ports or paste tokens.
`curb serve` serves the localhost API and dashboard on loopback for advanced
clients and service-style launches.
`curb usage` reads local Codex and Claude metadata logs and summarizes sessions,
models, and token usage without printing or storing prompt or response content.
`curb tail` streams new local usage events as agents spend tokens. Use
`curb tail --since 1h --interval 2s` for an operator view, or
`curb tail --once` in scripts and demos.
`curb status`, `curb runs`, and `curb ack` use usage session keys such as
`codex:session-id`; legacy ledger run ids remain event metadata, not the action
handle.

The built UI is embedded in the Rust binary. `curb app` is the normal launch
path; `cd ui && npm run dev` is only needed while developing the frontend.

`curb watch` is usage-first when usage monitoring is enabled. It warns on recent
token activity that crosses the configured latest-turn limit, and in enforcement
mode it stops only a correlated live agent process.

The generated default config watches agent worker processes such as Codex
Desktop workers, Codex CLI, Claude Code, and Anti-Gravity's `agy` CLI. Desktop
applications such as Codex Desktop and Claude Desktop are not enforcement
targets; Curb will not terminate the app root.

Curb creates a durable local `machine_id` in the configured state directory and
adds it to service-owned ledger events. The ledger is local by default. Optional
HTTP(S) forwarding can export the same metadata-only ledger events, but remote
systems do not make kill decisions.
