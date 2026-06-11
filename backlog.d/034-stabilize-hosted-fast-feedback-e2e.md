# Stabilize hosted fast-feedback enforcement proof

Priority: P0
Status: ready
Estimate: M

## Goal
Make the hosted `fast feedback (ubuntu)` lane trustworthy again by fixing the
Linux enforcement E2E failure without weakening process-termination safety or
removing the gate.

## Oracle
- [ ] Reproduce or explain the latest hosted failure from run `27037199553`, job
      `79804074637`: `terminated_session_is_not_rekilled_on_the_next_scan`
      ended with `usage_termination_failed` and `worker pid ... was not
      terminated on scan 2`.
- [ ] Preserve the local macOS contrast: `cargo test -p curb-core --test
      e2e_enforcement -- --nocapture` passes or any local failure is captured
      with diagnostics before code changes.
- [ ] Root cause whether the Ubuntu failure is product behavior,
      shell/process-tree semantics, process-capture liveness, test timing, or CI
      environment drift.
- [ ] Fix the product or the test harness at the root cause while preserving the
      invariant that production termination APIs never accept a bare PID.
- [ ] Keep `scripts/check-fast.sh` running the Rust workspace tests; do not skip
      `curb-core/tests/e2e_enforcement.rs` from the fast gate to get green.
- [ ] Capture a passing hosted `fast feedback (ubuntu)` run on `master` or a PR
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
