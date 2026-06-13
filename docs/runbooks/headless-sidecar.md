# Headless Sidecar Runbook

Use this runbook when Curb runs beside headless server applications or long-lived
agent hosts.

## Start

Build or use an installed binary, then start the loopback service:

```sh
cargo build --release --bin curb
CURB_LOG_FORMAT=json ./target/release/curb serve --headless --addr 127.0.0.1:8765 --config configs/curb.example.yaml 2>/tmp/curb-headless.ndjson
```

Headless mode does not open a browser and does not serve the dashboard UI from
root routes. It keeps protected API routes token-gated.

## Probes

Liveness is public:

```sh
curl -fsS http://127.0.0.1:8765/v1/live
```

Readiness is public and reports dependency state:

```sh
curl -fsS http://127.0.0.1:8765/v1/ready
```

Treat `ready` as usable. Treat `degraded` as diagnostic, not necessarily fatal:
the service may be bound while the first scan is still populating the runtime
cache, or while a bounded cache read is busy. Readiness must not trigger a scan,
terminate a process, or mutate service state.

## Protected API Check

Protected routes require the API token:

```sh
curl -i http://127.0.0.1:8765/v1/health
curl -fsS -H "Authorization: Bearer $(cat .curb/api.token)" http://127.0.0.1:8765/v1/health
```

Use the token path printed by the service at startup or the configured state
directory; `.curb/api.token` is the default relative path for this runbook's
command. Do not paste token values into dogfood notes.

## Observability

Validate the captured NDJSON before attaching it to evidence:

```sh
python3 scripts/parse-observability-smoke.py /tmp/curb-headless.ndjson
```

Attach logs and probe responses under
`evidence/dogfood/YYYY-MM-DD-<short-slug>/`.

## Failure Triage

- `degraded` before the first snapshot: the server is responsive, but the
  watcher has not produced an operator snapshot yet. Retry and inspect
  `usage_scan` timing.
- `ready` with `snapshot refresh in progress; serving cached snapshot`:
  expected during cache contention after the first good snapshot.
- `usage_reader` error: inspect provider root availability and source-health
  events. Do not add prompt/response logging to debug it.
- protected route returns `401`: token lookup or request auth is wrong; public
  probes are not proof that protected API access works.
- root route returns headless JSON/error: expected in headless mode.

## Verification

```sh
cargo test --bin curb headless -- --nocapture
cargo test --bin curb auth -- --nocapture
python3 scripts/parse-observability-smoke.py /tmp/curb-headless.ndjson
```

Related decision: [ADR 0001](../adr/0001-headless-service-contract.md).
