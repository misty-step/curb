# Finish facade and presenter simplification

Priority: P1
Status: pending
Estimate: L

## Goal
Finish the remaining no-behavior-change simplification after the broad readiness
refactors, focusing on bounded transport behavior, typed audit writes,
compatibility-only config retirement, and broad API/presenter facades that still
amplify future changes.

## Oracle
- [ ] Re-read `docs/refactor-map.md` and current source line counts, then
      identify the smallest remaining module-depth milestones that are still
      worth doing after the hosted gate is green.
- [ ] Bound loopback HTTP transport so a slow partial-header client cannot block
      `/v1/live`; read/write timeouts and loopback/auth/headless invariants are
      tested.
- [ ] Seal production audit-ledger writes behind typed event constructors or an
      explicit legacy/custom escape; unknown historical ledgers still read.
- [ ] Retire or deliberately migrate compatibility-only process-duration policy
      fields so active config/API/docs name token policy as the enforced
      behavior.
- [ ] Split the broad `api::Backend` port into use-case grouped read, write,
      notification, and onboarding ports only if that reduces whole-product fake
      implementations without one-file-per-route fragmentation.
- [ ] Prioritize presenter/UI read-model simplification and binary-shell pressure
      over speculative taxonomy splits.
- [ ] For each milestone, write the public behavior oracle first: API fixtures,
      UI read-model tests, CLI output, or dogfood smoke.
- [ ] Complete at least one milestone with no wire-format, safety, or policy
      behavior change.
- [ ] Use a fresh critic on the diff plus oracle before moving to the next milestone.

## Children
1. Audit the current facades: `src/http.rs`, `src/api.rs`, `src/cli.rs`,
   `src/main.rs`, `src/observability.rs`, `curb-core/src/ledger.rs`,
   `curb-core/src/config.rs`, `curb-core/src/runtime.rs`, and
   presenter/read-model modules.
2. Start with bounded loopback HTTP transport if the hosted gate is green; it is
   the most operator-visible simplification risk.
3. Type production ledger writes and classify the existing `doctor` event as
   first-class, legacy, or custom.
4. Retire process-duration policy fields through a compatibility-aware migration
   plan.
5. Split the API backend port only after route/auth/fixture behavior is locked.
6. Pick each milestone so it reduces change amplification without adding a new
   abstraction layer.
7. Implement with public behavior tests and run focused plus full gates.
8. Update `docs/refactor-map.md` only where the implemented milestone changes the map.

## Notes
**Why:** Architecture/simplification perspective. The previous `028` epic
closed the first deep-module tranche, but a fresh read found concrete remaining
pressure: `src/http.rs` has synchronous accepted-stream handling and no
`set_read_timeout`, `curb-core/src/ledger.rs` accepts arbitrary event strings
even though `ledger/taxonomy.rs` is closed, `curb-core/src/config.rs` documents
unused process-duration policy fields, and `src/api.rs` keeps a broad backend
port that test fakes must implement wholesale.

Do not start this until `034` is green; red hosted gates make refactor feedback
unreliable.
