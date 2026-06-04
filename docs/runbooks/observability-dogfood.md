# Observability Dogfood Runbook

Use this runbook to capture machine-readable evidence for Curb startup,
readiness, usage scanning, stop decisions, and source-health failures.

## Capture

Run a headless service or one-shot watcher with JSON logs enabled:

```sh
CURB_LOG_FORMAT=json ./target/release/curb serve --headless --addr 127.0.0.1:8765 2>/tmp/curb.ndjson
```

For a one-shot scan:

```sh
CURB_LOG_FORMAT=json ./target/release/curb watch --once --config configs/curb.example.yaml 2>/tmp/curb-watch.ndjson
```

## Validate

```sh
python3 scripts/parse-observability-smoke.py /tmp/curb.ndjson
```

The parser should confirm valid NDJSON and required schema fields. If parsing
fails, keep the raw artifact and record the first invalid line number rather
than summarizing it from memory.

## Event Triage

| Event | First owner | Operator action |
|---|---|---|
| `server_started` | `src/server_cmd.rs` / `src/http.rs` | Confirm loopback URL and headless/UI mode. |
| `config_loaded` | `curb-core/src/config.rs` | Confirm config path, mode, and process-agent count. |
| `api_request` | `src/http.rs` / `src/api.rs` | Check status, path template, and duration without relying on raw URLs. |
| `health_check` | `src/api.rs` | Confirms protected health behavior when authenticated. |
| `readiness_check` | `curb-core/src/runtime/readiness.rs` | Check `ok` vs `degraded` and reason code; do not expect readiness to rescan. |
| `usage_scan` | `curb-core/src/runtime/usage_tick.rs` | Inspect event counts, source errors, process counts, policy outcome counts, and duration. |
| `watcher_tick` | `curb-core/src/runtime/watcher.rs` | Inspect policy-loop outcome counts, grace state, stop revalidation outcomes, and timing. |
| `notification_attempt` | `curb-core/src/runtime.rs` | Check capability and delivery state; do not probe OS notifications from UI code. |
| `stop_decision` | `curb-core/src/usagewatch.rs` / `curb-core/src/write_path.rs` | Confirm mode, actionability, and sealed target evidence. |
| `stop_rejection` | `curb-core/src/usagewatch.rs` / `curb-core/src/write_path.rs` | Identify watch mode, alert mode, uncorrelated, acknowledged, or stale identity cases. |
| `source_health_error` | `curb-core/src/usage.rs` | Inspect provider root, file size, symlink/root checks, and parse errors. |
| `shutdown` | `src/server_cmd.rs` / `src/http.rs` | Confirm clean loop exit after signal or shutdown request. |

## Redaction Checks

Logs must not contain API tokens, authorization headers, prompt text, response
text, screenshots, keystrokes, file contents, raw provider log payloads, or
sensitive config values. HTTP logs use path templates, not raw session-key paths
or query strings.

Quick local check:

```sh
rg -i -n "Authorization|Bearer|prompt|response|screenshot|keystroke|file[-_ ]?content|raw provider|payload" /tmp/curb.ndjson
```

Investigate any hit before attaching evidence.

## Evidence Package

Put dogfood artifacts under:

```text
evidence/dogfood/YYYY-MM-DD-<short-slug>/
```

Include:

- command run;
- config path and state path;
- NDJSON log path;
- parser output;
- `/v1/live` and `/v1/ready` responses for headless runs;
- source-health baseline;
- residual risk or follow-up ticket references.

## Timed Headless Observability Smoke

To prove the headless observability loop over real local provider metadata
without enabling termination, run:

```sh
CURB_DOGFOOD_SECONDS=60 bash scripts/dogfood-headless-observability.sh
```

The script builds the release binary, copies the example config into the
evidence directory with private scratch state, disables local notifications,
runs `curb serve --headless --home "$HOME"`, captures public live/ready probes
and protected overview/session/event responses, parses the NDJSON log, and
checks redaction. It requires final readiness HTTP 200, at least one startup
`usage_scan`, and duration-scaled repeated `watcher_tick` events. The script
currently requires at least `max(2, CURB_DOGFOOD_SECONDS / 5)` watcher ticks,
which leaves slack for startup and API-probe overhead while catching stalled
watcher loops.

The script writes artifacts under
`evidence/dogfood/YYYY-MM-DD-headless-observability/` by default. This smoke is
visibility mode only; it must not terminate processes.

For a longer local sidecar window, pass an explicit output directory:

```sh
CURB_DOGFOOD_SECONDS=180 bash scripts/dogfood-headless-observability.sh evidence/dogfood/$(date +%F)-headless-observability-3min
```

The June 4, 2026 three-minute run captured 72 NDJSON events, 59
`watcher_tick` events, final readiness HTTP 200, zero source-health errors, and
clean parser/redaction checks under
`evidence/dogfood/2026-06-04-headless-observability-3min/`.

## Safe Stop-Rejection Smoke

To prove stop logging without terminating anything, run Curb with a
non-enforcement config, fetch a session, and POST a stop request with
`scope:"tree"`. The expected response is `409 Conflict` with an error such as
`enforcement mode is required`, and the NDJSON should contain `stop_rejection`
plus an `api_request` with path template `/v1/sessions/{session_key}/stop`.

Do not switch to enforcement mode for this smoke. Successful termination dogfood
must use a harmless synthetic subprocess and separate evidence.

Related decision: [ADR 0002](../adr/0002-structured-observability-contract.md).

## Headless Enforcement Smoke

To prove successful stop behavior, run the repo-managed enforcement dogfood:

```sh
bash scripts/dogfood-headless-enforcement.sh
```

The script builds the release binary, creates a private synthetic HOME with a
metadata-only Codex usage log, spawns a uniquely marked shell worker, starts
`curb serve --headless`, and sends a protected stop request with the selected
session's PID, start time, owner, and executable identity. The expected response
is `HTTP/1.1 200 OK`; the raw ledger must include `manual_stop_started` and
`manual_stop_completed`; the NDJSON must include `stop_decision` with status
`200`.

The script writes artifacts under
`evidence/dogfood/YYYY-MM-DD-headless-enforcement/` by default. It keeps API
token state under a temporary private directory, deletes that scratch state on
exit, and checks that the structured NDJSON does not contain the token, auth
header, synthetic marker, operator reason, prompt, response, screenshot, or
keystroke terms.
