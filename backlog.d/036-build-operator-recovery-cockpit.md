# Build the operator recovery cockpit

Priority: P1
Status: pending
Estimate: L

## Goal
Make first-run, degraded, auth, config, provider-source, notification, and
process-correlation failures recoverable from the product surface without log
spelunking.

## Oracle
- [ ] Define the operator recovery states from existing `/v1/onboarding`,
      `/v1/ready`, `/v1/health`, `/v1/overview`, notification health, and
      source-health data before adding UI.
- [ ] The app shows a single actionable recovery surface for missing
      config/state, API/auth mismatch, unavailable notifications, provider
      source errors, degraded readiness, and no correlated worker.
- [ ] Each recovery item names the exact command, config path, state path, or
      runbook section needed to fix it, without exposing tokens or private
      provider content.
- [ ] Rust contract fixtures and UI tests cover the recovery states through
      public API payloads, not ad hoc component branches.
- [ ] A deterministic browser smoke captures desktop and narrow recovery screens
      with no overflow and no generic `Failed to fetch` dead ends.

## Children
1. Shape the recovery-state read model and fixture set from current service views.
2. Add UI for the recovery surface inside the existing dashboard hierarchy.
3. Add route/API/UI tests for auth/config/provider/notification/readiness failures.
4. Dogfood a broken-startup scenario and record the recovery path in evidence.

## Notes
**Why:** Product/operator perspective. The current app is strong on safety and
happy-path readiness, but the subagent product lane found no active item that
explicitly owns first-run bootstrap recovery or troubleshooting when startup,
auth, config, or identity state drifts.

Do not let the React app infer platform or process policy. Service-owned
recovery states must carry the explanation.

Long sidecar dogfood in
`evidence/dogfood/2026-06-12-long-sidecar/` sharpened this ticket: recovery
must surface provider source-health failures such as oversized Claude JSONL
lines and repeated Claude/Codex session read failures, plus degraded readiness
when `watcher_runtime` reports `cache busy`, without requiring the operator to
read NDJSON logs.
