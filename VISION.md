# Curb Vision

Status: Canonical root vision for Curb. `docs/product-principles.md` remains
the deeper doctrine; this file is the cold-start north star.

## What Curb Is

Curb is the local control surface and watchdog for AI coding agents. It makes
local agent sessions, fresh turn spend, risk state, policy decisions, source
health, and safe operator actions visible on the machine where the work is
happening, then stops runaway workers only when the configured policy and
process identity evidence are strong enough.

It serves an operator or enterprise team running API-backed autonomous coding
loops without sufficient native cost controls. The basic question is: which
agents are spending now, how much have they spent since the last human message,
what risk are they creating, and what can Curb safely do?

## North Star

Local, privacy-preserving governance: Curb reads metadata evidence, correlates
it with live process reality, explains safety states, records decisions in an
append-only ledger, and only enforces when identity, policy, grace, and
fresh-spend checks agree.

## What Must Stay True

- Sessions are the product object because spend is the product risk. Processes
  explain liveness and actionability.
- Fresh turn spend is the runaway signal. The canonical policy question is how
  many tokens or dollars this worker has consumed since the operator last
  meaningfully intervened; wall-clock time is supporting evidence, not the
  primary risk measure.
- Privacy excludes prompt text, response text, screenshots, keystrokes, and
  file contents by default.
- The Rust service owns usage ingestion, policy, ledger, source health,
  correlation, and termination safety. CLI, web, tray, and native shells stay
  thin.
- Visibility and alert modes must never terminate processes.
- Enforcement mode may stop only a correlated, live, enforceable worker after
  policy, grace, and identity checks all agree.
- Token, dollar, turn, and runtime limits should be explicit policy objects with
  dry-run proofs before enforcement becomes the default.
- Termination APIs must never accept a bare PID. The safety boundary is PID
  plus process start time, owner, executable/app identity, and sealed target.

## What Curb Refuses

- Cloud policy authority for the launch product.
- Generic process-monitor scope.
- Prompt recording, response capture, screenshots, keystrokes, or file-content
  ingestion as a convenience feature.
- Terminating uncorrelated, watch-only, acknowledged, desktop-app-root, or
  otherwise non-enforceable processes.
- Provider support based on brand names rather than local metadata that proves
  token spend without content capture.
- UI shells that own business truth instead of rendering service-owned views.

## Current Bets

1. Keep the live menu bar, dashboard, ledger, and watchdog proofs tied to real
   dogfood evidence.
2. Make source-health and recovery actions clear without turning recovery into
   unsafe enforcement.
3. Preserve local endpoint authority even if future fleet or export surfaces
   appear.
4. Keep the rendered dashboard smoke and demo dry-run as product proof, not
   ceremonial gates.
5. Ship through `scripts/validate.sh` and visible QA for menu-bar/window
   behavior.

## Where The Depth Lives

- `docs/product-principles.md` is the detailed product and engineering doctrine.
- `AGENTS.md` is the repo operating contract, verification map, and invariants.
- `README.md` explains the product, supported agents, Rust implementation, and
  gate ladder.
- `SPEC.md` is the launch implementation specification.
- `docs/contributor-guide.md`, `docs/user-guide.md`,
  `docs/application-design.md`, and `docs/release-evidence.md` carry deeper
  implementation, user, UI, and proof contracts.
- `scripts/validate.sh` is the local pre-merge gate.
