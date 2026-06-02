# Olympus Governor Integration

This is the first embeddable shape for Curb as a governor inside Olympus.
Olympus owns orchestration, run identity, lane health, Sprite readiness, and the
stop primitive. Curb owns the policy state machine: thresholds, warning dedupe,
grace periods, terminated-session memory, and ledger projection.

## Chosen Shape

Use `curb_core::governor::GovernorEngine` over pre-correlated
`PolicySession`s.

Olympus should not expose raw SQLite rows, Sprite processes, or lane queues to
Curb. It should build a `PolicySession` for each governable run or job, attach
an opaque `StopToken`, and implement `Enforcer` for Olympus actions.

```text
Olympus runtime/lane state
  -> Olympus adapter: run/job observation + correlation
  -> PolicySession + OlympusStopToken
  -> GovernorEngine::scan(...)
  -> OlympusEnforcer::{notify, stop}
  -> request lane stop / run kill request / Olympus audit event
```

## Field Mapping

Recommended `PolicySession` values for an Olympus run:

- `key`: stable run key, for example `olympus:{workflow}:{run_id}`.
- `id`: Olympus run id or idempotency key.
- `provider`: `olympus`.
- `cwd`: repository/workspace root when known.
- `models`: models observed in job results or tool invocations.
- `last` and `last_usage`: latest run, job, or agent-result activity time.
- `latest_turn_tokens`: latest job or agent turn spend.
- `window_spent_tokens`: aggregate spend for the current governor window.
- `total_tokens`: lifetime run spend.
- `acknowledged`: resolved by Olympus from its own operator state.
- `target.matched`: true when the run is active and governable.
- `target.agent_id`: lane, stage, sprite, or workflow id.
- `target.can_terminate`: true only when Olympus can safely request a stop.
- `target.pid`: `None` unless Olympus is deliberately governing a local worker.
- `target.stop_token`: an Olympus token that can revalidate and stop the run.

The Olympus `StopToken` should carry the identity evidence Olympus needs, such
as run id, workflow, trace id, idempotency key, sprite name, and current run
generation. Curb stores the token across the grace lifecycle but never inspects
it.

## Enforcement

`Enforcer::notify` should write to Olympus-visible operator surfaces: run logs,
dashboard events, or lane health events.

`Enforcer::stop` should revalidate that the token still refers to the same live
run before taking action. Prefer cooperative orchestration actions such as
`requestLaneStop` or a run-level kill-request flag. Raw Sprite process
termination is a later adapter, not the first integration point.

## Deferred

- No `ObservationSource` trait yet. Olympus already has the observation model,
  and a generic source trait would be a shallow pass-through until a second
  real consumer needs it.
- No TypeScript/Rust FFI shim in this ticket. If Olympus remains TypeScript-only,
  add a small CLI/FFI bridge after the Rust API stabilizes.
- No local PID assumptions. The synthetic reference test proves the governor can
  stop an Olympus-like run with `pid: None`.
