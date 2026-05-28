# Curb Repo Guide

This repository is a Rust + React product. Shared agent routing, provider
rosters, skills, and harness behavior come from the system Spellbook
configuration, not repo-local harness projections.

## Repo Signals

- Product: local AI-agent visibility and watchdog tool.
- Backend: Rust is the primary implementation. Go remains an explicit legacy
  behavior oracle until step 11 of `docs/rust-rewrite.md` is complete.
- Frontend: React/Vite, embedded into the Rust binary from `internal/web/dist`.
- Primary docs: `README.md`, `SPEC.md`, `docs/contributor-guide.md`,
  `docs/user-guide.md`, `docs/application-design.md`.

## Verification

Use `scripts/validate.sh` as the local pre-merge gate. It runs:

- `scripts/build-ui.sh --check`
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- `bash demo/006/script/run-backlog-006-demo.sh --dry-run`
- `cd ui && npm run typecheck`
- `cd ui && npm run lint`
- `cd ui && npm test -- --run`

Useful focused checks:

- `cargo build`
- `cargo test -- --nocapture`
- `cargo build --release --bin curb`
- `cargo run -- validate-config configs/curb.example.yaml`
- `cargo run -- watch --once --config configs/curb.example.yaml`
- `bash demo/006/script/run-backlog-006-demo.sh --mode all`
- `scripts/validate-go-oracle.sh`

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

- `src/main.rs` owns Rust CLI composition only.
- `src/config.rs` owns strict YAML loading, defaults, validation, and policy
  merging for the Rust rewrite.
- `src/ledger.rs` owns append-only NDJSON ledger writes and reads for the Rust
  rewrite.
- `src/platform.rs` owns Rust process identity and termination-target safety.
- `cmd/curb` and `internal/*` are legacy Go oracle code. Do not add new product
  behavior there unless the matching Rust surface already exists and needs an
  oracle correction.

## Test Idioms

- Prefer real temp config files, real temp ledgers, real harmless subprocesses,
  and deterministic `platform.Snapshot` fixtures.
- Mock only external OS boundaries such as notifications or termination where a
  real action would be nondeterministic or harmful.
- Keep UI logic in tested selectors/read models rather than ad hoc component
  branches.
