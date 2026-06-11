# Dogfood evidence matrix and next-backlog intake

Priority: P0
Status: ready
Estimate: S

## Goal

Turn real Curb dogfooding into a repeatable evidence contract. The next tranche
should be driven by observed release-build behavior, not by speculative feature
ideas.

## Context

`023` identified dogfooding as the first post-closeout proof. Curb already has
green Linux/macOS validation and a release-build guide, but the repo does not
yet have a durable template for recording install friction, startup behavior,
source fidelity, notification behavior, process-correlation surprises, UI
clarity, or headless readiness.

## Oracle

- [x] Create `evidence/dogfood/README.md` describing the evidence contract and
      a run naming convention: `evidence/dogfood/YYYY-MM-DD-<short-slug>/`.
- [x] Add a dogfood evidence template under `docs/` or `evidence/` with fields
      for build SHA, OS, command, config path, state path, mode, provider roots
      detected, source-health errors, notification health, UI observations,
      false positives, false negatives, and enforcement safety observations.
- [x] Capture at least one real release-build session using:
      `cargo build --release --bin curb`,
      `./target/release/curb usage --since 24h`, and either
      `./target/release/curb app` or `./target/release/curb serve`.
- [x] The captured evidence proves prompt, response, screenshot, keystroke, and
      file-content capture remain absent.
- [x] The captured evidence includes a source-health baseline with expected and
      observed provider categories so later runs can compare false positives,
      false negatives, missing providers, and noisy roots deterministically.
- [x] The evidence produces a ranked next-backlog table with acceptance oracles
      and explicit source lines from the dogfood notes.
- [x] Update `docs/dogfooding.md` so future agents know the evidence artifact is
      the acceptance source before opening new feature tickets.
- [x] Capture a second dogfood run during an active agent session so usage
      fidelity, live process correlation, false positives, and false negatives
      are exercised against non-zero provider events.
- [x] Add a repeatable timed headless-observability dogfood script that captures
      real local provider metadata, structured NDJSON, readiness probes,
      protected API snapshots, parser output, and redaction evidence without
      writing service state into the worktree.
- [x] Harden the timed headless-observability oracle so weak runs fail:
      `scripts/dogfood-headless-observability.sh` validates positive integer
      duration, records `duration_seconds` and `expected_watcher_tick_min`,
      requires watcher ticks to scale with the requested window, and checks
      NDJSON for token/auth, prompt/response, screenshot, keystroke,
      file-content, raw-provider, and payload markers. The
      `2026-06-04-headless-observability-30s-oracle` proof captured 10 watcher
      ticks against a required minimum of 6.

## Non-Goals

- Do not change product behavior in this ticket.
- Do not claim product acceptance from build success alone.
- Do not add speculative backlog items without dogfood evidence references.

## Suggested Proof

```sh
mkdir -p evidence/dogfood/$(date +%F)-local-release
cargo build --release --bin curb
./target/release/curb validate-config configs/curb.example.yaml
./target/release/curb usage --since 24h
git status --short --untracked-files=all
```
