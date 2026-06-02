# Split usage ingestion into provider-shaped modules

Priority: P2
Status: done
Estimate: M

## Goal
Turn `curb-core/src/usage.rs` from a mixed reader/parser/reporting module into
deep provider-shaped boundaries that make the next adapter small and testable.

## Non-Goals
- Changing usage semantics.
- Adding a new provider in the same change.
- Moving local OS process correlation out of its existing service/runtime path.

## Oracle
- [x] `curb-core/src/usage.rs` is reduced to orchestration/shared types, with
      Codex and Claude parsers moved behind provider modules or a narrow parser
      trait.
- [x] Existing Codex/Claude fixture behavior is preserved by public parser or
      reader tests, with no internal collaborator mocks.
- [x] CLI-only report/summarize code is moved out of core if no core consumer
      requires it.
- [x] `scripts/validate.sh` passes.

## Notes
Closeout: implemented in `3e17bdc`; follow-on provider work in `018e8a0` now
uses the new boundary.

The technical-hotspots lane found `curb-core/src/usage.rs` at roughly 1.8k LOC,
mixing source discovery, cache behavior, provider dispatch, wire structs, and
report formatting. This is the highest-leverage simplification before ticket
021.
