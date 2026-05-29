# High-signal strict-lint pass (clippy + ESLint)

Priority: P3
Status: ready
Estimate: M

## Goal
Tighten linting beyond the current `-D warnings` baseline with a curated set of high-signal rules, wired into the gate — without drowning the repo in pedantic noise.

## Non-Goals
- Turning on clippy `pedantic`/`nursery` wholesale (noise > signal).
- Max-lines-per-file (covered by 009).

## Oracle
- [ ] A `clippy.toml` and/or crate-level lint config enables a curated set (e.g. `clippy::complexity`, `clippy::correctness` already on; add selected `clippy::suspicious`/cognitive-complexity) with documented rationale; `cargo clippy --all-targets -- -D warnings` stays green.
- [ ] ESLint config adds complexity/`max-depth`/`max-params`-style rules for `ui/src`; `npm run lint` stays green.
- [ ] Both run inside `scripts/validate.sh` and therefore in CI (001).
- [ ] Each enabled rule has a one-line justification in the config or this ticket.

## Notes
**Why (user seed, scoped by Carmack):** strict linting is wanted, but a process-killer's value is correctness, not lint-rule count. Enable rules that catch real defects (complexity, suspicious patterns), skip stylistic pedantry that generates churn. Land after the decomposition tickets so new rules judge the intended structure.
