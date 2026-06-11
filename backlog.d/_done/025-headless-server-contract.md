# Make headless server mode a first-class contract

Priority: P0
Status: ready
Estimate: M

## Goal

Define and enforce a clear headless/server mode for Curb so it can run beside
server applications and agent workers without relying on desktop UI assumptions.

## Context

Headless integration is the likely path for Olympus and other server-side agent
runtimes. Today `curb serve` can run without a browser, but the product contract
is still described through app/dashboard language and the API does not separate
liveness, readiness, web serving, and runtime authority as explicitly as a
server integration needs.

## Oracle

- [x] Add an explicit headless command/config contract, such as
      `curb serve --headless`, or document why the existing `curb serve` is the
      complete headless contract.
- [x] `curb serve --help` or `curb serve --headless --help` exposes the contract
      without requiring source inspection.
- [x] In headless mode, startup never opens a browser, never depends on Tauri,
      and does not require embedded UI assets for the API/runtime surface.
- [x] Add distinct liveness and readiness endpoints with JSON response bodies.
      Readiness must report config, ledger, usage reader, platform capability,
      and watcher/runtime state using stable reason codes.
- [x] Root/web routes in headless mode either do not serve UI or return an
      explicit JSON/HTTP response explaining that the server is headless.
- [x] Tests prove headless mode preserves existing safety invariants: loopback
      binding only, API token auth for protected routes, unsafe-method same-origin
      checks, alert/watch modes never terminate, desktop app roots remain
      watch-only, and stop paths still require sealed PID plus process start
      time/owner/executable or app identity.
- [x] Docs cover local sidecar usage for headless agent hosts and name which
      process actions remain forbidden.
- [x] Tests cover both UI-serving mode and headless mode without weakening
      loopback-only and token-auth invariants.

## Non-Goals

- Do not add Olympus-specific dependencies to Curb.
- Do not expose non-loopback service access by default.
- Do not weaken API token auth or termination identity checks.

## Suggested Proof

```sh
cargo run -- serve --headless --addr 127.0.0.1:8765 --config configs/curb.example.yaml
curl -sS http://127.0.0.1:8765/v1/live
curl -sS http://127.0.0.1:8765/v1/ready
curl -i http://127.0.0.1:8765/
cargo test --workspace headless
cargo test --workspace auth
cargo test --workspace termination
scripts/validate.sh
```
