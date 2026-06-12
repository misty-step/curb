# Curb Product Principles

Status: canonical product doctrine
Date: 2026-06-12

## Vision

Curb is the local control surface for AI coding agents. It makes agent work
visible, measurable, and governable on the machine where the work is happening.
The product succeeds when an operator can answer, at a glance:

```text
Which agents are spending now, what risk are they creating, and what can Curb
do safely?
```

Curb is not a cloud supervisor, a prompt recorder, a billing dashboard, or a
generic process monitor. It is a local endpoint agent that watches metadata-only
usage evidence, correlates it with live process reality, and gives the operator
clear, conservative action.

## Philosophy

Curb starts from the operator's trust boundary. The machine is the authority for
local process state, local provider metadata, local policy evaluation, local
notifications, and any local termination action. Remote systems may receive
metadata-only exports later, but they do not become the policy authority for the
launch product.

The product measures the risk that matters for autonomous coding loops: fresh
turn spend. Wall-clock runtime is supporting evidence. A process can sit idle
for hours without spending, while one instruction can burn a budget quickly.
Curb should therefore make token spend legible first and treat duration as a
secondary stuck-process signal.

Enforcement is a privilege, not the default posture. Visibility and alert modes
must never terminate processes. Enforcement mode may stop only a correlated,
live, enforceable worker after policy, grace, and identity checks all agree.

Privacy is an invariant, not a setting to trade away for convenience. Prompt
text, response text, screenshots, keystrokes, and file contents are outside the
product model by default and must be rejected at config or ingestion boundaries.

## Product Principles

1. Lead with sessions, not processes.
   Sessions are the product object because spend is the product risk. Processes
   are evidence that explain liveness and actionability.

2. Keep clients thin.
   The Rust service owns usage ingestion, correlation, policy, actionability,
   ledger writes, and termination safety. CLI, web, tray, and future native
   shells render service-owned views and send explicit user intent.

3. Make safety states visible.
   "Kill threshold crossed but cannot stop" is a successful safety state when
   the process is uncorrelated, watch-only, acknowledged, or outside enforce
   mode. The UI must explain that state instead of hiding it.

4. Prefer metadata evidence over brand support.
   A provider is usage-metered only when Curb can read local metadata that proves
   token spend without content capture. Process visibility alone is not usage
   metering.

5. Treat the ledger as the audit truth.
   The append-only local ledger records decisions, warnings, acknowledgements,
   stops, and source-health evidence. Process logs are diagnostics; the ledger
   is the product record.

6. Keep remote systems advisory.
   Exports, sidecars, and future fleet views may help operators coordinate, but
   local policy and local process authority remain endpoint-owned. Future ADRs
   may define packaging, export, or fleet-view contracts, but they must not move
   privacy, policy, or termination authority out of the local endpoint.

7. Ship through live evidence.
   A green unit-test suite is necessary but not enough. Product changes should
   leave reviewable evidence from the relevant live loop: CLI transcript,
   request/response capture, rendered dashboard smoke, dogfood packet, or CI
   run that exercises the real surface.

## Engineering Principles

1. Rust is the durable product boundary.
   Non-Rust surfaces are clients, packaging, or tests unless a platform boundary
   makes another language unavoidable.

2. Build deep modules with small interfaces.
   Hide provider quirks, process identity, ledger formats, policy state,
   observability schemas, and write-path ordering behind narrow domain modules.

3. Do not weaken gates to land a change.
   If a likely production failure would bypass the current gate, strengthen the
   gate or add an evidence loop as part of the work.

4. Test behavior through public surfaces.
   Use real temp files, real ledgers, harmless subprocesses, deterministic OS
   fixtures, and rendered UI checks. Mock only external OS or network
   boundaries where real action would be harmful or nondeterministic.

5. Preserve operator-owned state.
   Do not overwrite unknown configs, hooks, worktrees, or generated evidence.
   If state is ambiguous, surface it.

6. Refactor toward less surface.
   Delete stale docs, dead branches, unused seams, and shallow pass-throughs.
   Add abstraction only when it hides real complexity or preserves a durable
   product boundary.

## When Principles Conflict

Prefer this order:

1. Privacy and termination safety.
2. Local operator authority.
3. Accurate usage evidence.
4. Clear operator explanation.
5. Deep module boundaries.
6. Delivery speed.

Privacy, termination safety, and local endpoint authority are not demotable by
ADR. If a future feature clarifies lower-priority tradeoffs, record the decision
as an ADR and update this document in the same change.
