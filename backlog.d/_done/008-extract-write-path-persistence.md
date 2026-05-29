# Separate write-path persistence from the read-model boundary

Priority: P2
Status: ready
Estimate: M

## Goal
Move session-ack file I/O and ledger appends out of `service.rs` so the read-model derivation is no longer entangled with disk side effects and rollback semantics.

## Non-Goals
- Changing ack/stop behavior or ledger format.
- Merging this with the onboarding extraction (006) — distinct concern, distinct ticket.

## Oracle
- [ ] `write_session_ack` / `read_session_ack` / `delete_session_ack` / `rollback_session_ack` and `append_ledger_event` (≈`src/service.rs:1601-1668`, `:1884-1993`) live behind a dedicated persistence module/type, not beside the pure snapshot derivation (`build_snapshot_filtered`, `build_sessions`, `correlate`).
- [ ] A reader of the snapshot-derivation code path encounters no file or ledger I/O in the same module.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass unchanged.

## Notes
**Why (Ousterhout):** AGENTS.md names `service.rs` "the read-model boundary," yet it has also become a persistence layer — the same file builds the pure read model and performs ack file read/write/delete/rollback plus ledger appends. Holding both in view inflates cognitive load on the crate's most-imported file. Sequence after 006/007.
