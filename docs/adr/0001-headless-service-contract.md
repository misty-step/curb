# ADR 0001: Headless Service Contract

Status: accepted

Date: 2026-06-04

## Context

Curb is used both as a local app and as a sidecar-style service for headless
agent hosts. Server integrations need liveness and readiness probes without a
browser, Tauri shell, or embedded dashboard dependency. Protected API routes
still need loopback binding, API-token authentication, and same-origin checks
for unsafe cookie-authenticated requests.

Relevant implementation and tests:

- `src/server_cmd.rs` owns serve/app/watch lifecycle and headless/UI selection.
- `src/api/server.rs` owns the API front-door ordering for UI fallback,
  headless behavior, CORS, public probes, authentication, and unsafe cookie
  same-origin checks.
- `src/api/tests.rs` proves headless root behavior, public probes, protected API
  auth, and UI-serving mode.
- `contracts/api/live.json` and `contracts/api/ready.json` fix the probe wire
  shape for Rust and UI contract tests.

## Decision

`curb serve --headless` is the first-class server contract.

In headless mode, Curb:

- binds only to loopback unless a future explicit contract says otherwise;
- never opens a browser;
- does not require Tauri;
- does not serve the embedded dashboard UI from root/web routes;
- exposes public `GET /v1/live` and `GET /v1/ready` probes;
- keeps protected API routes token-gated;
- keeps unsafe cookie-authenticated methods same-origin protected;
- starts serving before the first usage scan has to finish, reporting
  `degraded` readiness until the runtime is ready.

`/v1/live` answers whether the process is serving. `/v1/ready` answers whether
the service dependencies are ready enough for clients: config, ledger, usage
reader, platform capabilities, and watcher/runtime state. Readiness may return
`degraded` instead of blocking behind a busy runtime cache or slow scan.

## Consequences

Headless integrations can use Curb as a local sidecar for server-hosted agents
without depending on desktop UI behavior. The dashboard remains a client of the
service, not the service contract itself.

Future API work must preserve the public probe/protected-route split. A new
route is not public just because it is useful to automation; it is public only
if it is a liveness/readiness style probe with no sensitive data and no
mutation.

## Verification

```sh
cargo test --bin curb headless -- --nocapture
cargo test --bin curb auth -- --nocapture
cargo test --bin curb contract -- --nocapture
scripts/validate.sh
```

Manual sidecar smoke:

```sh
cargo run -- serve --headless --addr 127.0.0.1:8765 --config configs/curb.example.yaml
curl -fsS http://127.0.0.1:8765/v1/live
curl -fsS http://127.0.0.1:8765/v1/ready
```

## Related

- [Headless Sidecar Runbook](../runbooks/headless-sidecar.md)
- [Observability](../observability.md)
- `backlog.d/025-headless-server-contract.md`
- `backlog.d/032-readiness-latency-and-observability-completion.md`
