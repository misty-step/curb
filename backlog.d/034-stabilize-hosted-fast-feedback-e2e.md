# Stabilize hosted fast-feedback enforcement proof

Priority: P0
Status: complete
Estimate: M

## Goal
Make the hosted `fast feedback (ubuntu)` lane trustworthy again by fixing the
Linux enforcement E2E failure without weakening process-termination safety or
removing the gate.

## Oracle
- [x] Reproduce or explain the latest hosted failure from run `27037199553`, job
      `79804074637`: `terminated_session_is_not_rekilled_on_the_next_scan`
      ended with `usage_termination_failed` and `worker pid ... was not
      terminated on scan 2`.
- [x] Preserve the local macOS contrast: `cargo test -p curb-core --test
      e2e_enforcement -- --nocapture` passes or any local failure is captured
      with diagnostics before code changes.
- [x] Root cause whether the Ubuntu failure is product behavior,
      shell/process-tree semantics, process-capture liveness, test timing, or CI
      environment drift.
- [x] Fix the product or the test harness at the root cause while preserving the
      invariant that production termination APIs never accept a bare PID.
- [x] Keep `scripts/check-fast.sh` running the Rust workspace tests; do not skip
      `curb-core/tests/e2e_enforcement.rs` from the fast gate to get green.
- [x] Capture a passing hosted `fast feedback (ubuntu)` run on `master` or a PR
      branch, plus the full `scripts/validate.sh` local proof.

## Children
1. Preserve the failing GitHub log excerpt and rerun the focused test locally with `--nocapture`.
2. Add or tighten diagnostics around Linux shell worker termination so
   `usage_termination_failed` identifies the OS result, process tree, and target
   identity.
3. Fix the termination or E2E harness behavior and mutation-check that the test
   still catches a missed second-scan stop.
4. Rerun local focused, local full, and hosted fast/full gates.

## Notes
**Why:** Harness/verification perspective. Live `gh run list` shows the newest
`master` CI run is red even though most hosted jobs pass. The failing log points
at `curb-core/tests/e2e_enforcement.rs:490`, while the same focused test passed
locally during this groom.

Do not turn this into a broad CI cleanup. The only acceptable outcome is a green
hosted fast lane with the enforcement proof still meaningful.

## Delivery Notes

June 11, 2026:
- Preserved the hosted failure receipt from run `27037199553`, job
  `79804074637`: `terminated_session_is_not_rekilled_on_the_next_scan` failed
  because the worker stayed alive and the ledger ended at
  `usage_termination_failed`.
- Preserved the local macOS contrast before edits:
  `cargo test -p curb-core --test e2e_enforcement -- --nocapture` passed with
  both real subprocess E2E tests green.
- Root cause: CI exposed a Linux process-capture timing race that was also
  reachable in production. A worker could appear in the process table before
  sysinfo exposed enough PID/start/owner/executable evidence to seal a
  grace-time stop token; the later stop correctly rejected the incomplete
  identity instead of killing by bare PID, but the stale unsealable token could
  be retried.
- Fix: local policy sessions now require a sealable process identity before
  advertising/storing a stop token, so grace cannot start on an unsealable
  process. The E2E worker observation loop mirrors that boundary by waiting for
  a revalidatable `TerminationTarget` before starting policy scans, and
  diagnostics now print termination identity and target scope.
  `usage_termination_failed` also carries the concrete rejection reason from the
  enforcer.
- Focused local proof after edits:
  `cargo test -p curb-core usagewatch::tests::pid_reuse_at_kill_time_records_termination_failed -- --nocapture`
  passed; `cargo test -p curb-core --test e2e_enforcement -- --nocapture`
  passed; `cargo test -p curb-core -- --nocapture` passed with 128 unit tests
  plus 2 E2E tests.
- Local gate proof: `scripts/check-fast.sh` passed with the Rust workspace tests
  still enabled; `scripts/validate.sh` passed, including UI typecheck/lint/unit
  tests, dashboard smoke, Tauri checks, and demo 006 dry run.
- Fresh-context peer review: Claude initially found the production-reachable
  incomplete-identity blocker; after the local stop-token sealability gate was
  added, re-review returned `NO BLOCKERS`.
- Hosted PR run `27373609901` moved the failure forward: the original
  incomplete-token rejection was gone, but `fast feedback (ubuntu)` still failed
  because scan 2 captured `can_terminate=false` and emitted
  `usage_kill_blocked` while a later diagnostic snapshot showed the same worker
  had become sealable. The product fix now drops rejected grace-time tokens so
  stale seals cannot be retried forever, and the E2E harness waits for stable
  sealed identity before asserting the kill lifecycle.
- Hosted proof: PR run `27374332183` passed `fast feedback (ubuntu)` in 1m47s,
  `full validate (ubuntu-latest)` in 4m0s, `full validate (macos-latest)` in
  3m41s, `windows smoke` in 1m44s, `coverage` in 1m12s, `dependency audit` in
  24s, and CodeRabbit review completed with pass.
