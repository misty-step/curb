# API/UI contract drift guard

Priority: P0
Status: ready
Estimate: M

## Goal

Prevent Rust service read models and TypeScript UI types from silently drifting
before the next deep-module refactors.

## Context

The UI currently mirrors Rust payload shapes by hand in `ui/src/types.ts`.
This is workable while the system is small, but broad refactors of
`curb-core/src/service.rs` and `src/api.rs` will be risky until wire contracts
are executable. The right next step is not a framework rewrite; it is a narrow
contract fixture/check that locks current public behavior.

## Oracle

- [x] Define canonical JSON fixtures for the UI-facing payloads:
      `/v1/snapshot`, `/v1/overview`, `/v1/sessions/{key}`,
      `/v1/sessions/{key}/turns`, `/v1/config`, `/v1/onboarding`,
      `/v1/live`, and `/v1/ready`.
- [x] Add a Rust test that serializes current service/API views and compares
      them to the canonical fixtures.
- [x] Add a UI test or script that loads the same fixtures through
      `ui/src/types.ts` and `ui/src/readModel.ts`.
- [x] Add unknown-enum and malformed-payload tests for operator-visible API
      failures.
- [x] Wire the contract check into the mandatory local gate or document why a
      subset remains advisory.
- [x] Document how to intentionally update fixtures when a wire contract changes.

## Non-Goals

- Do not generate TypeScript from Rust until fixture drift proves insufficient.
- Do not change route names, field names, or payload schemas in this ticket.
- Do not loosen TypeScript strictness or serde validation.

## Suggested Proof

```sh
cargo test --bin curb contract -- --nocapture
cd ui && npm run test -- --run contract
scripts/validate.sh
```
