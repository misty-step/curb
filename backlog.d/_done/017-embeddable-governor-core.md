# Extract an embeddable governor core for arbitrary agents in arbitrary environments

Priority: P2
Status: done
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
- [x] **Phase 1 — `curb-core` crate boundary. ✅ LANDED.** Cargo workspace: `curb-core` (lib: config, usage, usagewatch, platform, ledger, runtime, local_enforcer, service, onboarding, write_path, tail) + `curb` (bin at root: api/http/web/dashboard/cli + the web/dist embed). `cargo tree -p curb-core` shows no transport/clap deps; the bin depends on core, never the reverse (zero core→bin refs). The one core→bin edge (`tail → cli::default_home_dir`) was resolved by moving `default_home_dir` into `curb_core::config`. `Backend`/`Server` stay in the bin (a legal bin→core edge). Gate runs `--workspace`. 167 tests green.
- [x] **Phase 2 — Generalize identity & observation.** `PolicySession` and `AgentTarget` now carry the environment-agnostic identity surface, and `GovernorEngine` accepts pre-correlated sessions with `pid: None`. `curb-core/src/governor.rs` tests an Olympus-like run through warn→stop using an opaque run token rather than a local process identity.
- [x] **Phase 3 — Governor API + reference adapter.** `curb_core::governor::GovernorEngine` is the stable embedding API over existing `PolicySession` + `Enforcer` contracts. `docs/olympus-governor-integration.md` documents the Olympus adapter shape and explicitly defers a speculative `ObservationSource` trait until a second concrete consumer needs it.

## Status (2026-05-30)
- **Phases 0 & 1: LANDED** (commits `cac9ea4`, `983ad36`) — the watcher is a pure
  environment-agnostic policy module, now in an embeddable `curb-core` crate
  (the bin depends on core, never the reverse). CI green.
- **Phases 2 & 3: DEFERRED, pending concrete Olympus requirements.** A
  fresh-context review found Phase 2's oracle is already met by Phase 0: the
  policy governs synthetic non-OS `AgentTarget`s today (the 11 `usagewatch`
  tests use `FakeEnforcer`/`FakeToken`, no real PID, with seal revalidation).
  An `ObservationSource`/governor-API trait now would be a speculative
  pass-through with no real second consumer — against "no speculative
  abstractions." Unblock when Olympus provides ONE of: (a) a real non-local
  observation adapter (k8s/remote/container) to generalize toward; (b) the
  logical-identity revalidation contract a remote `StopToken` must satisfy; or
  (c) the governor call shape (who drives the tick loop, sync/async, required
  observed-session fields). Re-`/shape` Phases 2–3 into concrete tickets then.

## Grooming status (2026-06-01)
Active backlog grooming found this ticket was still listed as pending even
though its executed work landed in `cac9ea4` and `983ad36`, and its remaining
phases are explicitly deferred pending concrete Olympus requirements. Keep the
ticket for context, but do not treat it as ready work until those requirements
arrive and Phases 2-3 are reshaped into small tickets.

## Closeout (2026-06-02)
User supplied the missing Olympus direction: inspect Olympus and design the
governor around its lane/runtime seams. Three GPT-5.5 low-reasoning lanes
compared designs and converged on a narrow embedding boundary: Olympus observes
and enforces its own world; Curb evaluates pre-correlated policy sessions.
Implemented `GovernorEngine`, documented the Olympus mapping, and added a
synthetic Olympus-like no-PID run test.

## Notes
**Why (user direction):** fold Curb into Olympus as a governor for arbitrarily-defined agents in arbitrary environments. The user correctly intuited the prerequisite: properly decouple the engine/enforcement/watcher from the UI/UX.

**Coupling evidence (Explore lane, file:line):**
- `src/lib.rs:7-19` exports every module wholesale — no boundary today.
- **Dependency arrow is backwards:** `src/runtime.rs:11-16` and `src/usagewatch.rs:10` both `use crate::service` (`build_sessions`, `process_matches`, `correlate`, `build_snapshot_filtered`) — the engine depends on the module that builds API JSON. `service.rs` is the chokepoint: policy (ack/stop) + correlation + snapshot + view transforms all in one ~3.5k-line file.
- **Good news — the enforcement seam already exists:** `src/platform.rs` is a `Platform` trait (`capture`, `notify`, `terminate`, capability queries) with a `FakePlatform` test double. Observation and the kill action are already swappable; this ticket generalizes *what they observe/act on*, not whether they're abstracted.
- **What's hardwired to local OS:** `platform::Snapshot`/`Process` key on OS PIDs and OS identity fields (started_at, username, executable, bundle_id, team_id); `config::Match` (`src/config.rs:707-723`) matches process names / command regex / bundle ids / paths. A logically-defined agent has none of these — needs a new identity/matcher path.

**Resolved design:** the shipped boundary is `GovernorEngine` over
pre-correlated `PolicySession` values plus an environment-owned `Enforcer`.
The policy core never sees raw OS processes, raw provider events, Olympus DB
rows, or Sprite process facts. The local runtime and Olympus both keep
observation/correlation in their own adapters, then submit policy sessions to
Curb. A generic `ObservationSource`/`Governor<O, E>` framework remains deferred
until a second concrete consumer needs it.

**Sequencing / dependencies:**
- Phase 0 overlaps and extends **006** (extract onboarding), **007** (typed events), **008** (extract write-path) — do those first; they shrink `service.rs` and make the inversion tractable. This ticket should be re-`/shape`d into per-phase tickets when picked up.
- Strategic, but ranked below the P0/P1 trust+CI work (001–004): an embeddable governor is only worth shipping if the kill decision it embeds is provably correct.
