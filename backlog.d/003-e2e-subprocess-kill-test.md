# End-to-end real-subprocess enforcement test

Priority: P1
Status: ready
Estimate: M

## Goal
Exercise the full `UsageWatch → Runtime → SystemPlatform` pipeline against real spawned processes, proving Curb kills the exact runaway worker, spares its app-root sibling, and records the lifecycle in the ledger.

## Non-Goals
- Killing real Codex/Claude apps — use harmless local subprocesses only.
- Property-level fuzzing of the identity seal (that is 004).

## Oracle
- [ ] `cargo test --test e2e_enforcement` (or a clearly-named integration test) spawns a real harmless process tree, crosses the kill threshold, and asserts:
  - a warn event fires, then grace elapses, then the correct leaf/tree PID is terminated;
  - a sibling that looks like a desktop app-root is NOT terminated;
  - a session marked terminated is not re-killed on the next scan;
  - ledger events `usage_grace_started → usage_termination_started → usage_termination_completed` match the actual OS outcome.
- [ ] The test is guarded so CI runs it on macOS and Linux (skipped or adapted where the OS kill primitive differs).
- [ ] `cargo test` passes locally and in CI (001).

## Notes
**Why (Carmack + Pi):** the kill-safety unit logic in `platform.rs` is strong but runs only against a fake platform — the actual OS "weapon" plus the capture→grace→kill timing is never exercised end-to-end. The assertion that would embarrass us in production is "did we kill exactly the right thing, never the wrong thing, and prove it." Reuse the live-child fixtures already in `src/platform.rs` tests.
