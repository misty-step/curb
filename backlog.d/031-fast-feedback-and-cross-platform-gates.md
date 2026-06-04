# Fast feedback and cross-platform gates

Priority: P1
Status: ready
Estimate: M

## Goal

Keep `scripts/validate.sh` as the full pre-merge gate while adding faster,
more targeted checks for agent inner loops and a Windows compile/smoke lane for
platform-specific code paths.

## Context

The full gate is valuable but monolithic: UI embed check, rustfmt, clippy,
file-length, full Rust tests, desktop checks, demo dry-run, UI typecheck, lint,
and tests all run in one script. CI runs that script on macOS and Linux, plus a
macOS coverage job. A focused Windows smoke lane now covers Rust compilation,
example config validation, notification capability behavior, and Windows
termination-command construction; the remaining gap is observing that job green
on the hosted runner after the branch is pushed.

## Oracle

- [x] Add `scripts/check-fast.sh` for high-signal local feedback:
      Rust fmt, clippy, file-length, termination-boundary scan, UI typecheck,
      UI lint, UI tests, deterministic browser smoke, and the Rust workspace
      tests including real-process E2E.
- [x] Preserve `scripts/validate.sh` as the full local pre-merge gate and make
      it call or align with the fast gate without duplicating command drift.
- [x] Re-run the full local gate after the readiness/dogfood/UI hardening
      tranche: `scripts/validate.sh` passed on June 4, 2026, including
      `scripts/check-fast.sh`, desktop shell checks, and demo 006 dry-run.
- [x] Split CI into clearly named fast/full jobs, or document why the current
      two-OS full gate is intentionally retained.
- [x] Add a Windows CI job or smoke that proves compile, config validation,
      Windows notification capability behavior, and Windows termination command
      construction.
- [x] Promote `ui/scripts/smoke-dashboard.mjs` into the mandatory fast/full
      gate with deterministic demo data and artifacts on failure.
- [x] Cover the stoppable-row destructive-action state in the deterministic
      smoke: `Stop requires` identity labels, `Stop now`, and action-surface
      overflow on desktop and narrow viewports.
- [x] Document exact local reproduction commands for each gate class.

## Non-Goals

- Do not remove macOS/Linux full validation.
- Do not skip real-process enforcement tests from the full gate.
- Do not add flaky browser checks to the mandatory gate without deterministic
      fixtures and failure artifacts.

## Suggested Proof

```sh
scripts/check-fast.sh
scripts/validate.sh
rg -n "check-fast|Windows|smoke-dashboard|mandatory|advisory" scripts .github docs README.md
```
