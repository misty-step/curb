# Fix usage_termination_started mis-classified as a grace event

Priority: P3
Status: ready
Estimate: S

## Goal
Classify `usage_termination_started` (and legacy `termination_started`) as a termination event in the alert view, not as a grace-period event.

## Non-Goals
- Changing wire/on-disk event_type strings.
- Touching the typed taxonomy's structure (007 already landed it).

## Oracle
- [ ] `LedgerEvent::alert_class()` gives `UsageTerminationStarted`/`TerminationStarted` a termination-appropriate category and label (e.g. "stopping"), not `grace`.
- [ ] A test pins the alert category/label for `usage_termination_started` to the corrected value (none existed — this behavior was untested, which is why the bug was latent).
- [ ] `cargo test` green; dashboard still renders all other events unchanged.

## Notes
**Why (found during 007):** the pre-007 `.contains()` order tested `"started"` before any termination arm, so a kill-in-progress event rendered with category/label `grace` — internally inconsistent with its own correct explanation ("Curb started terminating a correlated worker") and `actionable` flag. 007 faithfully PRESERVED this behavior (with a flag comment at `src/ledger.rs:264-270`) rather than fixing it under cover of a refactor. This ticket is the deliberate, separately-reviewed correction. Low severity (cosmetic dashboard label), hence P3.
- While here, consider tightening `LedgerEvent::alert_class()` to an exhaustive `match` (drop the `_ =>` catch-alls) so the classifier is self-enforcing rather than free-riding on `as_str`/`view_class` exhaustiveness (ousterhout-critic residual note on 007).
