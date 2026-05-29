# Extract the onboarding/capability presenter out of service.rs

Priority: P1
Status: ready
Estimate: M

## Goal
Move onboarding and platform-capability presentation into its own module so `service.rs` is focused on the snapshot read-model and its write path.

## Non-Goals
- Behavior change — this is a pure structural move; the read-model output must be byte-identical.
- A blanket line-count split of service.rs (extract by *concern*, not by size).

## Oracle
- [ ] `onboarding_view`, `platform_capabilities`, and their ~14 private `*_step` / `*_capability` helpers (≈`src/service.rs:485-1409`) live in a new `src/onboarding.rs` whose public surface is only the view entry points + the View structs they return.
- [ ] `grep -cE '^\s*pub (fn|struct|enum)' src/service.rs` drops by ≥ 8.
- [ ] The string `onboarding` no longer appears in `service.rs` outside imports.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` pass unchanged.

## Notes
**Why (Ousterhout):** `service.rs` (3,516 lines) is a shallow god-object — ~44 public items spanning six unrelated concerns (read-model/snapshot, alert classification, onboarding, capability formatting, config view/mutation, write-path persistence). No single client uses the whole module; each importer pulls a disjoint slice. `usage.rs` and `api.rs` are the deep-module shape to aspire to. Onboarding/capabilities is the cleanest first concern to lift out.
- Related: 007 (event taxonomy) and 008 (write-path) carve out the other concerns; 009 ratchets a line cap only after these land.
