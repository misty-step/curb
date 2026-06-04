# Readiness Tranche Closeout Map

Date: 2026-06-04
Branch: `agent-readiness-closeout`

This map classifies the current dirty readiness tranche for review and hosted
proof. It is not a merge claim. The worktree is intentionally still dirty until
the branch is split, validated, pushed, and reviewed.

## Review Groups

| Group | Paths | Keep decision | Review concern |
|---|---|---|---|
| Headless service and API shell | `src/api.rs`, `src/api/*`, `src/http.rs`, `src/server_cmd.rs`, `src/main.rs`, `tests/cli.rs` | Keep for review. | Proves headless loopback API, public live/ready, protected routes, auth ordering, headless root behavior, and CLI aliases without weakening token or same-origin safety. |
| Structured observability | `src/observability.rs`, `src/observability/*`, `scripts/parse-observability-smoke.py`, `docs/observability.md`, `docs/runbooks/observability-dogfood.md` | Keep for review. | NDJSON schema, event registry, redaction, request path templating, runtime policy fields, and parser coverage must remain stable. |
| Runtime/readiness deepening | `curb-core/src/runtime.rs`, `curb-core/src/runtime/*` | Keep for review. | Separates cache, config-store, usage-tick, watcher, and readiness behavior behind the `Runtime` facade; hosted CI must prove no lifecycle regression. |
| Config and governance readiness | `curb-core/src/config.rs`, `curb-core/src/config/*`, `.harness-kit/agent-readiness.yaml`, `.editorconfig`, `SECURITY.md`, `.github/CODEOWNERS`, `README.md`, `docs/contributor-guide.md` | Keep for review. | Makes setup/gates/security policy durable; review for stale waivers, over-specific local paths, and no new toolchain churn. |
| Quality gates and CI | `.github/workflows/ci.yml`, `scripts/check-fast.sh`, `scripts/check-setup.sh`, `scripts/check-secrets.py`, `scripts/check-termination-boundary.sh`, `scripts/check-dependency-audit.sh`, `scripts/install-git-hooks.sh`, `scripts/git-hooks/pre-commit`, `scripts/validate.sh` | Keep for review. | Defines fast feedback, full validation, Windows smoke, advisory audit, coverage, secret scan, and local hook path. Hosted proof is still missing. |
| API/UI contract fixtures | `contracts/api/*.json`, `src/api/tests.rs`, `ui/src/contract.test.ts`, `ui/src/api.test.ts`, `ui/src/types.ts`, `ui/src/readModel.ts` | Keep for review. | Prevents Rust/TypeScript drift. Fixture updates must be intentional and covered by both Rust and UI tests. |
| UI stop-confirmation and smoke | `ui/src/components/dashboard.tsx`, `ui/src/components/sessionActions.tsx`, `ui/src/App.test.tsx`, `ui/scripts/smoke-dashboard.mjs`, `ui/src/styles.css` | Keep for review. | Destructive stop is now armed before POST. Browser smoke must keep desktop/narrow action surfaces inside viewport. |
| Embedded UI assets | `web/dist/index.html`, `web/dist/assets/index-CbWlSMQl.js`, `web/dist/assets/index-D-vLLOwG.css`, deleted old `web/dist/assets/index-DN380y4E.js`, deleted old `web/dist/assets/index-CzY8TPgA.css` | Keep for review. | Generated from the UI tranche. Must be refreshed by `scripts/build-ui.sh` and checked by `scripts/build-ui.sh --check` inside the full gate. |
| Domain/module refactors | `curb-core/src/service.rs`, `curb-core/src/service/*`, `curb-core/src/platform.rs`, `curb-core/src/platform/*`, `curb-core/src/ledger.rs`, `curb-core/src/ledger/*`, `curb-core/src/usage.rs`, `curb-core/src/usage/*`, `curb-core/src/usagewatch.rs`, `curb-core/src/usagewatch/*`, `curb-core/src/write_path.rs`, `curb-core/src/write_path/*`, `curb-core/src/governor.rs` | Keep for review. | Behavior-preserving deep-module work. Review must check public facades stayed smaller and safety invariants still block bare-PID termination. |
| Enforcement E2E diagnostics | `curb-core/tests/e2e_enforcement.rs` | Keep for review. | Local real-process tests have passed, but hosted/macOS behavior and failure artifacts remain important. |
| Backlog, ADRs, and runbooks | `backlog.d/023-033*.md`, `docs/adr/*`, `docs/refactor-map.md`, `docs/agent-readiness-roadmap.md`, `docs/dogfooding.md`, `docs/user-guide.md`, `docs/internal-desktop-app.md`, `docs/runbooks/*` | Keep for review. | Documents why the tranche exists and what remains. Review for stale claims now that Windows smoke is configured but not hosted-proven. |
| Dogfood evidence | `evidence/dogfood/*` | Keep for review for now. | Every current evidence directory has a README. Final PR may choose to keep all evidence, compress the set, or keep only representative run artifacts, but deletion requires preserving acceptance proof elsewhere. |
| Trace and receipts | `.harness-kit/traces/provider-lanes/*`, `.harness-kit/delegation-receipts.jsonl` | Keep for review for now. | Records provider attempts and local proof. Some Claude/native subagent attempts failed or timed out; do not present them as successful reviewer lanes. |
| Dependency graph | `Cargo.toml`, `Cargo.lock` | Keep for review. | Required by new audit/tooling paths. Dependency audit passed locally but hosted audit is still missing. |
| Ignore rules | `.gitignore` | Keep for review. | Should ignore generated local state without hiding evidence or source files. |

## Untracked Artifact Disposition

Every currently untracked path is intentionally kept for review. No untracked
artifact was deleted in this pass.

| Pattern | Disposition | Why it is kept |
|---|---|---|
| `.editorconfig` | Keep. | Establishes editor defaults for the readiness profile and contributor setup. |
| `.github/CODEOWNERS` | Keep. | Governance baseline for future hosted review. |
| `.harness-kit/agent-readiness.yaml` | Keep. | Durable agent-readiness contract and gate inventory. |
| `.harness-kit/traces/provider-lanes/*` | Keep. | Local proof receipts and provider-attempt evidence for this readiness tranche. |
| `SECURITY.md` | Keep. | Security reporting and dependency policy baseline. |
| `backlog.d/024-033*.md` | Keep. | Active shaped backlog path generated by the grooming/readiness pass. |
| `contracts/api/*.json` | Keep. | Canonical API/UI contract fixtures used by Rust and TypeScript tests. |
| `curb-core/src/config/*` | Keep. | Config duration/default/storage/preset/policy-merge/test extraction modules. |
| `curb-core/src/ledger/taxonomy.rs` | Keep. | Ledger event taxonomy and alert/view classification extraction. |
| `curb-core/src/platform/*` | Keep. | Platform capture, notification, sealed target, termination, and behavior-test extraction modules. |
| `curb-core/src/runtime/*` | Keep. | Runtime cache, config-store, readiness, usage-tick, watcher, and behavior-test extraction modules. |
| `curb-core/src/service/*` | Keep. | Service read-model, correlation, delta, event/config/ack, and behavior-test extraction modules. |
| `curb-core/src/usage/*` | Keep. | Usage cache, discovery, event, line-limit, provider orchestration, and behavior-test extraction modules. |
| `curb-core/src/usagewatch/*` | Keep. | Usagewatch event projection and behavior-test extraction modules. |
| `curb-core/src/write_path/*` | Keep. | Ack-store, ledger-event, stop-identity, and behavior-test extraction modules. |
| `docs/adr/*` | Keep. | Accepted decisions for headless service, structured observability, and termination-boundary safety. |
| `docs/agent-readiness-roadmap.md` | Keep. | Current L3/L4 readiness scorecard and ordered work. |
| `docs/observability.md` | Keep. | Structured log schema and redaction/runbook contract. |
| `docs/readiness-tranche-closeout.md` | Keep. | This closeout map. |
| `docs/refactor-map.md` | Keep. | Deep-module milestone map and refactor guardrails. |
| `docs/runbooks/*` | Keep. | Headless sidecar and observability dogfood operating procedures. |
| `evidence/dogfood/*` | Keep for review. | Acceptance evidence for local release, active-session, headless, stop-rejection, observability, and enforcement dogfood. Every current evidence directory has a README. |
| `scripts/check-dependency-audit.sh` | Keep. | Local and CI dependency advisory audit command. |
| `scripts/check-fast.sh` | Keep. | Mandatory fast feedback gate. |
| `scripts/check-secrets.py` | Keep. | Offline high-confidence secret scan used by the gate. |
| `scripts/check-setup.sh` | Keep. | Local setup smoke. |
| `scripts/check-termination-boundary.sh` | Keep. | Static termination-boundary invariant check. |
| `scripts/dogfood-headless-enforcement.sh` | Keep. | Repeatable successful headless enforcement dogfood proof. |
| `scripts/dogfood-headless-observability.sh` | Keep. | Repeatable timed headless observability dogfood proof. |
| `scripts/git-hooks/pre-commit` | Keep. | Repo-managed pre-commit hook template. |
| `scripts/install-git-hooks.sh` | Keep. | Hook installer for local agent/developer feedback. |
| `scripts/parse-observability-smoke.py` | Keep. | NDJSON observability parser and required-field oracle. |
| `src/api/*` | Keep. | API auth, dispatch, routes, wire, token-store, HTTP containers, server front-door, and behavior-test extraction modules. |
| `src/observability.rs` and `src/observability/*` | Keep. | Structured logging facade, schema, and event registry. |
| `src/server_cmd.rs` | Keep. | Serve/app/watch lifecycle and headless/UI selection extraction. |
| `src/usage_cli.rs` | Keep. | Usage/tail CLI presentation extraction. |
| `ui/src/components/sessionActions.tsx` | Keep. | Stop/ack action strip and two-step stop confirmation. |
| `ui/src/contract.test.ts` | Keep. | TypeScript fixture contract test. |
| `web/dist/assets/index-CbWlSMQl.js` and `web/dist/assets/index-D-vLLOwG.css` | Keep. | Refreshed embedded UI assets generated from the current UI tranche. |

## Hosted Proof Required

Local branch proof now exists:

- `scripts/validate.sh` passed on `agent-readiness-closeout` on
  June 4, 2026.
- `scripts/check-dependency-audit.sh --offline` passed against 1105 cached
  RustSec advisories and 187 crate dependencies.
- `bash scripts/check-dependency-audit.sh --online` fetched RustSec, updated
  crates.io, found 0 Rust vulnerabilities, and completed npm audit.
- `python3 /Users/phaedrus/Development/harness-kit/skills/agent-readiness/scripts/profile-crud.py --profile .harness-kit/agent-readiness.yaml validate`
  passed, proving the durable readiness profile is valid.

The tranche still does not reach L4 until these run on the pushed branch:

- `fast feedback (ubuntu)`
- `full validate (ubuntu-latest)`
- `full validate (macos-latest)`
- `windows smoke`
- `dependency audit`
- `coverage`

Local commands to rerun before pushing if the tree changes again:

```sh
scripts/validate.sh
scripts/check-dependency-audit.sh --offline
bash scripts/check-dependency-audit.sh --online
python3 /Users/phaedrus/Development/harness-kit/skills/agent-readiness/scripts/profile-crud.py --profile .harness-kit/agent-readiness.yaml validate
python3 - <<'PY'
import json
from pathlib import Path
for i, line in enumerate(Path('.harness-kit/delegation-receipts.jsonl').read_text().splitlines(), 1):
    json.loads(line)
print(f'valid jsonl: {i} lines')
PY
```

## Split Guidance

If this branch becomes too large for one review, split in this order:

1. Gates/governance/setup profile and CI.
2. API/UI contract fixtures.
3. Headless service plus structured observability.
4. Dogfood scripts and evidence.
5. UI stop-confirmation and smoke.
6. Deep-module refactors.

The split order prioritizes gates and contract oracles before broad refactors,
so later failures are easier to localize.

The concrete pathspec and verification plan for each semantic commit lives in
`docs/readiness-tranche-commit-plan.md`.

## Current Residual Risk

- The branch is local only and has not been pushed.
- The worktree is not clean.
- Hosted Windows smoke and dependency audit have not run.
- The dogfood windows are local and short; no multi-hour deployment run exists.
- Evidence volume is high and should be reviewed intentionally before commit.
