# Dogfood Run: 2026-06-03 active agent headless session

## Run Metadata

- Build SHA: `6bff6ce`
- Branch/worktree: detached `HEAD` at `<home>/.codex/worktrees/066e/curb`
- OS: macOS 26.5.1 arm64
- Curb command(s):
  - `cargo build --release --bin curb`
  - `./target/release/curb validate-config configs/curb.example.yaml`
  - `./target/release/curb usage --since 24h`
  - `CURB_LOG_FORMAT=json ./target/release/curb serve --headless --addr 127.0.0.1:18766 --config configs/curb.example.yaml`
- Config path: `configs/curb.example.yaml`
- State path: `.curb/`
- Mode: visibility/headless server
- Operator: Codex during an active Curb agent-readiness session

## Commands

```sh
cargo build --release --bin curb
./target/release/curb validate-config configs/curb.example.yaml
./target/release/curb usage --since 24h
CURB_LOG_FORMAT=json ./target/release/curb serve --headless --addr 127.0.0.1:18766 --config configs/curb.example.yaml 2> evidence/dogfood/2026-06-03-active-agent/headless-complete.ndjson
python3 scripts/parse-observability-smoke.py evidence/dogfood/2026-06-03-active-agent/headless-complete.ndjson
```

## Source-Health Baseline

| Provider | Expected | Observed | Files | Events | Source-health notes |
|---|---|---|---:|---:|---|
| codex | active local session metadata | present | not printed by CLI | 2360 | Non-zero active-session events after fixing `curb usage` default home discovery. |
| claude | historical/local metadata if present | present | not printed by CLI | 701 | Non-zero events, no source-health error. |
| pi | historical/local metadata if present | present | not printed by CLI | 317 | Non-zero events, no source-health error. |

Evidence:

- `usage-since-24h.txt`: initial documented command returned `codex 0 events; claude 0 events; pi 0 events`.
- `usage-since-24h-home.txt`: adding `--home <home>` returned `codex 2360 events; claude 701 events; pi 317 events`.
- `usage-since-24h-fixed.txt`: after the CLI fix, the documented command without `--home` returned the same non-zero provider events.

## Startup And Install Friction

- Build: release build completed successfully in the current worktree.
- Config selection: `validate-config` accepted `configs/curb.example.yaml` with `mode=visibility`, `agents=6`, and `ledger=.curb/runs.ndjson`.
- State directory: example config used `.curb/`; the API token file was written at `.curb/api.token`.
- Port/API startup: headless server bound to `127.0.0.1:18766`; `/v1/live` returned immediately.
- Toolchain issues: none during the release build.
- Discovered friction: `curb usage` initially defaulted to the current directory for provider discovery, unlike `tail`, `status`, `scan`, and `dashboard`. This made the documented command report zero events during an active session. The run fixed this by defaulting `usage` to the OS home directory and adding a CLI regression test.

## UI Or Operator Clarity

- Screen or command surface used: release CLI and headless HTTP probes.
- What was clear: headless startup printed the loopback URL, token path, disabled UI state, live/ready probe URLs, and watcher status.
- What was confusing: readiness can be 503 before the first usage scan. The complete run became ready after the usage scan, but the first short smoke captured the transient 503 in `headless.ndjson`.
- Missing operator action: none for headless CLI operation; rendered dashboard smoke remains advisory/manual.

## Notification And Enforcement Safety

- Notification capability: `/v1/ready` reported `notifications=ok` in `ready-complete.json`.
- Enforcement mode: visibility mode; no termination attempt was expected or observed.
- Correlated workers: the usage output included active Curb worktree sessions, proving provider metadata discovery, but this run did not manually stop a correlated worker.
- Watch-only app roots: no desktop app root was targeted.
- Stop/revalidation observations: no stop request was issued.

## Headless Observability

- `headless-complete.ndjson` contains 14 structured events.
- `parse-observability-smoke-complete.txt` reports:

```text
ok observability events 14 config_loaded,server_started,api_request,readiness_check,readiness_check,readiness_check,readiness_check,readiness_check,readiness_check,readiness_check,readiness_check,usage_scan,readiness_check,health_check
```

- `/v1/live` returned `{"status":"live","app":"curb","api_version":1}`.
- `/v1/ready` returned ready with config, ledger, usage reader, platform capabilities, notifications, and watcher runtime checks all `ok`.
- `/v1/health` required the bearer token from `.curb/api.token` and returned a protected JSON health response.

## Privacy Confirmation

Confirmed absent from captured command output and NDJSON artifacts:

- Prompt content: absent.
- Response content: absent.
- Screenshots: absent.
- Keystrokes: absent.
- File contents: absent.

The evidence records provider ids, session ids, event counts, token totals, cwd paths, readiness state, and structured event names. It does not include raw prompt or response text.

## False Positives / False Negatives

- False positives: none observed.
- False negatives: the initial documented `curb usage --since 24h` command was a false negative caused by defaulting provider discovery to the current directory.
- Process-correlation surprises: no stop path was exercised, so stop correlation remains unproven by this run.
- Noisy roots: active Codex, Claude, and Pi metadata were all present; no source-health error surfaced in the CLI output.

## Backlog Implications

| Rank | Backlog item | Evidence line(s) | Acceptance oracle |
|---:|---|---|---|
| 1 | `027-quality-gates-and-contract-tests.md` | `usage-since-24h.txt`, `usage-since-24h-home.txt`, `usage-since-24h-fixed.txt` | CLI defaults for usage discovery must match other usage-facing commands and have regression coverage. |
| 2 | `032-readiness-latency-and-observability-completion.md` | `headless.ndjson`, `headless-complete.ndjson` | Preserve fast binding plus explicit readiness degradation until first usage scan; keep NDJSON parse coverage. |
| 3 | `024-dogfood-evidence-matrix.md` | `usage-since-24h-fixed.txt` | Active-session dogfood must show non-zero provider events before closing the evidence gap. |
| 4 | `027-quality-gates-and-contract-tests.md` | This run did not issue a stop request. | Real-process E2E diagnostics still need richer failure artifacts before flaky stop/correlation failures are easy to triage. |
