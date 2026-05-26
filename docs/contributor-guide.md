# Curb Contributor Guide

Curb is intentionally shaped around a deep portable core and thin platform adapters.

## Architecture

- `cmd/curb`: CLI commands and user-facing command composition.
- `internal/api`: token-gated localhost HTTP adapter for UI and CLI clients.
- `internal/config`: strict YAML schema, defaults, validation, and policy merging.
- `internal/usage`: metadata-only agent usage readers, provider-neutral token summaries, and shared durable parse cache for repeated scans.
- `internal/usagewatch`: usage policy evaluation, process correlation, session acknowledgements, warnings, and usage-based enforcement.
- `internal/service`: daemon orchestration, config updates, snapshot cache, usagewatch loop ownership, ledger reads, and stable UI/API read models.
- `internal/watchdog`: run lifecycle, matching, warnings, ack handling, policy evaluation, and enforcement orchestration.
- `internal/platform`: real OS process capture, notification, and termination adapters.
- `internal/ledger`: append-only NDJSON ledger with hash chaining.

The strategic boundary is simple: usage facts and provider-file ingestion state
live in `usage`; provider parse state is operational cache under the service
state directory, not audit history; usage policy lives in `usagewatch`; daemon orchestration, config persistence, UI
actionability, session-ack commands, usagewatch loop ownership, and snapshot
caching live in `service`; `api` only serializes and routes through the service
interface; raw ledger structs stay inside the audit-log and service boundary;
legacy process-run policy lives in `watchdog`; OS facts and OS actions live in
`platform`. Provider log parsing should not leak into process enforcement code.
Termination crosses from policy code into OS actions only through
`platform.TerminationTarget`, which is produced by revalidating process identity
against a fresh `platform.Snapshot`; production termination functions must not
accept a bare root PID. Manual session stops live in `internal/service` as a
single `StopSession` use case. UI-supplied PID, start time, owner, and
executable/app fields are confirmation evidence only; authority always comes
from fresh usage ingestion, fresh process capture, service-owned correlation,
watch-only checks, and `TerminationTarget` construction.
Notification health and notification tests are service-owned actions over the
same injected notification boundary used by policy alerts; UI code must not
probe platform notification capabilities directly.
Platform capability reporting is also service-owned. `internal/platform` exposes
OS facts and actions; `internal/service` composes those facts with config and
current snapshots into UI/API capability views. React must render those views
instead of branching on OS names, mode strings, or raw process fields.
Snapshot-to-snapshot UI deltas are computed in `service` at the snapshot cache
boundary, where the previous and next service read models are both available;
frontend code should render those deltas instead of diffing sessions, turns, or
agents itself.

## Testing Discipline

Do not use internal mocks for Curb's core behavior. Tests should prefer:

- real temp config files,
- real temp ledgers,
- real harmless subprocesses such as `sleep`,
- deterministic fake `platform.Snapshot` values for policy/state-machine tests,
- injected platform boundary functions only when the real OS action would be nondeterministic or harmful.

The injected boundary functions are not mocks of the domain. They are substitutes for the operating system edge: process capture, notification delivery, and termination.

## Commands

```sh
gofmt -w cmd/curb internal/config internal/ledger internal/platform internal/service internal/usage internal/usagewatch internal/watchdog
scripts/validate.sh
scripts/build-ui.sh
scripts/build-ui.sh --check
go test ./...
go vet ./...
go test -race ./...
cd ui && npm run typecheck
cd ui && npm run lint
cd ui && npm test -- --run
go test ./... -coverprofile=/tmp/curb.cover
go tool cover -func=/tmp/curb.cover
GOOS=linux GOARCH=amd64 go build -o /tmp/curb-linux ./cmd/curb
GOOS=windows GOARCH=amd64 go build -o /tmp/curb-windows.exe ./cmd/curb
go build -o /tmp/curb-darwin ./cmd/curb
```

`internal/web/dist` is the Go embed source for `curb app`. After changing
`ui/src`, run `scripts/build-ui.sh`; it builds `ui/dist` and copies the result
into `internal/web/dist`. `scripts/build-ui.sh --check` performs a fresh
temporary Vite build and fails if the embedded assets are stale. `scripts/validate.sh`
runs the product gate (`build-ui --check`, Go tests, Go vet, UI typecheck,
UI lint, and UI tests).

## Design Rules

- Keep config strict. Unknown keys should fail validation.
- Keep enforcement conservative. Visibility and alert modes must never terminate processes.
- Keep privacy defaults hard. Prompt or response content capture is rejected by config validation.
- Keep usage readers metadata-only. Tests should prove token extraction without requiring prompt or response content.
- Prefer stable app identity over process names when available.
- Treat PID plus process start time as the identity for termination safety.
  Platform capture should keep a process when PID and start time are available,
  even if name, executable path, or command line are temporarily unavailable.
- Add platform-specific behavior behind platform files or adapter functions, not inside policy logic.

## Adding A New Agent Default

1. Add a matcher to `configs/curb.example.yaml`.
2. Prefer bundle/signing/path identity over command-line regex.
3. Add excludes for known helper, renderer, updater, and crash-handler processes.
4. Add or update tests for matcher scoring and exclusions.
5. Document any verified identifiers in `SPEC.md` if they came from a real machine.
