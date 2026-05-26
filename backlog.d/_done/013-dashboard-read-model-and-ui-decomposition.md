---
id: 013-dashboard-read-model-and-ui-decomposition
title: Decompose the dashboard around worker and usage read models
priority: P1
status: done
lifecycle_stage: Context
acceptance:
    - `ui/src/App.tsx` is split into focused components for operator summary, workers, sessions, turns, policy, health, alerts, and details.
    - Dashboard derivations live in a tested `ui/src/readModel.ts` selector module.
    - The first viewport answers: who is alive, who is spending now, what is unmatched, what needs attention, and what Curb will do.
    - Browser smoke tests cover desktop and narrow viewport rendering without overlapping or misleading state labels.
evidence_required:
    - `cd ui && npm test -- --run` (2026-05-26: 36 tests passed)
    - `cd ui && npm run build` (2026-05-26: Vite production build passed)
    - `scripts/build-ui.sh --check` (2026-05-26: embed check ok)
    - `cd ui && npm run smoke` against `http://127.0.0.1:8765/` (2026-05-26: desktop and narrow Playwright smoke passed)
---

# Context Packet: Decompose the dashboard around worker and usage read models

## Goal

The dashboard makes live workers, active token spend, unmatched usage, and policy action obvious without embedding product logic in React component branches.

## Non-Goals

- Do not change backend policy or enforcement semantics.
- Do not add a native desktop shell.
- Do not add marketing screens or decorative landing-page UI.
- Do not hide advanced details; move them behind clear sections.

## Constraints / Invariants

- Live provider rows come from `Snapshot.Agents`, not usage-session history.
- Usage sessions remain available for drilldown even when no live process is correlated.
- The top summary must never count multiple sessions on one process as multiple active workers.
- Text must fit on common laptop and narrow mobile widths.

## Authority Order

1. UI tests and browser smoke
2. Service read model
3. Product copy in `docs/application-design.md`
4. React component implementation

## Repo Anchors

- `ui/src/App.tsx` - current monolithic dashboard
- `ui/src/App.test.tsx` - behavior tests for summary, onboarding, config, details
- `ui/src/types.ts` - API read-model types
- `ui/src/styles.css` - layout and responsive behavior
- `docs/application-design.md` - dashboard state-axis design

## Oracle

- [ ] `ui/src/readModel.test.ts` proves active worker, alive worker, unmatched usage, and grouped worker selectors.
- [ ] `App.tsx` no longer owns dashboard derivation logic; it composes components.
- [ ] Browser smoke screenshot shows no “Claude” live row when live providers are only Codex and Anti-Gravity.
- [ ] Browser smoke screenshot shows `Active runs 0`, `Alive workers N`, and `Unmatched logs X` for uncorrelated usage.
- [ ] Existing onboarding/config/detail tests still pass.

## Implementation Sequence

1. Extract pure selectors into `ui/src/readModel.ts` with tests copied from current regression cases.
2. Split presentational components without changing layout.
3. Rename the main sections to product nouns: Workers, Usage, Alerts, Policy, Health.
4. Add browser smoke script or Vitest-compatible Playwright helper for the dashboard first viewport.
5. Run visual smoke at desktop and narrow widths and update docs if section names change.

## Risk + Rollout

- Risk: component split becomes churn. Keep selectors first and preserve CSS class names until tests pass.
- Risk: browser tests become flaky. Start with text and layout sanity, not pixel-perfect snapshots.
- Rollback: keep `readModel.ts` while collapsing components back into `App.tsx`.

## Why

Recent dashboard confusion came from React inferring product state from raw
session fields. A tested read-model layer prevents that class of bug.

## Completion

- Extracted dashboard selectors into `ui/src/readModel.ts` and covered active,
  alive, unmatched, grouped, and correlation selectors in `ui/src/readModel.test.ts`.
- Split the React dashboard surface out of `ui/src/App.tsx` into
  `ui/src/components/dashboard.tsx`; `App.tsx` now owns API state and actions.
- Added `ui/scripts/smoke-dashboard.mjs` and `npm run smoke` to exercise the
  first viewport at desktop and narrow widths against the embedded dashboard.
