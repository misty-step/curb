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
- [ ] Capture operator notes: install friction, startup behavior, UI clarity,
      usage source fidelity, notification behavior, false positives/negatives,
      and any process-correlation surprises.
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
