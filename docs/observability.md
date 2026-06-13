# Observability

Curb emits optional local process logs for headless and dogfood runs. These logs
are separate from the append-only ledger: the ledger is the product record, and
process logs are operator evidence for startup, HTTP, and runtime health.

Set `CURB_LOG_FORMAT=json` to emit newline-delimited JSON to stderr:

```sh
CURB_LOG_FORMAT=json ./target/release/curb serve --headless --addr 127.0.0.1:8765 2> curb.ndjson
```

## Schema v1

Each line is one JSON object with `schema_version: 1`.

Required top-level fields:

- `schema_version`: integer.
- `timestamp`: RFC3339 UTC timestamp.
- `level`: `info`, `warn`, or `error`.
- `component`: stable producer name.
- `event`: stable event name from the registry below.
- `outcome`: `ok`, `degraded`, `rejected`, or `error`.

Optional top-level fields:

- `duration_ms`: non-negative integer duration for timed events.
- `request_id`: process-local opaque request id.
- `session_key`: sanitized session key when a policy event needs one.
- `reason`: sanitized short explanation.
- `fields`: event-specific JSON object extension point.

Redaction rules:

- Do not log API token values, authorization headers, prompt text, response
  text, screenshots, keystrokes, or file contents.
- HTTP logs use path templates, not raw session-key paths or query strings.
- Config paths and server URLs are allowed; sensitive config values are not.
- Provider usage metadata may include counts and source-health status, not raw
  provider log payloads.

Backwards-compatible changes may add new optional fields or new event names.
Renaming/removing required fields or changing an event's meaning requires a new
schema version.

## Event Registry

| Event | Component | Meaning |
|---|---|---|
| `server_started` | `server` | Local server bound loopback and entered serve mode. |
| `config_loaded` | `config` | Config loaded and validated. |
| `api_request` | `http` | API or UI HTTP request completed. |
| `health_check` | `http` | Health route completed. |
| `readiness_check` | `http` | Readiness route completed. |
| `usage_scan` | `runtime` | One usage scan produced a snapshot and policy outcome summary. |
| `watcher_tick` | `runtime` | Watch loop tick produced a snapshot and policy outcome summary. |
| `notification_attempt` | `runtime` | Local notification delivery was attempted. |
| `stop_decision` | `policy` | Policy accepted or would accept a stop decision. |
| `stop_rejection` | `policy` | Stop request or policy stop was rejected. |
| `source_health_error` | `runtime` | Provider usage source reported an error. |
| `shutdown` | `server` | Server or watcher shutdown started. |

## Latency Expectations

`/v1/live` and `/v1/ready` are public headless probes. They must be cheap and
must not start a usage rescan, terminate processes, or mutate runtime state.
Readiness checks may return `degraded` when a dependency is busy; that is
preferred over blocking behind a scan or cache owner. Request logs include
`duration_ms` so dogfood evidence can catch slow probes.

Example readiness request log:

```json
{"schema_version":1,"level":"info","component":"http","event":"readiness_check","outcome":"ok","duration_ms":0,"request_id":"req-4","fields":{"method":"GET","path_template":"/v1/ready","status":200}}
```

Example startup event:

```json
{"schema_version":1,"level":"info","component":"config","event":"config_loaded","outcome":"ok","fields":{"path":"configs/curb.example.yaml","mode":"visibility","agent_count":6}}
```

Example usage scan event:

```json
{"schema_version":1,"level":"info","component":"runtime","event":"usage_scan","outcome":"ok","duration_ms":16,"fields":{"status":"OK","working":0,"warn":0,"kill":0,"source_errors":0,"event_count":0,"process_count":0,"observed_sessions":0,"policy_warnings":0,"would_stop":0,"stop_blocked":0,"grace_started":0,"grace_pending":0,"stop_attempted":0,"stop_completed":0,"stop_rejected":0,"resumed_sessions":0,"terminated_sessions":0}}
```

Runtime scan and watcher logs include the policy summary produced by the
governor for that tick. `grace_started` and `grace_pending` expose the waiting
state before enforcement, while `stop_attempted`, `stop_completed`, and
`stop_rejected` expose the stop revalidation outcome without logging raw
process command lines or operator reasons.

Example initial readiness response before the first snapshot exists:

```json
{"status":"degraded","app":"curb","api_version":1,"checks":[{"name":"watcher_runtime","status":"error","reason":"cache busy"}]}
```

After a first snapshot exists, cache contention stays ready and names the
cached-snapshot fallback instead of turning a responsive service into a 503:

```json
{"status":"ready","app":"curb","api_version":1,"checks":[{"name":"watcher_runtime","status":"ok","reason":"snapshot refresh in progress; serving cached snapshot"}],"recovery":[]}
```

Attach dogfood artifacts under
`evidence/dogfood/YYYY-MM-DD-<short-slug>/` with the command, stderr NDJSON
path, and the JSON parser command used to validate it.

Use the repo smoke parser for local artifacts:

```sh
curl -fsS -H "Authorization: Bearer $(cat token-file)" http://127.0.0.1:8765/v1/health
curl -fsS http://127.0.0.1:8765/v1/ready
scripts/parse-observability-smoke.py /tmp/curb.ndjson
```

When the server is stopped with `Ctrl-C`, the accept loop exits through the
shutdown flag and emits:

```json
{"schema_version":1,"level":"info","component":"server","event":"shutdown","outcome":"ok","reason":"serve loop exited"}
```
