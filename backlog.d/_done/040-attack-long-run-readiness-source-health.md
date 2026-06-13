# Attack long-run readiness and source-health findings

Status: done
Priority: highest

## Goal

Turn the two operator-facing findings from the long-running sidecar dogfood into
first-class product behavior instead of recurring release-note caveats.

## Acceptance

- After one successful snapshot, `/v1/ready` remains `ready` while the watcher
  owns the snapshot cache and explains that it is serving the cached snapshot.
- Before the first snapshot exists, readiness remains strict and degraded.
- Provider source-health errors exposed through the service overview are
  sanitized and paired with operator recovery items.
- React renders service-owned recovery from the frequently refreshed overview,
  deduping older onboarding/readiness recovery items.
- Documentation names the new boundary and preserves the requirement to rerun
  the two-hour sidecar before making a fresh release claim.

## Evidence

- `cargo test -p curb-core readiness_stays_ready_while_cached_snapshot_refreshes -- --nocapture`
- `cargo test -p curb-core readiness_reports_busy_runtime_without_blocking_on_cache -- --nocapture`
- `cargo test -p curb-core overview_exposes_sanitized_source_health_recovery -- --nocapture`
- `cd ui && npm test -- --run readModel.test.ts recovery.test.tsx`

## Follow-up

Run a fresh two-hour long sidecar packet against this change and confirm sampled
`/v1/ready` stays HTTP 200 after the first snapshot while overview recovery
contains sanitized source-health actions.
