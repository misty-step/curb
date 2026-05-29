# Assert enforcement decisions via the ledger, not just the body count

Priority: P0
Status: ready
Estimate: M

## Goal
Extend the `UsageWatch::scan` tests so every refusal and lifecycle branch asserts the *emitted ledger event*, proving Curb decided correctly — not merely that some process died.

## Non-Goals
- Real OS process termination (that is the E2E ticket, 003).
- Changing enforcement logic; this is test coverage of existing behavior.

## Oracle
- [ ] Tests cover all six scenarios, each asserting the exact `event_type` in a real temp ledger AND the expected `platform.terminated` pid set:
  - uncorrelated-over-kill → `usage_kill_blocked`
  - watch-only / supervised-over-kill → `usage_kill_blocked`
  - alert-mode-over-kill → `usage_would_terminate`
  - safety-guard-rejected (PID identity mismatch at kill time) → `usage_termination_failed`
  - killed-then-resumed → re-armed and re-killed on the next scan
  - kill aged out of window → row drops, no re-kill
- [ ] Deleting any single refusal branch in `usagewatch.rs` turns at least one test red.
- [ ] `cargo test` passes; new tests use the AGENTS idiom (real temp ledger + fake `KillPlatform`).

## Notes
**Why (Beck):** `usagewatch.rs` has 5 distinct refuse/blocked outcomes (`src/usagewatch.rs:111-232`) but all 9 existing assertions check only `platform.terminated` / `terminated_keys()` (`src/usagewatch.rs:515-755`). The branches that decide *not* to kill — or to kill again after resume — are exactly where this tool harms a user, and a future refactor could turn "blocked, no correlation" into "killed the wrong PID" with every test still green.
- The terminated-state lifecycle (`src/usagewatch.rs:56-76`) is half-tested today: kill-once is covered, resume/re-arm and window age-out are not.
