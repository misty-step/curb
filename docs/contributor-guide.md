# Curb Contributor Guide

Curb is intentionally shaped as one deep local endpoint agent with thin clients
and thin OS adapters.

## Architecture

- `src/main.rs`: CLI command composition and local app launch.
- `src/cli.rs`: first-run config, install, config presets, and compact terminal
  commands.
- `src/config.rs`: strict YAML schema, defaults, validation, and policy merging.
- `src/usage.rs`: metadata-only agent usage readers, provider-neutral token
  summaries, provider scan errors, and durable parse cache.
- `src/usagewatch.rs`: usage policy evaluation, warnings, acknowledgements,
  grace windows, notification state, and usage-based enforcement.
- `src/service.rs`: stable UI/API read models, session/process correlation,
  actionability, stop-session revalidation, and acknowledgement projection.
- `src/runtime.rs`: daemon orchestration, config updates, snapshot cache,
  notification health, scan ownership, and shared API state.
- `src/platform.rs`: real OS process capture, notification, and sealed
  termination-target construction.
- `src/ledger.rs`: append-only NDJSON ledger with hash chaining, metadata
  enrichment, redaction, and append hooks.
- `src/api.rs`: token-gated loopback HTTP adapter for UI and CLI clients.
- `src/web.rs`: embedded dashboard assets only.

The strategic boundary is simple: one machine has one service authority.
Usage facts and provider-file ingestion state
live in `usage`; provider parse state is operational cache under the service
state directory, not audit history; usage policy lives in `usagewatch`; daemon
orchestration, config persistence, UI actionability, session-ack commands,
usagewatch loop ownership, and snapshot caching live in `runtime` and
`service`; `api` only serializes and routes through the service
interface; raw ledger structs stay inside the audit-log and service boundary;
legacy process-run policy lives in `watchdog`; OS facts and OS actions live in
`platform`. Optional ledger export is service-owned and must not move HTTP,
credential, retry, or remote-policy behavior into `ledger`, `usagewatch`, or
`platform`. Provider log parsing should not leak into process enforcement code.
Termination crosses from policy code into OS actions only through
`platform.TerminationTarget`, which is produced by revalidating process identity
against a fresh `platform.Snapshot`; production termination functions must not
accept a bare root PID. Manual session stops live in `src/service.rs` as a
single `StopSession` use case. UI-supplied PID, start time, owner, and
executable/app fields are confirmation evidence only; authority always comes
from fresh usage ingestion, fresh process capture, service-owned correlation,
watch-only checks, and `TerminationTarget` construction.
Notification health and notification tests are service-owned actions over the
same injected notification boundary used by policy alerts; UI code must not
probe platform notification capabilities directly.
Platform capability reporting is also service-owned. `src/platform.rs` exposes
OS facts and actions; `src/service.rs` composes those facts with config and
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
scripts/check-setup.sh
scripts/install-git-hooks.sh
scripts/check-fast.sh
scripts/validate.sh
scripts/build-ui.sh
scripts/build-ui.sh --check
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo test -- --nocapture
cargo build --release --bin curb
bash demo/006/script/run-backlog-006-demo.sh --dry-run
bash demo/006/script/run-backlog-006-demo.sh --mode all
cd ui && npm run typecheck
cd ui && npm run lint
cd ui && npm test -- --run
cd ui && npm run smoke
```

`web/dist` is the committed embed source for `curb app`. After
changing `ui/src`, run `scripts/build-ui.sh`; it builds `ui/dist` and copies
the result into `web/dist`. `scripts/build-ui.sh --check` performs a
fresh temporary Vite build and fails if the embedded assets are stale.
`scripts/check-setup.sh` is the local environment smoke: it checks Rust, Node,
locked dependencies, installed UI dependencies, the embedded UI directory, and
the example config, and syntax-checks the repo-managed Git hook scripts.
`scripts/install-git-hooks.sh` installs the versioned
`scripts/git-hooks/pre-commit` template into the current checkout's Git hook
directory. It uses `git rev-parse --git-path hooks/pre-commit`, so it works in
linked worktrees as well as normal clones. It refuses to replace an existing
non-Curb hook unless rerun with `--force`, because global or shared hook
installations are user-owned state. The hook runs `scripts/check-fast.sh`
before commit; set `CURB_SKIP_PRE_COMMIT=1` only for emergency state
preservation and record the reason in the handoff.
`scripts/check-fast.sh` is the fastest high-signal product
gate (`build-ui --check`, Rust fmt, clippy, file-length ratchet,
termination-boundary scan, offline secret scan, Rust tests, UI typecheck, UI
lint, UI tests, and deterministic dashboard browser smoke).
`scripts/validate.sh` is the full pre-merge gate and adds the desktop shell
checks plus the synthetic demo dry-run. `cd ui && npm run smoke` runs the
Playwright dashboard smoke directly against the Vite app with `/v1/*` fulfilled
from committed fixtures in `contracts/api/`. It writes screenshot and manifest
artifacts under `ui/artifacts/smoke-dashboard` by default, or under
`CURB_SMOKE_ARTIFACTS` when that environment variable is set. Set
`CURB_SMOKE_URL` to smoke a live Curb endpoint instead of the local
fixture-backed Vite server.

Shared API/UI contract fixtures live in `contracts/api/`. Rust API tests compare
the current route responses against those fixtures, and UI tests import the same
files through TypeScript read-model types. When a wire contract intentionally
changes, update the Rust route behavior, update the matching fixture in
`contracts/api/`, update `ui/src/types.ts` if the UI-facing type changed, then
run:

```sh
cargo test --bin curb contract -- --nocapture
cd ui && npm test -- --run contract
```

## Agent Readiness Contract

The durable readiness profile lives at `.harness-kit/agent-readiness.yaml`.
Update it when local gates, CI gates, module boundaries, observability signals,
or approved waivers change. The profile is validated with:

```sh
<agent-readiness-skill>/scripts/profile-crud.py validate
```

The current gate ladder is:

1. `scripts/check-setup.sh` for local prerequisites and example config health.
2. `scripts/install-git-hooks.sh` once per checkout to install the
   repo-managed pre-commit hook.
3. `scripts/check-fast.sh` for fast code, type, lint, stale embed, secret,
   rendered UI, and test regressions.
4. `scripts/validate.sh` for the full local pre-merge gate.

Gate policy:

- Mandatory before merge: `scripts/validate.sh` locally and the GitHub
  `full validate` workflow on Linux/macOS.
- Mandatory fast CI feedback: the GitHub `fast feedback` job runs
  `scripts/check-fast.sh` on Ubuntu so agents get a named, high-signal lane
  before the slower full matrix and demo dry-run finish. This includes the
  offline secret scan and deterministic dashboard browser smoke.
- Mandatory CI smoke: the GitHub `windows smoke` job compiles the Rust binary,
  validates `configs/curb.example.yaml`, checks notification capability
  behavior, and checks the Windows `taskkill.exe` termination-command contract.
- Mandatory CI advisory audit: the GitHub `dependency audit` job runs
  `scripts/check-dependency-audit.sh --online`, which refreshes RustSec data
  through `cargo audit` and checks the UI lockfile with `npm audit`.
- Mandatory before editing a fresh checkout: `scripts/check-setup.sh`, because
  missing UI dependencies or stale toolchains waste agent time.
- Recommended once per checkout: `scripts/install-git-hooks.sh`, because it
  turns `scripts/check-fast.sh` into pre-commit feedback without adding a new
  hook framework.
- Recommended inner loop: `scripts/check-fast.sh` after each coherent Rust/UI
  slice and before asking for review. For UI-only iteration, run
  `cd ui && npm run smoke` directly to refresh the artifact-backed screenshots
  before the full fast gate.

Do not mark a readiness item complete unless the relevant gate or artifact is
named in the profile and the command has been exercised on the current tree.

Durable architecture decisions live in `docs/adr/`. Add or update an ADR when a
decision is hard to reverse, surprising without context, and backed by a real
tradeoff. Current accepted decisions cover the headless service contract,
structured observability contract, and termination-boundary safety. Operational
runbooks live in `docs/runbooks/`; they should contain commands and triage
steps, not duplicate the decision rationale from ADRs.

Dependency and security updates should preserve existing package managers and
lockfiles. Use Cargo/Cargo.lock for Rust crates, `npm`/`package-lock.json` for
the UI, and the existing Tauri lockfile for the desktop shell. Security updates
must pass `scripts/validate.sh` and must not weaken privacy or termination
invariants to satisfy a dependency change.

`scripts/check-dependency-audit.sh --offline` runs a cache-backed RustSec audit
for local inner loops when `cargo-audit` is installed. `--online` is the
mandatory CI shape and should be used before dependency PRs; it intentionally
keeps registry-backed npm audit out of local `scripts/validate.sh`.
Waivers require the advisory id, affected package, reason, owner, expiry date,
and compensating control in the backlog item or PR that keeps the dependency.

`python3 scripts/check-secrets.py` scans tracked and untracked non-ignored text
files for high-confidence secrets. It is intentionally offline and narrow:
synthetic test tokens are fine, but private key blocks and real-looking API keys
block the fast and full gates.

## Design Rules

- Keep config strict. Unknown keys should fail validation.
- Keep enforcement conservative. Visibility and alert modes must never terminate processes.
- Keep privacy defaults hard. Prompt or response content capture is rejected by config validation.
- Keep usage readers metadata-only. Tests should prove token extraction without requiring prompt or response content.
- Keep remote systems advisory. They may receive metadata-only events, but
  policy evaluation and enforcement authority stay on the endpoint.
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
