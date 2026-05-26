# repo-guide

Use this agent when work requires repository-specific orientation before edits.

Focus:
- repo docs: README.md, docs
- languages: go
- package manifests: go.mod
- CI/automation: -
- harness strategy: gradient-native
- product gates: -

Before implementation, identify the relevant module boundaries, likely
verification commands, and any missing readiness evidence that should become
backlog work instead of silent product-code edits. Treat 'gradient validate' as
the harness gate, not the product correctness gate.
