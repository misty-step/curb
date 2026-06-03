# Dogfooding Curb

Curb's next proof is real local use. The merged code has green local and remote
gates, but product confidence now comes from running the release binary during
normal agent work and recording what is useful, confusing, or wrong.

## Current Build

Build the release CLI:

```sh
cargo build --release --bin curb
```

Run the local app:

```sh
./target/release/curb app
```

For a headless/server-style run:

```sh
./target/release/curb serve --addr 127.0.0.1:8765
./target/release/curb watch
```

## What To Watch

- Does startup choose the expected config and state directory?
- Does `curb usage --since 24h` find real Codex, Claude, and Pi metadata without
  showing prompt or response content?
- Does the app clearly distinguish active, warn, stop, watch-only,
  uncorrelated, idle-high, and idle sessions?
- Do notifications report truthfully when they are disabled or unavailable?
- Does enforcement remain scoped to correlated worker processes, never desktop
  app roots?
- Are false positives, false negatives, or process-correlation surprises easy to
  understand from the UI and ledger?

## Olympus Readiness

Curb is effectively modular enough for Olympus when run headless on Linux.
Olympus can treat Curb as a governor core or sidecar: initialize it on a Sprite,
feed policy sessions from Olympus run state, and use Olympus-owned stop tokens
for cooperative run or lane stops. The first integration should stay in Olympus
adapter code rather than making Curb depend on Olympus internals.

## Next Grooming Session

After the first real dogfood session, use
`backlog.d/023-post-closeout-grooming-and-dogfood.md` to shape the next tranche:
refactoring, stronger gates, Windows proof, release/install flow, user-like QA,
and Olympus adapter readiness.
