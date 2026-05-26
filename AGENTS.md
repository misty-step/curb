# Gradient Repo Harness

This repository is Gradient-managed at adoption level 'evidence' with profile 'solo-frontier'.

## Repo Signals

- Repository: 'curb'
- Languages: go
- Package manifests: go.mod
- Docs: README.md, docs
- CI/automation: -
- Harness strategy: gradient-native

## Agent Workflow

1. Read the repo docs listed above before changing product code.
2. Use the product verification commands below when they apply:

- `go test ./...`
- `go test -race ./...`
- `go build -o /tmp/curb-darwin ./cmd/curb`
- `GOOS=linux GOARCH=amd64 go build -o /tmp/curb-linux ./cmd/curb`
- `GOOS=windows GOARCH=amd64 go build -o /tmp/curb-windows.exe ./cmd/curb`
- `/tmp/curb-darwin validate-config configs/curb.example.yaml`
- `/tmp/curb-darwin scan --config configs/curb.example.yaml`
3. Run 'gradient resolve' and 'gradient validate' to verify the Gradient harness
   projection. This is not a substitute for the product gate.

## Documented Invariants

- Visibility and alert modes must never terminate processes.
- Prompt, response, screenshot, keystroke, and file-content capture are rejected
  by default.
- PID plus process start time is the identity boundary for termination safety.
- Policy and state live in `internal/watchdog`; OS facts and actions live in
  `internal/platform`.

## Module Boundaries

- ``docs/contributor-guide.md` - contributor architecture and testing guide.`
- `internal/config` owns strict YAML loading, defaults, validation, and policy
  merging.
- `internal/watchdog` owns matching, run lifecycle, warnings, acknowledgements,
  and enforcement orchestration.
- `internal/platform` owns real OS process capture, notifications, and
  termination.
- `internal/ledger` owns append-only NDJSON ledger writes and reads.
- `cmd/curb` owns CLI composition only.

## Narrow Test Idioms

- ``docs/contributor-guide.md` - contributor architecture and testing guide.`
- `go test ./...`
- `/curb validate-config configs/curb.example.yaml`
- `matched process trees after warning, kill threshold, and grace-period checks.`

## Gradient Contract

Gradient owns the repo-local harness projection and profile. Existing product
code is repo-owned; initialization logs improvement work instead of silently
editing product implementation.
