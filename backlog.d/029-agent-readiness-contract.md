# Agent-readiness contract and governance baseline

Priority: P1
Status: ready
Estimate: M

## Goal

Make Curb's agent-readiness posture durable: fast local setup checks, explicit
governance files, and a machine-readable readiness contract that future agents
can validate before changing code.

## Context

The current repo is already strong: `scripts/validate.sh` passes, CI runs the
full gate on macOS/Linux, TypeScript is strict, and Rust clippy denies warnings.
The remaining readiness gaps are environmental and governance gaps that make
agent work slower or less predictable: no editor config, no setup smoke, no
repo-local security/reporting policy, no CODEOWNERS, and no persisted readiness
profile.

## Oracle

- [x] Create `.harness-kit/agent-readiness.yaml` or the repo's chosen
      equivalent with current pillar levels, required commands, waiver policy,
      and expiry dates for known gaps.
- [x] Add a setup smoke command that checks Rust toolchain, Node version,
      `ui/node_modules`, and committed UI embed freshness without running the
      full product gate.
- [x] Add editor defaults (`.editorconfig`) that match Rust/TS formatting and
      line-ending expectations.
- [x] Add governance files or documented equivalents: `SECURITY.md`,
      `CODEOWNERS`, and dependency/security update policy.
- [x] Add an offline high-confidence secret scan to the mandatory fast/full
      gate so credentials cannot ride along in tracked or untracked text files.
- [x] Add a mandatory dependency-advisory audit policy for Cargo and npm with a
      deterministic local reproduction command and documented waiver path.
- [x] Re-run the dependency-advisory audit locally after the readiness tranche:
      offline RustSec passed against 1105 cached advisories and 187 crate
      dependencies, online RustSec+npm audit passed, and
      `scripts/check-dependency-audit.sh` is executable for direct local use.
- [x] Add a repo-managed pre-commit hook installer so local checkouts can run
      the fast gate before commit without introducing a new hook framework.
- [x] Add ADR/runbook coverage for major headless, structured-observability,
      and termination-boundary decisions so future agents have durable rationale
      and operational commands.
- [x] Document mandatory, advisory, and manual checks in `README.md` or
      `docs/contributor-guide.md`.
- [x] Re-run the setup smoke and `scripts/validate.sh`.

## Non-Goals

- Do not add a new package manager, formatter, or frontend test framework.
- Do not introduce branch-protection claims without live GitHub evidence.
- Do not weaken any current gate to make setup faster.

## Suggested Proof

```sh
scripts/check-setup.sh
scripts/validate.sh
rg -n "mandatory|advisory|manual|SECURITY|CODEOWNERS|agent-readiness" README.md docs .harness-kit .github
```
