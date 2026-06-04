# Dogfood Run: 2026-06-03 local release proof

## Run Metadata

- Build SHA: `6bff6ce2c72ff19bc4c003e611a44d3976d6f780`
- Branch/worktree: detached worktree at `<home>/.codex/worktrees/066e/curb`
- OS: macOS 26.5.1 build 25F80, Darwin 25.5.0 arm64
- Curb command(s):
  - `cargo build --release --bin curb`
  - `./target/release/curb validate-config configs/curb.example.yaml`
  - `./target/release/curb usage --since 24h`
  - `./target/release/curb serve --addr 127.0.0.1:8765 --config configs/curb.example.yaml`
  - unauthenticated `curl` probes for `/v1/health` and `/`
  - `./target/release/curb doctor --config configs/curb.example.yaml`
- Config path: `configs/curb.example.yaml`
- State path: `.curb`
- Mode: release-build dogfood, non-enforcement proof; example config mode is `visibility`
- Operator: Codex

## Commands

```sh
cargo build --release --bin curb
# Finished release profile in 3m 41s.

./target/release/curb validate-config configs/curb.example.yaml
# ok config=configs/curb.example.yaml mode=visibility agents=6 ledger=.curb/runs.ndjson

./target/release/curb usage --since 24h
# curb usage
#   sources: codex 0 events; claude 0 events; pi 0 events
#   sessions: 0

./target/release/curb serve --addr 127.0.0.1:8765 --config configs/curb.example.yaml
# curb rust app
#   listening: http://127.0.0.1:8765/
#   token: .curb/api.token
#   auth: Authorization: Bearer $(cat token-file)
#   watcher: usage policy scans run in this process

curl -i --max-time 5 http://127.0.0.1:8765/v1/health
# HTTP/1.1 401 Unauthorized
# {"error":"unauthorized"}

curl -i --max-time 5 http://127.0.0.1:8765/ | head -20
# HTTP/1.1 200 OK
# content-type: text/html; charset=utf-8
# set-cookie: curb_token=<redacted>; Path=/v1/; HttpOnly; SameSite=Strict
# embedded Vite index served

./target/release/curb doctor --config configs/curb.example.yaml
# config: ok configs/curb.example.yaml
# state_dir: ok .curb
# ledger: ok .curb/runs.ndjson
# process_snapshot: ok processes=926 platform=macos
# notifications: available macOS user notifications available through osascript
```

## Source-Health Baseline

| Provider | Expected | Observed | Files | Events | Source-health notes |
|---|---|---|---:|---:|---|
| codex | local metadata may exist | source scanned | not reported by CLI | 0 | No 24h events reported. Need dogfood during an active agent run to prove live fidelity. |
| claude | local metadata may exist | source scanned | not reported by CLI | 0 | No 24h events reported. |
| pi | local metadata may exist | source scanned | not reported by CLI | 0 | No 24h events reported. |

## Startup And Install Friction

- Build: release build succeeded, but cold release compilation took 3m41s.
- Config selection: explicit `configs/curb.example.yaml` validated successfully.
- State directory: `.curb`; API token created at `.curb/api.token` with `rw-------`.
- Port/API startup: server listened on `127.0.0.1:8765`; port was clear after terminating the proof server.
- Toolchain issues: local `ui/node_modules` had to be restored earlier with
  `npm ci` before validation; release build itself did not require additional
  UI setup because `web/dist` was already committed and current.

## UI Or Operator Clarity

- Screen or command surface used: command-line release proof plus embedded root
  HTML probe.
- What was clear: startup output names listener, token path, auth header shape,
  and watcher ownership.
- What was confusing: `/v1/health` is auth-protected and returns 401 without
  token, which is safe but makes unauthenticated readiness probing impossible
  without a separate liveness/readiness contract.
- Missing operator action: no first-class headless mode or unauthenticated
  liveness endpoint exists yet.

## Notification And Enforcement Safety

- Notification capability: `doctor` reported macOS notifications available via
  `osascript`.
- Enforcement mode: no enforcement action was run; example config validated in
  `visibility` mode.
- Correlated workers: no usage sessions in the 24h metadata window.
- Watch-only app roots: not exercised in this proof.
- Stop/revalidation observations: no stop action was run in this proof.

## Privacy Confirmation

Confirm each stayed absent from command output, logs, UI, and evidence:

- Prompt content: absent from captured command output.
- Response content: absent from captured command output.
- Screenshots: absent; no screenshot capture performed.
- Keystrokes: absent.
- File contents: absent, aside from paths and embedded HTML shell metadata.

## False Positives / False Negatives

- False positives: none observed because no sessions were present.
- False negatives: possible; the 24h window reported zero events despite active
  local agent usage elsewhere in the environment, so a real active-session
  dogfood run is still needed.
- Process-correlation surprises: not exercised because no sessions were present.
- Noisy roots: none observed.

## Backlog Implications

| Rank | Backlog item | Evidence line(s) | Acceptance oracle |
|---:|---|---|---|
| 1 | `025-headless-server-contract.md` | `/v1/health` returned 401 without auth; server root served UI by default; startup output says watcher runs in-process. | Separate headless mode, liveness/readiness, and UI-serving behavior with explicit safety tests. |
| 2 | `026-structured-observability.md` | Startup output is human-readable text, not schema-stable JSON. | Versioned NDJSON schema and event registry for startup, request, health, watcher, and stop decisions. |
| 3 | `027-quality-gates-and-contract-tests.md` | Earlier full `scripts/validate.sh` run exposed a real-process E2E flake; focused reruns passed. | Instrument/stabilize E2E enforcement failures and classify mandatory vs report-only gates. |
| 4 | `024-dogfood-evidence-matrix.md` follow-up | `usage --since 24h` reported zero events. | Capture a second dogfood run during an active agent session to prove live usage fidelity. |

## Follow-Up Implementation Proof

`025-headless-server-contract.md` was implemented after this initial dogfood
run. The debug proof command:

```sh
cargo run -- serve --headless --addr 127.0.0.1:8765 --config configs/curb.example.yaml
```

reported `curb headless server`, `ui: disabled`, `/v1/live`, and `/v1/ready`.
Route probes then showed:

- `GET /v1/live` -> `200 {"status":"live","app":"curb","api_version":1}`.
- `GET /v1/ready` -> `200` with `config`, `ledger`, `usage_reader`,
  `platform_capabilities`, `notifications`, and `watcher_runtime` checks.
- `GET /` -> `404 {"error":"headless server","app":"curb","ui":false}`.
- unauthenticated `GET /v1/health` -> `401 {"error":"unauthorized"}`.

The full repository gate passed afterward:

```sh
scripts/validate.sh
# scripts/validate.sh  123.30s user 11.84s system 212% cpu 1:03.66 total
```

## Structured Observability Proof

`026-structured-observability.md` now has a first local NDJSON slice for
headless dogfood runs. The direct-binary smoke used an ephemeral loopback port:

```sh
CURB_LOG_FORMAT=json target/debug/curb serve --headless --addr 127.0.0.1:0 --config configs/curb.example.yaml 2>/tmp/curb-observability-clean.ndjson
curl -s -o /tmp/curb-live-clean.json -w 'live:%{http_code}\n' http://127.0.0.1:63044/v1/live
curl -s -o /tmp/curb-ready-clean.json -w 'ready:%{http_code}\n' http://127.0.0.1:63044/v1/ready
curl -s -o /tmp/curb-health-clean.json -w 'health:%{http_code}\n' http://127.0.0.1:63044/v1/health
```

Observed route statuses:

- `live:200`
- `ready:200`
- `health:401`

The parser proof required every line to be JSON and required the stable fields
`schema_version`, `timestamp`, `level`, `component`, `event`, and `outcome`.
It also asserted that `Authorization`, `Bearer`, raw query tokens, and raw
session-key test content were absent.

```sh
python3 - <<'PY'
import json
from pathlib import Path
objs=[]
for line in Path('/tmp/curb-observability-clean.ndjson').read_text().splitlines():
    if not line.strip():
        continue
    obj=json.loads(line)
    assert {'schema_version','timestamp','level','component','event','outcome'} <= obj.keys(), obj
    objs.append(obj)
events={o['event'] for o in objs}
assert {'usage_scan','server_started','api_request','readiness_check','health_check'} <= events, events
text='\n'.join(json.dumps(o) for o in objs)
for forbidden in ['Authorization','Bearer','token=','secret-session']:
    assert forbidden not in text, forbidden
ready=[o for o in objs if o['event']=='readiness_check'][-1]
print('ok json events', len(objs), 'ready_duration_ms', ready.get('duration_ms'), ','.join(o['event'] for o in objs))
PY
# ok json events 5 ready_duration_ms 5020 usage_scan,server_started,api_request,health_check,readiness_check
```

Residual observability risk: the request log made readiness latency visible, and
this smoke still showed a slow readiness response (`duration_ms=5020`). The
runtime cache readiness check now uses `try_lock` so it reports busy/degraded
instead of waiting on that mutex, but the remaining readiness delay needs a
follow-up root-cause pass before claiming the readiness path is fast.

Follow-up readiness latency probe:

```sh
CURB_LOG_FORMAT=json target/debug/curb serve --headless --addr 127.0.0.1:0 --config configs/curb.example.yaml 2>/tmp/curb-ready-latency.ndjson
python3 - <<'PY'
import time, urllib.request
for path in ['v1/live','v1/ready','v1/health']:
    start=time.perf_counter()
    try:
        with urllib.request.urlopen(f'http://127.0.0.1:50486/{path}', timeout=15) as response:
            body=response.read().decode()
            status=response.status
    except Exception as error:
        status=getattr(error, 'code', 'error')
        body=str(error)
    print(path, status, round((time.perf_counter()-start)*1000, 2), body[:160])
PY
# v1/live 200 13.66 {"status":"live","app":"curb","api_version":1}
# v1/ready 503 1.19 HTTP Error 503: OK
# v1/health 401 0.23 HTTP Error 401: Unauthorized
```

The first readiness probe returned a fast degraded response while the runtime
cache was busy; a later probe returned `200` and logged
`"event":"readiness_check","duration_ms":0`. This narrows the earlier
`duration_ms=5020` concern to startup/cache contention rather than a readiness
handler that performs blocking work. A regression test now locks the cache and
asserts readiness reports `degraded` with `reason: "cache busy"` instead of
waiting.

The next NDJSON smoke after adding config/source-health/stop/notification event
hooks parsed five events:

```sh
python3 - <<'PY'
import json
from pathlib import Path
objs=[]
for line in Path('/tmp/curb-observability-032.ndjson').read_text().splitlines():
    if not line.strip():
        continue
    obj=json.loads(line)
    assert {'schema_version','timestamp','level','component','event','outcome'} <= obj.keys(), obj
    objs.append(obj)
events={o['event'] for o in objs}
assert {'config_loaded','usage_scan','server_started','readiness_check','api_request'} <= events, events
text='\n'.join(json.dumps(o) for o in objs)
for forbidden in ['Authorization','Bearer','token=']:
    assert forbidden not in text, forbidden
ready=[o for o in objs if o['event']=='readiness_check'][-1]
print('ok json events', len(objs), 'ready_status', ready['fields']['status'], 'ready_duration_ms', ready.get('duration_ms'), ','.join(o['event'] for o in objs))
PY
# ok json events 5 ready_status 503 ready_duration_ms 0 config_loaded,usage_scan,server_started,readiness_check,api_request
```

Important distinction: the same smoke showed `config_loaded` at
`20:36:25.701Z` and `server_started` at `20:36:47.460Z`, so the remaining
operator-perceived delay is startup usage scanning before the listener binds,
not the `/v1/ready` handler itself.

Follow-up startup-order fix:

```sh
CURB_LOG_FORMAT=json target/debug/curb serve --headless --addr 127.0.0.1:0 --config configs/curb.example.yaml 2>/tmp/curb-startup-bind.ndjson
python3 - <<'PY'
import time, urllib.request
for path in ['v1/live','v1/ready']:
    start=time.perf_counter()
    try:
        with urllib.request.urlopen(f'http://127.0.0.1:52269/{path}', timeout=10) as response:
            body=response.read().decode()
            status=response.status
    except Exception as error:
        status=getattr(error, 'code', 'error')
        body=str(error)
    print(path, status, round((time.perf_counter()-start)*1000, 2), body[:240])
PY
# v1/live 200 13.96 {"status":"live","app":"curb","api_version":1}
# v1/ready 503 1.49 HTTP Error 503: OK

sleep 25
# v1/ready-after-scan 200 22.23 {"status":"ready","app":"curb","api_version":1,...}

scripts/parse-observability-smoke.py /tmp/curb-startup-bind.ndjson
# ok observability events 5 config_loaded,server_started,api_request,readiness_check,usage_scan
```

The log now shows `config_loaded` and `server_started` one millisecond apart,
then the first `usage_scan` later. Headless integrations can connect
immediately, observe degraded readiness during startup scan, and proceed once
readiness turns `200`.
