# Curb

Curb is a local agent visibility and watchdog tool for AI-assisted engineering
work.

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

## Go Implementation

Build and run:

```sh
go test ./...
go build ./cmd/curb
./curb install
./curb init
curb
```

Useful commands:

```sh
./curb config
./curb dashboard
./curb app
./curb daemon
./curb usage --since 24h
./curb tail
./curb config aggressive
./curb config reasonable
./curb runs
```

The normal product surface is intentionally small: configure Curb, then run
Curb. Advanced inspection commands are available through `./curb help advanced`.

`curb dashboard` shows live agent workers and recent usage in one terminal view.
`curb app` serves the built dashboard and opens it in your browser. If a
compatible daemon is already running on the configured loopback address, it
opens that dashboard instead of asking you to manage ports or paste tokens.
`curb daemon` serves the localhost API and dashboard on loopback for advanced
clients and service-style launches.
`curb usage` reads local Codex and Claude metadata logs and summarizes sessions,
models, and token usage without printing or storing prompt or response content.
`curb tail` streams new local usage events.

The built UI is embedded in the Go binary. `curb app` is the normal launch path;
`cd ui && npm run dev` is only needed while developing the frontend.

`curb watch` is usage-first when usage monitoring is enabled. It warns on recent
token activity that crosses the configured latest-turn limit, and in enforcement
mode it stops only a correlated live agent process.

The generated default config watches agent worker processes such as Codex
Desktop workers, Codex CLI, Claude Code, and Anti-Gravity's `agy` CLI. Desktop
applications such as Codex Desktop and Claude Desktop are not enforcement
targets; Curb will not terminate the app root.
