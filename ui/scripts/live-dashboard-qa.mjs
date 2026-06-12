import { chromium } from "playwright";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import path from "node:path";

const baseURL = requiredEnv("CURB_LIVE_QA_URL");
const artifactDir = requiredEnv("CURB_LIVE_QA_OUT");
const homeDir = requiredEnv("CURB_LIVE_QA_HOME");
const workRoot = requiredEnv("CURB_LIVE_QA_WORK_ROOT");
const workerExe = requiredEnv("CURB_LIVE_QA_WORKER");
const workerMarker = requiredEnv("CURB_LIVE_QA_MARKER");
const configPath = requiredEnv("CURB_LIVE_QA_CONFIG");

const viewports = [
  { name: "desktop", width: 1440, height: 900 },
  { name: "narrow", width: 390, height: 844 },
];
const failures = [];
const actions = [];
const consoleErrors = [];
const screenshots = [];
const workers = [];

await mkdir(artifactDir, { recursive: true });
const browser = await chromium.launch();

try {
  await runDesktopFlow();
  await runNarrowCapture();
} finally {
  await browser.close();
  for (const worker of workers) {
    if (!worker.killed) worker.kill("SIGTERM");
  }
}

for (const error of consoleErrors) {
  if (!expectedConsoleError(error.text)) {
    failures.push(`${error.viewport}: unexpected console error ${JSON.stringify(error.text)}`);
  }
}

const manifest = {
  baseURL,
  mode: "live-curb-serve",
  configPath,
  viewports,
  screenshots,
  actions,
  consoleErrors,
  failures,
};
await writeFile(path.join(artifactDir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
if (consoleErrors.length > 0) {
  await writeFile(path.join(artifactDir, "console-errors.json"), `${JSON.stringify(consoleErrors, null, 2)}\n`);
}
if (failures.length > 0) {
  await writeFile(path.join(artifactDir, "failures.json"), `${JSON.stringify(failures, null, 2)}\n`);
  for (const failure of failures) console.error(failure);
  process.exit(1);
}

console.log(`live dashboard QA ok: ${artifactDir}`);

async function runDesktopFlow() {
  const page = await newPage(viewports[0]);
  try {
    await page.goto(baseURL, { waitUntil: "networkidle" });
    await expectVisibleText(page, "Curb", "desktop");
    await expectVisibleText(page, "Recovery", "desktop");
    await expectVisibleText(page, "First-run setup", "desktop");
    await expectVisibleText(page, "ack", "desktop");
    await expectVisibleText(page, "over kill", "desktop");
    await selectSession(page, "ack", "desktop");
    await clickButton(page, "Acknowledge", "desktop");
    await expectVisibleText(page, "Acknowledged until", "desktop");
    actions.push("acknowledged uncorrelated live-ack session");

    await openSettings(page, "desktop");
    await toggleNotify(page);
    await expectVisibleText(page, "Saved.", "desktop");
    await toggleNotify(page);
    await expectVisibleText(page, "Saved.", "desktop");
    await clickButton(page, "Test", "desktop");
    actions.push("saved and reverted notification setting, then exercised notification test");

    const stopWorker = await createStopSession();
    await page.getByLabel("Rescan now").click();
    await expectVisibleText(page, "stop", "desktop");
    await selectSession(page, "stop", "desktop");
    await expectVisibleText(page, "Stop requires", "desktop");
    await expectVisibleText(page, "PID", "desktop");
    await expectVisibleText(page, "start time", "desktop");
    await expectVisibleText(page, "owner", "desktop");
    await expectVisibleText(page, "executable", "desktop");

    await safeStopRejection(page);
    await clickButton(page, "Stop now", "desktop");
    await expectVisibleText(page, "Confirm stop", "desktop");
    await clickButton(page, "Confirm stop", "desktop");
    await waitForWorkerExit(stopWorker);
    actions.push("confirmed browser stop terminated synthetic live-stop worker");

    await page.route("**/v1/snapshot", (route) => route.abort("failed"));
    await page.getByLabel("Rescan now").click();
    await expectVisibleText(page, "API connection", "desktop");
    await expectVisibleText(page, "api.token", "desktop");
    await assertNoText(page, "Failed to fetch", "desktop");
    actions.push("verified live API failure renders recovery copy without raw Failed to fetch");

    await screenshot(page, "dashboard-desktop.png");
    await assertNoViewportOverflow(page, ".topbar", "desktop");
    await assertNoViewportOverflow(page, ".agents", "desktop");
    await assertNoViewportOverflow(page, ".recovery", "desktop");
    await assertNoViewportOverflow(page, ".readiness", "desktop");
  } catch (error) {
    failures.push(`desktop: ${error instanceof Error ? error.message : String(error)}`);
    await screenshot(page, "dashboard-desktop-failure.png");
  } finally {
    await page.close();
  }
}

async function runNarrowCapture() {
  const page = await newPage(viewports[1]);
  try {
    await page.goto(baseURL, { waitUntil: "networkidle" });
    await expectVisibleText(page, "Curb", "narrow");
    await expectVisibleText(page, "Recovery", "narrow");
    await expectVisibleText(page, "Limits & mode", "narrow");
    await screenshot(page, "dashboard-narrow.png");
    await assertNoViewportOverflow(page, ".topbar", "narrow");
    await assertNoViewportOverflow(page, ".agents", "narrow");
    await assertNoViewportOverflow(page, ".recovery", "narrow");
    await assertNoViewportOverflow(page, ".readiness", "narrow");
  } catch (error) {
    failures.push(`narrow: ${error instanceof Error ? error.message : String(error)}`);
    await screenshot(page, "dashboard-narrow-failure.png");
  } finally {
    await page.close();
  }
}

async function newPage(viewport) {
  const page = await browser.newPage({ viewport });
  page.on("console", (message) => {
    if (message.type() === "error") {
      consoleErrors.push({ viewport: viewport.name, text: message.text() });
    }
  });
  page.on("pageerror", (error) => {
    consoleErrors.push({ viewport: viewport.name, text: error.message });
  });
  return page;
}

async function toggleNotify(page) {
  const saved = page.waitForResponse((response) => {
    return response.url().endsWith("/v1/config") && response.request().method() === "PUT" && response.status() === 200;
  });
  await page.locator("#notify").click();
  await saved;
}

async function createStopSession() {
  const stopDir = path.join(workRoot, "stop");
  const worker = spawn(workerExe, ["-c", "import time; time.sleep(120)", workerMarker], {
    cwd: stopDir,
    stdio: "ignore",
  });
  workers.push(worker);
  const usageDir = path.join(homeDir, ".codex", "archived_sessions");
  const now = new Date().toISOString();
  const rows = [
    { timestamp: now, type: "session_meta", payload: { id: "live-stop", cwd: stopDir } },
    {
      timestamp: now,
      type: "event_msg",
      payload: {
        type: "token_count",
        info: {
          last_token_usage: {
            input_tokens: 260,
            cached_input_tokens: 0,
            output_tokens: 20,
            reasoning_output_tokens: 0,
            total_tokens: 280,
          },
          total_token_usage: { total_tokens: 280 },
          model_context_window: 258400,
        },
      },
    },
  ];
  await writeFile(
    path.join(usageDir, "live-stop.jsonl"),
    `${rows.map((row) => JSON.stringify(row)).join("\n")}\n`,
  );
  await writeFile(
    path.join(artifactDir, "worker-start.json"),
    `${JSON.stringify({ pid: worker.pid, executable: workerExe, cwd: stopDir, marker: workerMarker }, null, 2)}\n`,
  );
  actions.push(`started synthetic worker pid=${worker.pid}`);
  return worker;
}

async function safeStopRejection(page) {
  const response = await page.evaluate(async () => {
    const result = await fetch("/v1/sessions/codex%3Alive-stop/stop", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        confirm: true,
        scope: "tree",
        reason: "live dashboard QA stale stop rejection",
        expected: {
          pid: 1,
          started_at: "1970-01-01T00:00:00Z",
          owner: "stale",
          executable: "/tmp/stale",
        },
      }),
    });
    return { status: result.status, body: await result.text() };
  });
  await writeFile(path.join(artifactDir, "safe-stop-rejection.json"), `${JSON.stringify(response, null, 2)}\n`);
  if (response.status < 400) {
    throw new Error(`safe stop rejection returned ${response.status}`);
  }
  actions.push(`safe stop rejection returned HTTP ${response.status}`);
}

async function waitForWorkerExit(worker) {
  const exited = await new Promise((resolve) => {
    const deadline = Date.now() + 8000;
    const check = () => {
      if (!processExists(worker.pid)) {
        resolve(true);
        return;
      }
      if (Date.now() >= deadline) {
        resolve(false);
        return;
      }
      setTimeout(check, 100);
    };
    check();
  });
  await writeFile(
    path.join(artifactDir, "worker-exit.json"),
    `${JSON.stringify({ pid: worker.pid, exited }, null, 2)}\n`,
  );
  if (!exited) {
    throw new Error(`worker pid ${worker.pid} did not exit after confirmed stop`);
  }
}

async function selectSession(page, text, viewportName) {
  const row = page.locator("button.row-head").filter({ hasText: text }).first();
  await row.waitFor({ state: "visible", timeout: 10_000 });
  await row.click();
  actions.push(`${viewportName}: selected ${text}`);
}

async function openSettings(page, viewportName) {
  const drawer = page.locator(".drawer").first();
  await drawer.locator("summary").click();
  await expectVisibleText(page, "Notify me", viewportName);
  actions.push(`${viewportName}: opened settings`);
}

async function clickButton(page, text, viewportName) {
  const locator = page.getByRole("button", { name: text, exact: false }).first();
  await locator.waitFor({ state: "visible", timeout: 10_000 });
  await locator.click();
  actions.push(`${viewportName}: clicked ${text}`);
}

async function expectVisibleText(page, text, viewportName) {
  try {
    await page.getByText(text, { exact: false }).first().waitFor({ state: "visible", timeout: 10_000 });
  } catch (error) {
    const message = `${viewportName}: missing visible text ${JSON.stringify(text)}`;
    failures.push(message);
    throw error instanceof Error ? new Error(`${message}: ${error.message}`) : new Error(message);
  }
}

async function assertNoText(page, text, viewportName) {
  const count = await page.getByText(text, { exact: false }).count();
  if (count > 0) failures.push(`${viewportName}: unexpected text ${JSON.stringify(text)}`);
}

async function screenshot(page, name) {
  const target = path.join(artifactDir, name);
  await page.screenshot({ path: target, fullPage: true });
  screenshots.push(name);
}

async function assertNoViewportOverflow(page, selector, viewportName) {
  const count = await page.locator(selector).count();
  if (count === 0) return;
  const overflow = await page.locator(selector).first().evaluate((node) => {
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

function requiredEnv(name) {
  const value = process.env[name];
  if (!value) throw new Error(`missing ${name}`);
  return value;
}

function expectedConsoleError(text) {
  return (
    text.includes("the server responded with a status of 503") ||
    text.includes("the server responded with a status of 409") ||
    text.includes("the server responded with a status of 404") ||
    text.includes("net::ERR_FAILED")
  );
}

function processExists(pid) {
  if (!pid) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error && error.code === "EPERM";
  }
}
