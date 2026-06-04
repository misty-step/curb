# Architecture Decision Records

ADRs record durable product and architecture decisions that would be easy to
break during otherwise reasonable refactors. They are short on purpose: each ADR
names the decision, the invariant it protects, and the commands or artifacts
that prove the decision still holds.

Current ADRs:

- [0001 Headless Service Contract](0001-headless-service-contract.md)
- [0002 Structured Observability Contract](0002-structured-observability-contract.md)
- [0003 Termination Boundary Safety](0003-termination-boundary-safety.md)

Update an ADR when a public contract changes, when a new gate protects the
decision, or when dogfood evidence shows the decision no longer fits reality.
Do not duplicate module inventories from `docs/refactor-map.md`; link them.
