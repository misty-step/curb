// Design-review capture harness.
//
// Boots the Vite dev build, mocks the local /v1 API with the rich "runaway"
// hero scenario (one session over the kill line, one over warn, one in limits,
// one idle), and screenshots the dashboard across its key states in both color
// modes at desktop and mobile widths. Reduced-motion is forced so every frame
// is settled, not mid-animation.
//
// This is a visual-evidence loop, complementary to scripts/smoke-dashboard.mjs
// (which is pass/fail assertions). Output lands in ui/artifacts/design-review/.
//
//   node ui/scripts/capture-design.mjs            # all states
//   CAPTURE_DIR=/tmp/foo node ui/scripts/capture-design.mjs

import { chromium } from "playwright";
import { createServer } from "vite";
import { mkdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const uiRoot = path.resolve(scriptDir, "..");
// CAPTURE_SCENARIO=degraded reproduces the source-health state so we can verify
// the stripped UI shows one quiet health line, not an operator console.
const DEGRADED = process.env.CAPTURE_SCENARIO === "degraded";
const artifactDir = process.env.CAPTURE_DIR || path.join(uiRoot, "artifacts", "design-review", DEGRADED ? "degraded" : "healthy");

// Anchored to the wall clock so relativeTime() renders realistically
// ("just now", "7m ago", "1h ago") instead of a stale fixed offset.
const iso = (msAgo) => new Date(Date.now() - msAgo).toISOString();
const NOW = iso(8_000);
const A_MIN_AGO = iso(60_000);
const MINS_AGO = iso(7 * 60_000);
const HOUR_AGO = iso(60 * 60_000);

const RECOVERY_ITEMS = [
  { id: "source-codex", label: "codex source", status: "oversized line", message: "codex usage metadata could not be read.", action: "rotate the log" },
  { id: "source-claude", label: "claude source", status: "oversized line", message: "claude usage metadata could not be read.", action: "rotate the log" },
];

const config = {
  path: "/Users/you/.config/curb/config.yaml",
  mode: "enforcement",
  usage_enabled: true,
  warn_turn_tokens: 1_000_000,
  kill_turn_tokens: 3_000_000,
  usage_window_seconds: 900,
  usage_scan_seconds: 5,
  lookback_seconds: 86_400,
  process_warn_seconds: 5_400,
  process_kill_seconds: 7_200,
  ack_extension_seconds: 1_800,
  local_notifications: true,
  escalate_supervised: false,
  agents: [
    { id: "codex-cli", label: "Codex CLI", family: "codex", kind: "process", terminates: true, description: "Codex CLI worker" },
    { id: "claude-code", label: "Claude Code", family: "claude", kind: "process", terminates: true, description: "Claude Code worker" },
  ],
};

const sessions = [
  {
    key: "codex:olympus", id: "olympus", provider: "codex", status: "working", alert: "kill",
    can_stop: true, can_acknowledge: false, project: "olympus", cwd: "/Users/you/dev/olympus",
    models: ["gpt-5-codex"], turn_tokens: 3_300_000, turn_context_tokens: 3_600_000, total_tokens: 8_100_000,
    calls: 52, last_activity_at: NOW, pid: 7731, process_started_at: A_MIN_AGO, owner: "you",
    executable: "/usr/local/bin/codex",
    explanation: "Over your kill line this turn — Curb will stop the worker after the grace period.",
  },
  {
    key: "claude:gradient", id: "gradient", provider: "claude", status: "working", alert: "warn",
    can_stop: false, can_acknowledge: true, project: "gradient", cwd: "/Users/you/dev/gradient",
    models: ["claude-opus-4-8"], turn_tokens: 1_400_000, turn_context_tokens: 1_650_000, total_tokens: 4_200_000,
    calls: 38, last_activity_at: NOW, pid: 4242, process_started_at: HOUR_AGO, owner: "you",
    executable: "/usr/local/bin/claude",
    explanation: "Over your warn line this turn.",
  },
  {
    key: "codex:curb", id: "curb", provider: "codex", status: "working", alert: "ok",
    can_stop: false, can_acknowledge: false, project: "curb", cwd: "/Users/you/dev/curb",
    models: ["gpt-5-codex"], turn_tokens: 320_000, turn_context_tokens: 410_000, total_tokens: 1_900_000,
    calls: 21, last_activity_at: NOW, pid: 5310, process_started_at: A_MIN_AGO, owner: "you",
    executable: "/usr/local/bin/codex",
    explanation: "Working, within your limits.",
  },
  {
    key: "codex:daybook", id: "daybook", provider: "codex", status: "idle", alert: "ok",
    can_stop: false, can_acknowledge: false, project: "daybook", cwd: "/Users/you/dev/daybook",
    models: ["gpt-5-codex"], turn_tokens: 55_000, turn_context_tokens: 120_000, total_tokens: 2_300_000,
    calls: 64, last_activity_at: MINS_AGO, explanation: "Idle between turns.",
  },
];

const turnsByKey = {
  "codex:olympus": [
    {
      id: "turn-3", request_id: "req-3", session_key: "codex:olympus", session_id: "olympus", provider: "codex",
      at: NOW, model: "gpt-5-codex", input_tokens: 1_200_000, cached_input_tokens: 180_000,
      cache_creation_input_tokens: 25_000, output_tokens: 240_000, reasoning_output_tokens: 90_000,
      total_tokens: 1_555_000, spent_tokens: 1_375_000, cumulative_tokens: 3_300_000, source: "codex usage log",
    },
    {
      id: "turn-2", request_id: "req-2", session_key: "codex:olympus", session_id: "olympus", provider: "codex",
      at: A_MIN_AGO, model: "gpt-5-codex", input_tokens: 900_000, cached_input_tokens: 120_000,
      cache_creation_input_tokens: 18_000, output_tokens: 160_000, reasoning_output_tokens: 60_000,
      total_tokens: 1_138_000, spent_tokens: 1_018_000, cumulative_tokens: 1_925_000, source: "codex usage log",
    },
  ],
};

const snapshot = {
  overview: {
    mode: "enforce",
    status: "ACTION",
    message: "1 agent over the kill line",
    working: 3, warn: 1, kill: 1,
    busiest_turn_tokens: 3_300_000,
    last_scan: NOW,
    sources: [
      { provider: "codex", files: 12, events: 340 },
      { provider: "claude", files: 8, events: 210 },
    ],
    recovery: DEGRADED ? RECOVERY_ITEMS : [],
    changes: { new_sessions: 0, sessions_with_new_turns: 1, tokens_added: 62_000, new_alerts: 0, agents_started: 0, agents_ended: 0, source_errors: DEGRADED ? 2 : 0 },
    capabilities: {
      platform: "darwin",
      notifications: { available: true, status: "ready", message: "notifications ready" },
      process_capture: { available: true, status: "ready", message: "process capture available" },
      process_identity: { available: true, status: "ready", message: "identity evidence available" },
      enforcement: { available: !DEGRADED, status: DEGRADED ? "unavailable" : "ready", message: DEGRADED ? "no correlated worker identity to stop" : "enforcement armed for correlated workers" },
    },
  },
  agents: sessions
    .filter((s) => s.status === "working")
    .map((s) => ({
      id: `${s.provider}-worker`, provider: s.provider, label: s.project, status: s.status,
      pid: s.pid, process_started_at: s.process_started_at, running_for_seconds: 600,
      project: s.project, cwd: s.cwd, session_key: s.key, turn_tokens: s.turn_tokens, explanation: s.explanation,
    })),
  sessions,
  turns: turnsByKey["codex:olympus"],
};

const notifications = {
  enabled: true,
  available: !DEGRADED,
  status: DEGRADED ? "error" : "ready",
  message: DEGRADED ? "Notification permission was denied." : "Notifications are ready.",
};

const onboarding = {
  completed: true,
  config_path: config.path,
  steps: [
    { id: "config", label: "Configuration", status: "ready", message: "Using your config at ~/.config/curb/config.yaml." },
    { id: "sources", label: "Agent sources", status: "ready", message: "Reading Codex and Claude Code usage logs." },
    { id: "notifications", label: "Notifications", status: "ready", message: "Local notifications are enabled." },
  ],
};

const ready = { ready: true, checks: [] };

const routes = {
  "/v1/snapshot": () => snapshot,
  "/v1/service/rescan": () => snapshot,
  "/v1/config": () => config,
  "/v1/notifications/health": () => notifications,
  "/v1/onboarding": () => onboarding,
  "/v1/ready": () => ready,
  "/v1/alerts": () => [],
};

function payloadFor(pathname) {
  const turns = pathname.match(/^\/v1\/sessions\/(.+)\/turns$/);
  if (turns) return turnsByKey[decodeURIComponent(turns[1])] ?? [];
  const session = pathname.match(/^\/v1\/sessions\/(.+)$/);
  if (session) {
    const key = decodeURIComponent(session[1]);
    return sessions.find((s) => s.key === key) ?? null;
  }
  const route = routes[pathname];
  return route ? route() : undefined;
}

const viewports = [
  { name: "desktop", width: 1440, height: 900 },
  { name: "mobile", width: 390, height: 844 },
];
const modes = ["light", "dark"];

const server = await createServer({ root: uiRoot, logLevel: "error", server: { host: "127.0.0.1", port: 0 } });
await server.listen();
const baseURL = server.resolvedUrls?.local?.[0];
if (!baseURL) throw new Error("Vite did not report a local URL");

const browser = await chromium.launch();
await rm(artifactDir, { recursive: true, force: true });
await mkdir(artifactDir, { recursive: true });
const shots = [];
const fits = [];

try {
  for (const mode of modes) {
    for (const vp of viewports) {
      const context = await browser.newContext({
        viewport: { width: vp.width, height: vp.height },
        // scale 1 keeps desktop captures within the image-read size limit
        deviceScaleFactor: 1,
        colorScheme: mode,
        reducedMotion: "reduce",
      });
      await context.addInitScript((m) => {
        try { localStorage.setItem("ae-mode", m); } catch { /* private mode */ }
      }, mode);
      const page = await context.newPage();
      await page.route("**/v1/**", async (route) => {
        const url = new URL(route.request().url());
        const payload = payloadFor(url.pathname);
        if (payload === undefined) {
          await route.fulfill({ status: 404, contentType: "application/json", body: JSON.stringify({ error: "unhandled" }) });
          return;
        }
        await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(payload) });
      });

      const tag = `${mode}-${vp.name}`;
      await page.goto(baseURL, { waitUntil: "domcontentloaded" });
      await page.getByText("olympus", { exact: false }).first().waitFor({ state: "visible", timeout: 10_000 });

      // No-scrollbar check: the stage must not overflow vertically at rest, and
      // the document must never overflow horizontally.
      const fit = await page.evaluate(() => {
        const s = document.querySelector(".ae-stage");
        const de = document.documentElement;
        return { vScroll: s ? s.scrollHeight - s.clientHeight : 0, hScroll: de.scrollWidth - de.clientWidth };
      });
      fits.push(`${tag}: vScroll=${fit.vScroll}px hScroll=${fit.hScroll}px`);

      await capture(page, `${tag}-01-overview`);
      await capture(page, `${tag}-01-overview-full`, { unroll: true });

      // Expand the runaway session.
      await page.getByText("olympus", { exact: false }).first().click();
      await page.getByText("ALERT & CORRELATION EVIDENCE", { exact: false }).first().waitFor({ state: "visible", timeout: 10_000 });
      await capture(page, `${tag}-02-session-expanded-full`, { unroll: true });
      await capture(page, `${tag}-02-session-expanded`);

      // Stop confirmation dialog.
      await page.getByText("Stop now", { exact: false }).first().click();
      await page.locator("dialog.ae-dialog").waitFor({ state: "visible", timeout: 10_000 });
      await capture(page, `${tag}-03-stop-dialog`);
      await page.getByText("Cancel", { exact: false }).first().click();
      // Collapse the session again.
      await page.getByText("olympus", { exact: false }).first().click();

      // Settings drawer.
      await page.getByText("Limits & mode", { exact: false }).first().click();
      await page.getByText("A turn is the work an agent does", { exact: false }).first().waitFor({ state: "visible", timeout: 10_000 });
      await capture(page, `${tag}-04-settings-full`, { unroll: true });

      await context.close();
    }
  }
} finally {
  await browser.close();
  await server.close();
}

await writeFile(path.join(artifactDir, "manifest.json"), `${JSON.stringify({ baseURL, modes, viewports, shots, fits }, null, 2)}\n`);
console.log(`design capture ok: ${artifactDir}\n${shots.length} screenshots\nfit: ${fits.join(" | ")}`);

// The app shell is a fixed 100dvh screen that scrolls *internally* (.ae-stage
// has overflow-y:auto), so Playwright's fullPage — which only captures document
// scroll height — clips to one viewport. `unroll` injects a style that lets the
// shell grow to its content so a fullPage shot captures everything below the
// fold; it is removed immediately after.
async function capture(page, name, { fullPage = false, unroll = false } = {}) {
  let style;
  if (unroll) {
    style = await page.addStyleTag({
      content: ".ae-screen{height:auto!important;overflow:visible!important}.ae-stage{overflow:visible!important}",
    });
    await page.waitForTimeout(50);
  }
  const file = path.join(artifactDir, `${name}.png`);
  await page.screenshot({ path: file, fullPage: fullPage || unroll });
  if (style) await style.evaluate((node) => node.remove());
  shots.push(path.relative(uiRoot, file));
}
