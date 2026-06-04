# ADR 0003: Termination Boundary Safety

Status: accepted

Date: 2026-06-04

## Context

Curb can stop runaway agent workers in enforcement mode. That power is useful
only if the stop path is conservative, correlated, and hard to misuse during
future refactors. A bare PID is not enough: PIDs can be reused, process names can
be ambiguous, and desktop app roots must remain watch-only unless explicitly
escalated.

The termination contract spans several deep modules:

- usage ingestion builds metadata-only session facts;
- service/read-model code decides whether a session is actionable;
- write paths and local enforcement revalidate live process identity;
- `platform` owns OS process facts and sealed termination targets;
- `platform/termination.rs` owns OS-specific kill/taskkill execution.

## Decision

Production termination APIs must never accept a bare PID. They accept only a
sealed `TerminationTarget` built from fresh process capture and matching process
identity:

- PID;
- process start time;
- owner;
- executable and/or stable app identity.

Automatic enforcement and manual stop both revalidate identity immediately
before termination. Watch mode and alert mode never terminate. Uncorrelated
sessions, acknowledged sessions, stale identity, watch-only app roots, and
non-actionable sessions are rejected before OS termination is attempted.

Deep-module rule: policy modules may decide that a stop is allowed or blocked,
but only `platform` can seal OS identity and only `platform/termination.rs` can
execute process-tree termination.

## Consequences

Termination safety is preserved across UI, API, CLI, and automatic watcher
paths. Future refactors must keep target construction in `platform`, identity
validation near the write/enforcer boundary, and OS command construction out of
policy code.

This adds ceremony to stop paths, but it prevents the highest-cost failure:
terminating the wrong process or a desktop app root because stale UI or ledger
data looked plausible.

## Verification

```sh
bash scripts/check-termination-boundary.sh
cargo test -p curb-core platform::tests::termination -- --nocapture
cargo test -p curb-core write_path::tests::stop_session -- --nocapture
cargo test -p curb-core usagewatch -- --nocapture
scripts/validate.sh
```

The boundary scan proves production termination remains sealed behind
`TerminationTarget` and OS kill/taskkill command construction stays isolated to
`curb-core/src/platform/termination.rs`.

## Related

- [Deep-Module Refactor Map](../refactor-map.md)
- [Contributor Guide](../contributor-guide.md)
- `backlog.d/028-deep-module-refactor-path.md`
