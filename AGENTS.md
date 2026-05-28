# Curb Repo Guide

This repository is being rewritten from Go + React to Rust + React. Shared
agent routing, provider rosters, skills, and harness behavior come from the
system Spellbook configuration, not repo-local harness projections.

## Repo Signals

- Product: local AI-agent visibility and watchdog tool.
- Backend: Rust rewrite in progress; Go remains the behavior oracle until the
  Rust implementation fully replaces it.
- Frontend: React/Vite, embedded into `internal/web/dist`.
- Primary docs: `README.md`, `SPEC.md`, `docs/contributor-guide.md`,
  `docs/user-guide.md`, `docs/application-design.md`.

## Verification

Use `scripts/validate.sh` as the local pre-merge gate. It runs:

- `scripts/build-ui.sh --check`
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- `go test ./...`
- `go vet ./...`
- `cd ui && npm run typecheck`
- `cd ui && npm run lint`
- `cd ui && npm test -- --run`

Useful focused checks:

- `go test -race ./...`
- `go build -o /tmp/curb-darwin ./cmd/curb`
- `GOOS=linux GOARCH=amd64 go build -o /tmp/curb-linux ./cmd/curb`
- `GOOS=windows GOARCH=amd64 go build -o /tmp/curb-windows.exe ./cmd/curb`
- `cargo build`
- `cargo run -- validate-config configs/curb.example.yaml`
- `/tmp/curb-darwin validate-config configs/curb.example.yaml`
- `/tmp/curb-darwin scan --config configs/curb.example.yaml`

## Invariants

- Visibility and alert modes must never terminate processes.
- Prompt, response, screenshot, keystroke, and file-content capture are rejected
  by default.
- PID plus process start time is the identity boundary for termination safety.
- In Rust, policy/state must live behind deep service/domain boundaries; OS
  facts and actions live behind `src/platform.rs`.
- Desktop app roots are not enforcement targets. Only correlated worker or CLI
  processes may be stopped.
- Rust termination APIs must never accept a bare PID. They accept only a sealed
  termination target built from PID plus process start time, owner, and
  executable/app identity.

## Module Boundaries

- `cmd/curb` owns CLI composition only.
- `src/main.rs` owns Rust CLI composition only.
- `src/config.rs` owns strict YAML loading, defaults, validation, and policy
  merging for the Rust rewrite.
- `src/ledger.rs` owns append-only NDJSON ledger writes and reads for the Rust
  rewrite.
- `src/platform.rs` owns Rust process identity and termination-target safety.
- `internal/config` owns strict YAML loading, defaults, validation, and policy
  merging.
- `internal/service` owns daemon orchestration, config updates, snapshot cache,
  UI/API read models, usagewatch loop ownership, and actions.
- `internal/usage` owns metadata-only provider log readers and durable parse
  cache.
- `internal/usagewatch` owns usage policy evaluation, session/process
  correlation, acknowledgement, warnings, and usage-based enforcement.
- `internal/watchdog` owns legacy process-run matching and duration policy.
- `internal/platform` owns real OS process capture, notifications, and
  termination.
- `internal/ledger` owns append-only NDJSON ledger writes and reads.

## Test Idioms

- Prefer real temp config files, real temp ledgers, real harmless subprocesses,
  and deterministic `platform.Snapshot` fixtures.
- Mock only external OS boundaries such as notifications or termination where a
  real action would be nondeterministic or harmful.
- Keep UI logic in tested selectors/read models rather than ad hoc component
  branches.
