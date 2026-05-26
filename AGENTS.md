# Curb Repo Guide

This repository is a Go + React product. Shared agent routing, provider
rosters, skills, and harness behavior come from the system Spellbook
configuration, not repo-local harness projections.

## Repo Signals

- Product: local AI-agent visibility and watchdog tool.
- Backend: Go.
- Frontend: React/Vite, embedded into `internal/web/dist`.
- Primary docs: `README.md`, `SPEC.md`, `docs/contributor-guide.md`,
  `docs/user-guide.md`, `docs/application-design.md`.

## Verification

Use `scripts/validate.sh` as the local pre-merge gate. It runs:

- `scripts/build-ui.sh --check`
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
- `/tmp/curb-darwin validate-config configs/curb.example.yaml`
- `/tmp/curb-darwin scan --config configs/curb.example.yaml`

## Invariants

- Visibility and alert modes must never terminate processes.
- Prompt, response, screenshot, keystroke, and file-content capture are rejected
  by default.
- PID plus process start time is the identity boundary for termination safety.
- Policy and state live in `internal/watchdog` and `internal/usagewatch`; OS
  facts and actions live in `internal/platform`.
- Desktop app roots are not enforcement targets. Only correlated worker or CLI
  processes may be stopped.

## Module Boundaries

- `cmd/curb` owns CLI composition only.
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
