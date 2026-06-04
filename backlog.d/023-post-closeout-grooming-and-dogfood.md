# Post-closeout grooming and dogfood loop

Priority: P1
Status: ready
Estimate: M

## Goal

After the backlog closeout and release dogfood launch, run a proper grooming
session that turns real Curb usage into the next quality roadmap: refactoring,
stronger gates, better cross-platform proof, cleaner module boundaries, and
better product ergonomics.

## Context

Curb is now merged to `master` with the governor core, internal Tauri shell,
Linux/macOS CI validation, and provider-boundary work landed. The next useful
work should come from dogfooding the release build and then grooming with live
evidence rather than adding speculative features.

Olympus integration is likely already viable through the headless governor
shape: Olympus runs agents on Sprites, Sprites are long-lived Linux machines,
and Curb can run headless on Linux. The remaining Olympus work is probably
adapter/orchestration work in Olympus: pull Curb, initialize it, run it on
Sprites, and feed Curb/governor policy sessions from Olympus run state.

## Oracle

- [ ] Dogfood Curb locally from a release build for at least one real work
      session.
- [~] Capture operator notes: install friction, startup behavior, UI clarity,
      usage source fidelity, notification behavior, false positives/negatives,
      and any process-correlation surprises. Current UI note: destructive stop
      actions now require an inline `Confirm stop` step after the identity
      checklist; install friction, notification behavior, false positives, and
      longer real-session notes remain open.
- [x] Capture a longer local headless observability window:
      `evidence/dogfood/2026-06-04-headless-observability-3min/` ran for
      180 seconds in visibility mode and captured 72 NDJSON events, 59 watcher
      ticks, final readiness HTTP 200, no source-health errors, parser
      acceptance, and redaction success.
- [ ] Run a grooming session with fresh evidence from the dogfood run, current
      docs, current CI, and current backlog state.
- [ ] Produce a ranked next backlog with acceptance oracles for:
      cross-platform runtime proof, Windows CI or smoke coverage, release/install
      flow, module-boundary simplification, hardening/property tests, UI/QA
      evidence, and Olympus adapter readiness.
- [ ] Decide whether Curb needs a repo-local QA/dogfood skill or whether the
      existing Harness Kit `/qa`, `/agent-readiness`, `/refactor`, and `/groom`
      skills are sufficient.
- [ ] Keep the invariant that desktop app roots are not enforcement targets and
      Rust termination APIs never accept a bare PID.

## Non-Goals

- Do not expand Curb into an Olympus-specific codebase.
- Do not start a new feature tranche before the first dogfood evidence is
  reviewed.
- Do not weaken the Linux/macOS CI gate or the process-identity safety tests.
- Do not treat a successful release build alone as product acceptance.

## Suggested First Grooming Lanes

- Product dogfood lane: where the local app helps or confuses a real operator.
- Refactor lane: module seams, duplicated policy/read-model logic, and deletion
  opportunities.
- Hardening lane: process identity, zombie/dead process liveness, provider log
  parsing, and API auth edge cases.
- CI/QA lane: Windows proof, Tauri runtime smoke, and user-like UI flows.
- Olympus lane: minimal adapter contract from Sprite/run state to
  `GovernorEngine`.

## Groomed Next Tranche

This pass created the next backlog path in this order:

1. `024-dogfood-evidence-matrix.md` - make dogfooding evidence repeatable before
   new feature work.
2. `025-headless-server-contract.md` - make server/headless mode a first-class
   product contract.
3. `026-structured-observability.md` - first JSON-log slice; continue with
   `032-readiness-latency-and-observability-completion.md` before claiming
   headless readiness is fast.
4. `030-api-ui-contract-drift-guard.md` - lock Rust/TypeScript API contracts
   before broad refactors.
5. `031-fast-feedback-and-cross-platform-gates.md` - split fast/full feedback
   and add Windows proof.
6. `029-agent-readiness-contract.md` - persist governance/setup readiness.
7. `028-deep-module-refactor-path.md` - simplify the broad service/API/runtime
   surfaces without changing behavior.
8. `033-hosted-proof-and-tranche-closeout.md` - convert the dirty local
   readiness tranche into a named branch with full local reruns, hosted
   fast/full/Windows/audit/coverage evidence, and an intentional review shape.

The current agent-readiness snapshot is recorded in
`docs/agent-readiness-roadmap.md`. The roadmap rates the repo as L3
Standardized overall, with L4 blocked by contract drift protection, faster gate
lanes, observability completion, governance basics, and deep-module
extractions.

Provider-roster note: system roster providers were probed, but this grooming
pass used native read-only agents plus local repo evidence because no
repo-local receipt script exists and no implementation patch was requested.
