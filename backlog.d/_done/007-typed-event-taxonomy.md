# Replace event-type substring sniffing with one typed taxonomy

Priority: P1
Status: ready
Estimate: M

## Goal
Define one typed event taxonomy (enum or single classify table) so emit-side and read-side share exactly one mapping, instead of re-deriving meaning by substring matching.

## Non-Goals
- Changing the ledger's on-disk event-type strings (keep wire compatibility).
- Reworking the alert UI; only the classification source changes.

## Oracle
- [ ] A single `classify(event_type) -> EventClass { category, severity, label, actionable }` (or equivalent enum) is the only place event meaning is derived.
- [ ] The count of `.contains(` calls inside the alert functions of `service.rs` (`alert_event`, `alert_category`, `alert_severity`, `alert_label`, `event_class`, `default_alert_message`, `actionable_event`, `alert_explanation`, ≈`:690-809`) drops to 0.
- [ ] A test asserts every event-type string emitted by `usagewatch.rs`/`service.rs` round-trips to a non-default class; adding a new event without updating the table fails to compile (non-exhaustive match) rather than silently mis-coloring the dashboard.
- [ ] `cargo test` passes.

## Notes
**Why (Grug):** a typed lifecycle event is flattened to a bare string at ~50 emit sites in service.rs / 9 in usagewatch.rs, then guessed back by eight near-identical `.contains()` functions. Rename one event or add `usage_termination_grace_completed` and `.contains("grace")`/`("completed")`/`("started")` all fire at once and mis-color the UI. This is the highest-leverage correctness cleanup in `service.rs` and shrinks it materially.
