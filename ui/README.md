# Curb UI

This is the first desktop-app UI shell for Curb. It is a React/Vite dashboard
that consumes the local `curb daemon` API and keeps all policy, process
matching, token parsing, and enforcement logic in Go.

## Run

Normal product launch:

```sh
go build -o /tmp/curb ./cmd/curb
/tmp/curb app --addr 127.0.0.1:8765
```

The Go binary serves the built dashboard and `/v1/*` API from the same loopback
origin.

For frontend development, start the daemon:

```sh
go build -o /tmp/curb ./cmd/curb
/tmp/curb daemon --addr 127.0.0.1:8765
```

Then start Vite:

```sh
cd ui
npm install
npm run dev
```

Open `http://127.0.0.1:5173` and connect. The embedded `curb app` dashboard
authenticates automatically with a same-origin HttpOnly cookie. Vite
development can use a pasted token from `<state_dir>/api.token`, or demo mode
with no API base URL.

Without an API base URL, the UI renders demo data. That keeps the layout
reviewable without process permissions or local agent logs.

## Boundaries

- The UI does not read provider logs directly.
- The UI does not inspect processes directly.
- The UI does not reimplement policy or actionability.
- Prompt and response content are not rendered.
- `state`, `usage_state`, `actionable`, and `can_acknowledge` are displayed or
  acted on exactly as the API provides them.
- Selected-session turn history comes from `/v1/sessions/{key}/turns`, not from
  frontend filtering of the snapshot's policy-window turns.
- Dashboard data is fetched from `/v1/snapshot` so overview, agents, and
  sessions are rendered from one coherent cached read model.
- The manual refresh button calls `POST /v1/service/rescan`; polling continues
  to read the cached snapshot.
- Configuration is fetched from `/v1/config` and saved through `PUT /v1/config`.
  The UI sends only a narrow policy DTO; Go validates and persists the YAML.
- The selected session shows both an at-a-glance token timeline and a turn table
  with input, cached input, output, reasoning, and total token columns.
- Alerts are fetched from `/v1/alerts`, a service-owned projection over ledger
  events. The UI does not classify raw event types, and alert Extend buttons use
  only service-projected `session_key` plus `can_acknowledge`.

## Verify

```sh
npm test
npm run build
../scripts/build-ui.sh
../scripts/build-ui.sh --check
```

`npm run build` writes `ui/dist` for local frontend checks. The Go binary embeds
`internal/web/dist`, so use `../scripts/build-ui.sh` before testing or shipping
`curb app`.
