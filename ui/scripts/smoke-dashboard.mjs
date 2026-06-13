import { chromium } from "playwright";
import { createServer } from "vite";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";

const artifactDir = process.env.CURB_SMOKE_ARTIFACTS || path.join("artifacts", "smoke-dashboard");
const externalURL = process.env.CURB_SMOKE_URL;
const viewports = [
  { name: "desktop", width: 1440, height: 900 },
  { name: "narrow", width: 390, height: 844 },
];
const routes = await loadRoutes();

let server;
const baseURL = externalURL || (await startVite()).url;
const browser = await chromium.launch();
const failures = [];
const screenshots = [];
await rm(artifactDir, { recursive: true, force: true });
await mkdir(artifactDir, { recursive: true });

try {
  for (const viewport of viewports) {
    const page = await browser.newPage({ viewport });
    await routeApi(page);
    const screenshot = path.join(artifactDir, `dashboard-${viewport.name}.png`);
    try {
      await page.goto(baseURL, { waitUntil: "networkidle" });
      await expectVisibleText(page, "Curb", viewport.name);
      await expectVisibleText(page, "repo", viewport.name);
      await expectVisibleText(page, "over warn", viewport.name);
      await expectVisibleText(page, "Limits & mode", viewport.name);
      await expectVisibleText(page, "Using safe defaults", viewport.name);
      await expectVisibleText(page, "Recovery", viewport.name);
      await expectVisibleText(page, "First-run setup", viewport.name);
      await expectVisibleText(page, "curb init --config /tmp/curb/config.yaml", viewport.name);
      await expectVisibleText(page, "Diagnostics", viewport.name);
      await expectVisibleText(page, "Optional", viewport.name);
      await page.getByText("repo", { exact: false }).first().click();
      await expectVisibleText(page, "This turn", viewport.name);
      await expectVisibleText(page, "Model calls", viewport.name);
      await expectVisibleText(page, "Stop requires", viewport.name);
      await expectVisibleText(page, "PID", viewport.name);
      await expectVisibleText(page, "start time", viewport.name);
      await expectVisibleText(page, "owner", viewport.name);
      await expectVisibleText(page, "executable", viewport.name);
      await expectVisibleText(page, "Stop now", viewport.name);
      await page.getByText("Stop now", { exact: false }).first().click();
      // The confirmation is asked in the modal dialog costume.
      await expectVisibleText(page, "Confirm stop", viewport.name);
      await expectVisibleText(page, "Cancel", viewport.name);
      await assertNoViewportOverflow(page, "dialog.ae-dialog", viewport.name);
      await page.getByText("Cancel", { exact: false }).first().click();
      await assertNoViewportOverflow(page, ".topbar", viewport.name);
      await assertNoViewportOverflow(page, ".agents", viewport.name);
      await assertNoViewportOverflow(page, ".action-strip", viewport.name);
      await assertNoViewportOverflow(page, ".stop-checks", viewport.name);
      await assertNoViewportOverflow(page, ".row-actions", viewport.name);
      await assertNoViewportOverflow(page, ".recovery", viewport.name);
      await assertNoViewportOverflow(page, ".readiness", viewport.name);
      await assertNoViewportOverflow(page, ".drawer", viewport.name);
    } catch (error) {
      failures.push(`${viewport.name}: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      await page.screenshot({ path: screenshot, fullPage: true });
      screenshots.push(screenshot);
      await page.close();
    }
  }
} finally {
  await browser.close();
  if (server) await server.close();
}

const manifest = {
  baseURL,
  mode: externalURL ? "external" : "deterministic-vite",
  viewports,
  screenshots,
  failures,
};
await writeFile(path.join(artifactDir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
if (failures.length > 0) {
  await writeFile(path.join(artifactDir, "failures.json"), `${JSON.stringify(failures, null, 2)}\n`);
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.error(failure);
  }
  process.exit(1);
}
console.log(`dashboard smoke ok: ${artifactDir}`);

async function startVite() {
  server = await createServer({
    root: process.cwd(),
    logLevel: "error",
    server: { host: "127.0.0.1", port: 0 },
  });
  await server.listen();
  const url = server.resolvedUrls?.local?.[0];
  if (!url) throw new Error("Vite did not report a local URL");
  return { url };
}

async function loadRoutes() {
  const snapshot = stoppableSnapshot(await readJson("../../contracts/api/snapshot.json"));
  const session = stoppableSession(await readJson("../../contracts/api/session.json"));
  return {
    "/v1/snapshot": snapshot,
    "/v1/config": await readJson("../../contracts/api/config.json"),
    "/v1/notifications/health": notificationHealth(),
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

function notificationHealth() {
  return {
    enabled: true,
    available: true,
    status: "ready",
    message: "Smoke notifications are ready.",
  };
}

function stoppableSnapshot(snapshot) {
  return {
    ...snapshot,
    sessions: snapshot.sessions.map((session, index) =>
      index === 0 ? stoppableSession(session) : session,
    ),
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
  if (externalURL) return;
  await page.route("**/v1/**", async (route) => {
    const url = new URL(route.request().url());
    const payload = await payloadFor(url.pathname);
    if (payload === undefined) {
      failures.push(`api: unhandled route ${url.pathname}`);
      await route.fulfill({
        status: 404,
        contentType: "application/json",
        body: JSON.stringify({ error: "unhandled smoke route" }),
      });
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(payload),
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
  if (/^\/v1\/sessions\/[^/]+\/ack$/.test(pathname)) {
    return {
      session_key: decodeURIComponent(pathname.split("/")[3]),
      extend_seconds: 1800,
      until: "2026-05-28T16:30:00Z",
    };
  }
  return routes[pathname];
}

async function expectVisibleText(page, text, viewportName) {
  const locator = page.getByText(text, { exact: false }).first();
  try {
    await locator.waitFor({ state: "visible", timeout: 5000 });
  } catch {
    failures.push(`${viewportName}: missing visible text ${JSON.stringify(text)}`);
  }
}

async function assertNoViewportOverflow(page, selector, viewportName) {
  const overflow = await page.locator(selector).evaluate((node) => {
    const rect = node.getBoundingClientRect();
    return {
      left: rect.left,
      right: rect.right,
      width: window.innerWidth,
      overflow: rect.left < -1 || rect.right > window.innerWidth + 1,
    };
  });
  if (overflow.overflow) {
    failures.push(
      `${viewportName}: ${selector} overflows viewport (${overflow.left.toFixed(1)}..${overflow.right.toFixed(1)} / ${overflow.width})`,
    );
  }
}
