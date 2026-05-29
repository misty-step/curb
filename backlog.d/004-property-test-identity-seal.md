# Property-test the termination identity seal

Priority: P1
Status: ready
Estimate: M

## Goal
Exhaustively verify that the termination identity seal never approves a kill when any identity facet has mutated, and always approves when all facets match.

## Non-Goals
- Replacing the existing hand-written seal unit tests (keep them as named regressions).
- Testing the OS kill itself (003).

## Oracle
- [ ] A `proptest` (dev-dependency) suite generates randomized `Process` pairs with combinatorial mutation of every identity facet — PID, start time, owner, executable, and where present bundle id / team id.
- [ ] Property: the seal approves termination **iff** all facets match; zero approvals on any single-facet mismatch across ≥10k cases.
- [ ] Property: `None` vs `Some` on any facet never silently counts as a match.
- [ ] `cargo test` passes; the suite runs in CI (001).

## Notes
**Why (Pi):** `same_process_identity` / `Snapshot::termination_target` (`src/platform.rs`, seal logic ~`:602-701`) is the last line of defense before the OS kill command. Current tests cover the happy path and a few edges; property testing surfaces partial-field matches and `Option` interaction bugs that hand-written cases miss.
- Trade-off: adds `proptest` as a dev-dependency. Acceptable for a process-killer's safety boundary; confine it to test code.
- The AGENTS invariant this defends: "Rust termination APIs must never accept a bare PID" — they accept only a sealed target built from PID + start time + owner + executable/app identity.
