# Readiness Tranche Commit Plan

Date: 2026-06-04
Branch: `agent-readiness-closeout`

Historical note, 2026-06-12: this is the June 4 readiness tranche staging plan,
not current staging guidance. Current release proof and artifact disposition
live in `docs/release-evidence.md`.

This plan turns the current dirty readiness tranche into reviewable semantic
commits. It is a staging plan, not a staging action. Do not commit these groups
until the operator explicitly approves closeout.

## Commit Order

### 1. Gates, Governance, And Readiness Contract

Intent: make the repository easier for agents and humans to validate before
touching product behavior.

Pathspecs:

```text
.editorconfig
.github/CODEOWNERS
.github/workflows/ci.yml
.gitignore
.harness-kit/agent-readiness.yaml
SECURITY.md
Cargo.toml
Cargo.lock
README.md
docs/contributor-guide.md
scripts/check-fast.sh
scripts/check-setup.sh
scripts/check-secrets.py
scripts/check-termination-boundary.sh
scripts/check-dependency-audit.sh
scripts/install-git-hooks.sh
scripts/git-hooks/pre-commit
scripts/validate.sh
```

Verification before commit:

```sh
scripts/check-setup.sh
scripts/check-fast.sh
scripts/check-dependency-audit.sh --offline
python3 <agent-readiness-skill>/scripts/profile-crud.py --profile .harness-kit/agent-readiness.yaml validate
```

Risk to review: workflow syntax, local path leakage in the profile, and whether
the dependency-audit command remains practical for local and hosted runs.

### 2. API/UI Contract Fixtures

Intent: lock the Rust API and TypeScript read-model contract before broad
module refactors and UI polish.

Pathspecs:

```text
contracts/api/config.json
contracts/api/live.json
contracts/api/onboarding.json
contracts/api/overview.json
contracts/api/ready.json
contracts/api/session.json
contracts/api/snapshot.json
contracts/api/turns.json
src/api/tests.rs
ui/src/api.test.ts
ui/src/contract.test.ts
ui/src/readModel.ts
ui/src/types.ts
```

Verification before commit:

```sh
cargo test --bin curb api_contract_fixtures_match_ui_facing_routes -- --nocapture
cd ui && npm test -- --run contract api
```

Risk to review: fixtures should represent public wire behavior, not internal
implementation detail.

### 3. Headless Service And Structured Observability

Intent: make Curb a server-side/headless sidecar with stable local API probes
and parseable JSON logs.

Pathspecs:

```text
src/api.rs
src/api/auth.rs
src/api/dispatch.rs
src/api/http_types.rs
src/api/response.rs
src/api/routes.rs
src/api/server.rs
src/api/token_store.rs
src/api/wire.rs
src/http.rs
src/main.rs
src/observability.rs
src/observability/event.rs
src/observability/registry.rs
src/server_cmd.rs
tests/cli.rs
docs/adr/0001-headless-service-contract.md
docs/adr/0002-structured-observability-contract.md
docs/adr/README.md
docs/observability.md
docs/runbooks/headless-sidecar.md
docs/runbooks/observability-dogfood.md
scripts/parse-observability-smoke.py
```

Verification before commit:

```sh
cargo test --bin curb headless observability api -- --nocapture
cargo test --test cli serve -- --nocapture
python3 scripts/parse-observability-smoke.py <captured-ndjson>
```

Risk to review: `/v1/live` and `/v1/ready` stay public, protected routes stay
token-gated, unsafe cookie auth stays same-origin-gated, and logs never include
tokens, prompts, responses, file contents, screenshots, or keystrokes.

### 4. Dogfood Scripts, Runbooks, And Evidence

Intent: preserve proof that the headless, observability, rejection, and
enforcement paths work against release binaries and real/synthetic local data.

Pathspecs:

```text
docs/dogfooding.md
docs/user-guide.md
evidence/dogfood/README.md
evidence/dogfood/TEMPLATE.md
evidence/dogfood/2026-06-03-active-agent/
evidence/dogfood/2026-06-03-local-release/
evidence/dogfood/2026-06-04-runbook-headless/
evidence/dogfood/2026-06-04-stop-rejection/
evidence/dogfood/2026-06-04-headless-observability/
evidence/dogfood/2026-06-04-headless-observability-3min/
evidence/dogfood/2026-06-04-headless-observability-30s-oracle/
evidence/dogfood/2026-06-04-headless-enforcement/
scripts/dogfood-headless-observability.sh
scripts/dogfood-headless-enforcement.sh
```

Verification before commit:

```sh
bash -n scripts/dogfood-headless-observability.sh
bash -n scripts/dogfood-headless-enforcement.sh
CURB_DOGFOOD_SECONDS=30 bash scripts/dogfood-headless-observability.sh /tmp/curb-headless-observability-check
```

Risk to review: evidence volume is high. If reducing artifacts, preserve the
README, command output, parser output, redaction check, and final readiness or
stop-decision evidence for each proof class.

### 5. UI Stop Confirmation And Embedded Assets

Intent: make destructive stop actions harder to trigger accidentally and prove
the dashboard still renders inside viewport constraints.

Pathspecs:

```text
ui/src/components/dashboard.tsx
ui/src/components/sessionActions.tsx
ui/src/App.test.tsx
ui/scripts/smoke-dashboard.mjs
ui/src/styles.css
web/dist/index.html
web/dist/assets/index-CbWlSMQl.js
web/dist/assets/index-D-vLLOwG.css
web/dist/assets/index-DN380y4E.js
web/dist/assets/index-CzY8TPgA.css
```

Verification before commit:

```sh
cd ui && npm test -- --run App
cd ui && npm run smoke
scripts/build-ui.sh --check
```

Risk to review: no `/stop` POST should happen before `Confirm stop`, and the
generated `web/dist` assets must match the source UI.

### 6. Deep-Module Refactors

Intent: reduce change amplification while preserving public behavior and safety
invariants.

Pathspecs:

```text
curb-core/src/config.rs
curb-core/src/config/defaults.rs
curb-core/src/config/duration.rs
curb-core/src/config/policy_merge.rs
curb-core/src/config/preset.rs
curb-core/src/config/storage.rs
curb-core/src/config/tests.rs
curb-core/src/governor.rs
curb-core/src/ledger.rs
curb-core/src/ledger/taxonomy.rs
curb-core/src/platform.rs
curb-core/src/platform/capture.rs
curb-core/src/platform/notification.rs
curb-core/src/platform/target.rs
curb-core/src/platform/termination.rs
curb-core/src/platform/tests.rs
curb-core/src/runtime.rs
curb-core/src/runtime/cache.rs
curb-core/src/runtime/config_store.rs
curb-core/src/runtime/readiness.rs
curb-core/src/runtime/tests.rs
curb-core/src/runtime/usage_tick.rs
curb-core/src/runtime/watcher.rs
curb-core/src/service.rs
curb-core/src/service/ack_state.rs
curb-core/src/service/config_model.rs
curb-core/src/service/correlation.rs
curb-core/src/service/delta.rs
curb-core/src/service/events_model.rs
curb-core/src/service/snapshot_model.rs
curb-core/src/service/tests.rs
curb-core/src/usage.rs
curb-core/src/usage/cache.rs
curb-core/src/usage/discovery.rs
curb-core/src/usage/events.rs
curb-core/src/usage/lines.rs
curb-core/src/usage/provider.rs
curb-core/src/usage/tests.rs
curb-core/src/usagewatch.rs
curb-core/src/usagewatch/events.rs
curb-core/src/usagewatch/tests.rs
curb-core/src/write_path.rs
curb-core/src/write_path/ack_store.rs
curb-core/src/write_path/ledger_events.rs
curb-core/src/write_path/stop_identity.rs
curb-core/src/write_path/tests.rs
curb-core/tests/e2e_enforcement.rs
src/usage_cli.rs
docs/adr/0003-termination-boundary-safety.md
docs/refactor-map.md
```

Verification before commit:

```sh
cargo test -p curb-core -- --nocapture
cargo test -p curb-core --test e2e_enforcement -- --nocapture
scripts/check-termination-boundary.sh
```

Risk to review: these are behavior-preserving extractions. Public facades must
stay simpler, production termination must still reject bare PIDs, and tests
must exercise observable behavior rather than internal call counts.

### 7. Backlog, Roadmap, Trace, And Closeout Receipts

Intent: preserve the grooming rationale, proof trail, and remaining hosted
closeout work.

Pathspecs:

```text
backlog.d/023-post-closeout-grooming-and-dogfood.md
backlog.d/024-dogfood-evidence-matrix.md
backlog.d/025-headless-server-contract.md
backlog.d/026-structured-observability.md
backlog.d/027-quality-gates-and-contract-tests.md
backlog.d/028-deep-module-refactor-path.md
backlog.d/029-agent-readiness-contract.md
backlog.d/030-api-ui-contract-drift-guard.md
backlog.d/031-fast-feedback-and-cross-platform-gates.md
backlog.d/032-readiness-latency-and-observability-completion.md
backlog.d/033-hosted-proof-and-tranche-closeout.md
docs/agent-readiness-roadmap.md
docs/readiness-tranche-closeout.md
docs/readiness-tranche-commit-plan.md
docs/internal-desktop-app.md
.harness-kit/delegation-receipts.jsonl
.harness-kit/traces/provider-lanes/
```

Verification before commit:

```sh
python3 - <<'PY'
import json
from pathlib import Path
for i, line in enumerate(Path('.harness-kit/delegation-receipts.jsonl').read_text().splitlines(), 1):
    json.loads(line)
print(f'valid jsonl: {i} lines')
PY
git diff --check
```

Risk to review: receipts must not overstate failed Claude/native-subagent
attempts as successful reviewers, and backlog status must distinguish local
proof from hosted proof.

## Final Branch Gate

After all semantic commits are created, run the full branch proof again:

```sh
scripts/validate.sh
scripts/check-dependency-audit.sh --offline
bash scripts/check-dependency-audit.sh --online
python3 <agent-readiness-skill>/scripts/profile-crud.py --profile .harness-kit/agent-readiness.yaml validate
python3 - <<'PY'
import json
from pathlib import Path
for i, line in enumerate(Path('.harness-kit/delegation-receipts.jsonl').read_text().splitlines(), 1):
    json.loads(line)
print(f'valid jsonl: {i} lines')
PY
git status --short --branch --untracked-files=all
```

Then push `agent-readiness-closeout` and capture hosted evidence for fast
feedback, full Linux/macOS validation, Windows smoke, dependency audit, and
coverage before claiming L4 readiness.
