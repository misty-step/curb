# Extract an embeddable governor core for arbitrary agents in arbitrary environments

Priority: P2
Status: pending
Estimate: XL

## Goal
Split Curb into an environment-agnostic `curb-core` governor (policy + observation + enforcement) that another orchestrator (Olympus) can embed to govern agents that are not necessarily local OS processes, leaving the HTTP API + UI as one consumer of that core.

## Non-Goals
- Building Olympus itself, or any specific remote/container transport — this ticket delivers the *seam* and one reference adapter, not the integration.
- Changing the local-OS watchdog's behavior or its sealed-termination safety contract (PID + start + owner + executable). Existing tests must stay green.
- Touching the React UI — it stays a consumer of the HTTP shell, which becomes a consumer of `curb-core`.
- A plugin/DSL system. Olympus integrates via Rust traits, not a config language (initially).

## Oracle
Phased; each phase is independently shippable.

- [x] **Phase 0 — Make the policy state machine environment-agnostic. ✅ LANDED.** `UsageWatch` is now a pure policy module over `PolicySession` + `AgentTarget` (OS-free; pid is a bare `i64`, the seal hidden in an opaque `StopToken`) + an `Enforcer` trait. `grep -lE "use crate::(service|platform)" src/usagewatch.rs` returns nothing. Correlation/ack/escalation/seal moved to `src/local_enforcer.rs`; runtime drives the pure scan. 166 tests green; ousterhout-critic approved. *(This was the heart; everything else is I/O plumbing around it — pi lane.)*
- [ ] **Phase 1 — `curb-core` crate boundary.** Workspace splits into `curb-core` (config, usage, usagewatch, platform, ledger, runtime, policy) and `curb` (bin: api/http/web/dashboard/cli + view transforms). `cargo tree -p curb-core` shows no api/http/web/dashboard deps; the bin depends on core, never the reverse. `lib.rs` stops re-exporting transport modules.
- [ ] **Phase 2 — Generalize identity & observation.** `Snapshot`/`Process` sit behind an identity abstraction so an agent need not have a local PID; an `AgentKind`/matcher path exists for logically-defined agents. A test governs a synthetic non-OS agent (no real PID) through warn→stop via a fake observation source + enforcer, with the safety seal satisfied by environment-appropriate identity evidence.
- [ ] **Phase 3 — Governor API + reference adapter.** A stable trait surface (`ObservationSource`, `Enforcer`, policy `Engine`) is documented; an example adapter governs an arbitrarily-defined agent end to end. The local-OS path still passes every existing test.

## Notes
**Why (user direction):** fold Curb into Olympus as a governor for arbitrarily-defined agents in arbitrary environments. The user correctly intuited the prerequisite: properly decouple the engine/enforcement/watcher from the UI/UX.

**Coupling evidence (Explore lane, file:line):**
- `src/lib.rs:7-19` exports every module wholesale — no boundary today.
- **Dependency arrow is backwards:** `src/runtime.rs:11-16` and `src/usagewatch.rs:10` both `use crate::service` (`build_sessions`, `process_matches`, `correlate`, `build_snapshot_filtered`) — the engine depends on the module that builds API JSON. `service.rs` is the chokepoint: policy (ack/stop) + correlation + snapshot + view transforms all in one ~3.5k-line file.
- **Good news — the enforcement seam already exists:** `src/platform.rs` is a `Platform` trait (`capture`, `notify`, `terminate`, capability queries) with a `FakePlatform` test double. Observation and the kill action are already swappable; this ticket generalizes *what they observe/act on*, not whether they're abstracted.
- **What's hardwired to local OS:** `platform::Snapshot`/`Process` key on OS PIDs and OS identity fields (started_at, username, executable, bundle_id, team_id); `config::Match` (`src/config.rs:707-723`) matches process names / command regex / bundle ids / paths. A logically-defined agent has none of these — needs a new identity/matcher path.

**Design sketch (refined by pi roster lane — anchor on the existing `Platform` trait, then split it into two):**
- `Observer` — returns **already-correlated** `AgentTarget`s (opaque id + identity-revalidation tokens + optional spend/turn summary + labels). *Key decision: the policy core never sees raw OS processes or raw log events.* The local `Observer` internally does what `build_snapshot` does today (scan `usage::Reader`, capture the process tree, `correlate`); a k8s/remote `Observer` queries its own world. **Correlation moves into the local adapter, not the core.**
- `Enforcer` — executes `warn` / `acknowledge` / `stop{grace,force}` against an opaque `AgentTarget`. *Key decision: the sealed `TerminationTarget` (PID + start + owner + executable) is an implementation detail of the local enforcer, NOT a core abstraction* — it lives in `LocalEnforcer`, so the safety contract is preserved locally without leaking OS facts into the core.
- policy `Engine` — pure, environment-agnostic: thresholds, window, grace, terminated-state, escalate-supervised, ack suppression. No `Platform`/`Snapshot`. This is what Olympus reuses unchanged. Config splits too: env-agnostic thresholds stay in core; OS matchers (`process_names`/`bundle_ids`/`*_paths`/`command_regex`) move into a `LocalMatcher` owned by the local observer.
- Orchestration: `runtime.rs` → `Governor<O: Observer, E: Enforcer>` owning config/state/ledger and the tick loop; `api`/`http`/`web` call `Governor`, not `Runtime<P: Platform>`. Adapters land under `src/adapters/` (`local_usage`, `local_platform`, `local_observer`, `local_enforcer`).

**Sequencing / dependencies:**
- Phase 0 overlaps and extends **006** (extract onboarding), **007** (typed events), **008** (extract write-path) — do those first; they shrink `service.rs` and make the inversion tractable. This ticket should be re-`/shape`d into per-phase tickets when picked up.
- Strategic, but ranked below the P0/P1 trust+CI work (001–004): an embeddable governor is only worth shipping if the kill decision it embeds is provably correct.
