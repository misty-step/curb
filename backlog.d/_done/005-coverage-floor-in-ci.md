# Measure coverage in CI and enforce a calibrated floor

Priority: P2
Status: ready
Estimate: S

## Goal
Generate a coverage report on every PR and fail the build when coverage drops below a calibrated, ratcheting floor.

## Non-Goals
- Chasing 100% — coverage is a regression guard, not the testing strategy (the behaviors in 002–004 are).
- Counting generated code or `main.rs` entry glue against the floor if it distorts the signal.

## Oracle
- [ ] `cargo llvm-cov` runs in CI and uploads/attaches an HTML or summary report artifact.
- [ ] An initial floor is set from the *measured current* line coverage minus a small margin, so the gate passes on creation (record the baseline number in this ticket's commit).
- [ ] A PR that deletes a tested module's tests drops below the floor and fails CI.
- [ ] The floor is documented as ratcheting upward; the value lives in the workflow, not hand-waved.

## Notes
**Why (user seed, re-aimed by the bench):** the seed asked for "coverage reports"; chosen shape is behavior-first + a hard threshold. The threshold guards against regression but is deliberately calibrated to current reality so it never blocks day one and ratchets as 002–004 land.
- Depends on 001 (CI). Order after the behavior tickets so the baseline reflects the new tests.
- 132 Rust `#[test]` exist today; no coverage tooling is configured yet.
