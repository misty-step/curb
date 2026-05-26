import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";

const baseURL = process.env.CURB_SMOKE_URL || "http://127.0.0.1:8765/";
const viewports = [
  { name: "desktop", width: 1440, height: 900 },
  { name: "narrow", width: 390, height: 844 },
];

const browser = await chromium.launch();
const failures = [];
await mkdir("artifacts", { recursive: true });

try {
  for (const viewport of viewports) {
    const page = await browser.newPage({ viewport });
    await page.goto(baseURL, { waitUntil: "networkidle" });
    await expectVisibleText(page, "Curb", viewport.name);
    await expectVisibleText(page, "Right now", viewport.name);
    await expectVisibleText(page, "Active runs", viewport.name);
    await expectVisibleText(page, "Alive workers", viewport.name);
    await expectVisibleText(page, "Unmatched logs", viewport.name);
    await expectVisibleText(page, "Policy", viewport.name);
    await assertNoViewportOverflow(page, ".operator-summary", viewport.name);
    await assertNoViewportOverflow(page, ".topbar", viewport.name);
    await page.screenshot({ path: `artifacts/dashboard-${viewport.name}.png`, fullPage: true });
    await page.close();
  }
} finally {
  await browser.close();
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.error(failure);
  }
  process.exit(1);
}

async function expectVisibleText(page, text, viewportName) {
  const locator = page.getByText(text, { exact: false }).first();
  if (!(await locator.isVisible().catch(() => false))) {
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
