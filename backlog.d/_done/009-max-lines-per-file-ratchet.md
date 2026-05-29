# Add a max-lines-per-file ratchet (Rust + TS)

Priority: P3
Status: blocked
Estimate: S

## Goal
Cap file length with a lint that fails CI above a threshold, set just above the post-decomposition maximum so files can't regress toward god-object size.

## Non-Goals
- Forcing splits that scatter the deep boundaries AGENTS.md mandates — this is a *ratchet to hold* gains from 006–008, not a driver of mechanical splitting.
- A tiny cap that fights the codebase; pick a realistic ceiling.

## Oracle
- [ ] After 006–008 land, the largest `src/*.rs` is measured and the cap is set modestly above it (record the number).
- [ ] CI fails when any `src/*.rs` or `ui/src/**/*.{ts,tsx}` exceeds the cap (Rust via a clippy/CI check or script; TS via ESLint `max-lines`).
- [ ] Adding 200 junk lines to the largest file turns CI red; reverting goes green.
- [ ] The check is wired into `scripts/validate.sh` so local and CI agree.

## Notes
**Why (user seed, re-aimed by Ousterhout + Carmack):** the seed asked for a max-lines lint; the bench agrees the files are too big (service.rs 3,516; usage.rs 1,845; api.rs 1,771) but warns the lint is the wrong *first* instrument — it optimizes a proxy metric and can force bad splits. So this ticket is **blocked on 006–008**: decompose by concern first, then ratchet to prevent regression.
