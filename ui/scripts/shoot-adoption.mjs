// Adoption evidence shots: the deterministic fixture-backed dashboard at
// 1280x800 in both color schemes — the agent list, an open session detail,
// and the settings drawer. Mirrors smoke-dashboard.mjs API routing so the
// frames are reproducible from committed contracts.
//
//   CURB_SHOT_PREFIX=before node scripts/shoot-adoption.mjs
//   CURB_SHOT_PREFIX=after  node scripts/shoot-adoption.mjs
//
// Writes ../docs/adoption/<prefix>-<view>-<scheme>.png

import { chromium } from "playwright";
import { createServer } from "vite";
import { mkdir, readFile } from "node:fs/promises";
import path from "node:path";

const prefix = process.env.CURB_SHOT_PREFIX || "shot";
const outDir = process.env.CURB_SHOT_DIR || path.join("..", "docs", "adoption");
const viewport = { width: 1280, height: 800 };
const routes = await loadRoutes();

const server = await createServer({
  root: process.cwd(),
  logLevel: "error",
  server: { host: "127.0.0.1", port: 0 },
});
await server.listen();
const baseURL = server.resolvedUrls?.local?.[0];
if (!baseURL) throw new Error("Vite did not report a local URL");

const browser = await chromium.launch();
await mkdir(outDir, { recursive: true });

try {
  for (const colorScheme of ["light", "dark"]) {
    const page = await browser.newPage({ viewport, colorScheme });
    await routeApi(page);
    await page.goto(baseURL, { waitUntil: "networkidle" });
    await page.getByText("repo", { exact: false }).first().waitFor({ state: "visible" });
    await shoot(page, "list", colorScheme);

    await page.getByText("repo", { exact: false }).first().click();
    await page.getByText("Stop now", { exact: false }).first().waitFor({ state: "visible" });
    await shoot(page, "detail", colorScheme);

    await page.getByText("Limits & mode", { exact: false }).first().click();
    await page.getByText("Warn at", { exact: false }).first().waitFor({ state: "visible" });
    await shoot(page, "settings", colorScheme);

    await page.close();
  }
} finally {
  await browser.close();
  await server.close();
}
console.log(`adoption shots ok: ${outDir} (${prefix}-*)`);

async function shoot(page, view, colorScheme) {
  // Let entrance feedback resolve before the frame.
  await page.waitForTimeout(700);
  await page.screenshot({ path: path.join(outDir, `${prefix}-${view}-${colorScheme}.png`) });
}

async function loadRoutes() {
  const snapshot = stoppableSnapshot(await readJson("../../contracts/api/snapshot.json"));
  const session = stoppableSession(await readJson("../../contracts/api/session.json"));
  return {
    "/v1/snapshot": snapshot,
    "/v1/config": await readJson("../../contracts/api/config.json"),
    "/v1/notifications/health": {
      enabled: true,
      available: true,
      status: "ready",
      message: "Notifications are ready.",
    },
    "/v1/onboarding": await readJson("../../contracts/api/onboarding.json"),
    "/v1/ready": await readJson("../../contracts/api/ready.json"),
    "/v1/service/rescan": snapshot,
    "/v1/alerts": [],
    session,
  };
}

async function readJson(relativePath) {
  return JSON.parse(await readFile(new URL(relativePath, import.meta.url), "utf8"));
}

function stoppableSnapshot(snapshot) {
  return {
    ...snapshot,
    sessions: snapshot.sessions.map((entry, index) => (index === 0 ? stoppableSession(entry) : entry)),
  };
}

function stoppableSession(session) {
  return {
    ...session,
    can_acknowledge: false,
    can_stop: true,
    explanation: "Over your kill line - stopping after the grace period.",
  };
}

async function routeApi(page) {
  await page.route("**/v1/**", async (route) => {
    const url = new URL(route.request().url());
    const payload = await payloadFor(url.pathname);
    await route.fulfill({
      status: payload === undefined ? 404 : 200,
      contentType: "application/json",
      body: JSON.stringify(payload ?? { error: "unhandled shot route" }),
    });
  });
}

async function payloadFor(pathname) {
  if (/^\/v1\/sessions\/[^/]+\/turns$/.test(pathname)) {
    return readJson("../../contracts/api/turns.json");
  }
  if (/^\/v1\/sessions\/[^/]+$/.test(pathname)) {
    return routes.session;
  }
  return routes[pathname];
}
