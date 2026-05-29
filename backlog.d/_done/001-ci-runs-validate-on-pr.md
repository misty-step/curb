# Run the validate gate in CI on every PR

Priority: P0
Status: ready
Estimate: S

## Goal
Make `scripts/validate.sh` a required, automated check on every pull request so the existing gate stops being volunteer-only.

## Non-Goals
- Release/packaging automation (separate concern).
- Adding new gate steps — this ticket only *runs the gate that already exists*.
- Coverage thresholds (see 005) or new lint rules (see 009, 010).

## Oracle
- [ ] `.github/workflows/ci.yml` exists and triggers on `pull_request` and on push to `master`.
- [ ] The job runs `scripts/validate.sh` (fmt-check, clippy `-D warnings`, `cargo test`, build-ui `--check`, demo dry-run, UI typecheck/lint/test/build) on both `macos-latest` and `ubuntu-latest`.
- [ ] A branch with a deliberate `cargo fmt` violation produces a red check; reverting it goes green.
- [ ] Branch protection (or a documented step to enable it) requires the check before merge.

## Notes
**Why (Carmack + Explore + Pi, three independent lanes):** the whole gate is currently unenforced — no `.github/workflows`, `.git/hooks` are stock samples, `validate.sh` is referenced only in docs. Coverage and lint tickets are inert until *something* runs them, so this is the foundation for the rest of the backlog.
- The Linux runner cannot exercise process-kill paths the same way macOS can; the live-kill E2E test (003) may need an OS guard. Keep this ticket scoped to running the existing gate.
- Build-ui `--check` requires `web/dist` to match a fresh UI build; ensure the runner installs Node + runs the UI build before the check.
