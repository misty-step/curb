# Adopt @misty-step/aesthetic as the dashboard design system

curb is the reference instrument-panel consumer — it was already
ink-on-paper minimal, so the design system is its true costume. This
adopts aesthetic across the embedded dashboard, retiring ~1,450 lines
of hand-rolled CSS for the system's tokens, primitives, and the app
shell.

## The argument

curb's steering block is **empty** — this is the doctrine's quietest
case. Identity comes entirely from composition: the app shell, the
ruled meters, the house status triplet (curb's `ok/warn/kill` map onto
`--ae-ok/--ae-warn/--ae-err`; the kill action takes `--ae-err`). No
accent is defined and none is needed.

## What changed

- **`ui/src/styles.css`** — ~1,450 lines of bespoke tokens, panels,
  pills, gauges, and radii deleted; what remains is layout-only glue
  over the system.
- **`ui/src/aesthetic.css`** — the system vendored (v2.5.1, header
  comment marks the version; upgrade by replacing the file) and
  imported first in `main.tsx`. The embedded dashboard is offline-
  capable, so vendoring with a pinned header is the honest path over a
  network dependency; Geist loads from Google Fonts with the system's
  own `ui-monospace`/system fallback so the embed degrades cleanly.
- **Composition** — the dashboard becomes the **app shell**: hairline
  divisions instead of nested rounded cards; radii 6/9/13 → 0. The
  spend gauge is now an **`.ae-meter`** (a ruled line with a threshold
  `.ae-meter-mark`, the fill taking `ae-warn`/`ae-err` only when the
  level is the signal); figures are **`.ae-num`** tabular; status chips
  become **`.ae-tag`** + status glyphs (no filled pills); the
  watch/enforce control reads as **`.ae-tabs`**; the settings drawer
  uses **`.ae-fold`** and the stop-confirm is an **`.ae-dialog`**;
  buttons are `.ae-button`/`-quiet`/`-compact`. New `mode.tsx`
  (the `.ae-mode` toggle, recipes/mode.js choreography) and
  `settings.tsx`.

## Verification

- `npm run typecheck` · `npm run lint` · `npm test` (29 passing) ·
  `npm run build` — all green. Tests gained a jsdom `showModal`/`close`
  polyfill (`src/test-setup.ts`) since jsdom lacks `<dialog>`.
- `scripts/build-ui.sh --check` — the embed gate passes; `web/dist` is
  a fresh build of the new dashboard.
- `cargo build` — the Rust host compiles and embeds the new assets.
- `npm run smoke` — dashboard smoke ok.
- Visual: deterministic fixture-backed shots at 1280×800, both modes,
  three views (list, detail, settings) via `scripts/shoot-adoption.mjs`.

### Before / after — the agent list (light)

Nested rounded cards, a filled amber "WATCH" pill, a segmented gauge,
an "over warn" pill → hairline divisions, a single ruled meter with a
threshold mark, the warning glyph carrying the hue, an `over warn`
tag, the letterspaced `CURB` wordmark, radius 0 throughout:

![before](docs/adoption/before-list-light.png)
![after](docs/adoption/after-list-light.png)

Settings, detail, and both dark variants are in `docs/adoption/`.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
