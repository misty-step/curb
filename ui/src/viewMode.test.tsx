// @vitest-environment jsdom

import React from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { demoConfig, demoSnapshot } from "./demo";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | undefined;

beforeEach(() => {
  vi.resetModules();
  vi.stubGlobal("localStorage", memoryStorage());
  localStorage.clear();
  document.body.innerHTML = '<div id="root"></div>';
  installFetch();
});

afterEach(() => {
  root?.unmount();
  root = undefined;
  vi.restoreAllMocks();
});

describe("view mode", () => {
  it("is a compact table by default and expands to the full dashboard", async () => {
    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    // Compact default: the agent table is the whole surface. The headline lede
    // and the Limits & mode drawer are tucked away — the menu-bar app carries
    // the at-a-glance headline.
    const compact = document.body.textContent ?? "";
    expect(compact).toContain("gradient");
    expect(compact).toContain("over warn");
    expect(compact).not.toContain("1 over the kill line");
    expect(compact).not.toContain("Limits & mode");

    const expand = document.querySelector('button[aria-label="Expand for limits and detail"]');
    expect(expand).toBeTruthy();
    await actRender(null, () => (expand as HTMLButtonElement).click());

    // Expanded reveals the headline lede, the policy summary, and the drawer.
    const expanded = document.body.textContent ?? "";
    expect(expanded).toContain("1 over the kill line");
    expect(expanded).toContain("Limits & mode");
    expect(expanded).toContain("Warn at 1,000,000 · stop disabled");
    expect(localStorage.getItem("curb-view")).toBe("expanded");
  });

  it("reopens in the persisted expanded view", async () => {
    localStorage.setItem("curb-view", "expanded");
    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);
    expect(document.body.textContent ?? "").toContain("Limits & mode");
  });
});

function installFetch() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes("/v1/snapshot") || url.includes("/v1/service/rescan")) return json(demoSnapshot);
      if (url.includes("/v1/config")) return json(demoConfig);
      if (url.includes("/v1/ready")) return json({ status: "ready", app: "curb", api_version: 1, checks: [], recovery: [] });
      if (url.includes("/v1/notifications")) return json({ enabled: true, available: true, status: "ready", message: "ready" });
      if (url.includes("/v1/onboarding")) return json({ required: false, recovery: [] });
      return new Response("not found", { status: 404 });
    }),
  );
}

function json(value: unknown): Response {
  return new Response(JSON.stringify(value), { status: 200, headers: { "Content-Type": "application/json" } });
}

async function actRender(element: React.ReactElement | null, action?: () => void) {
  const { act } = await import("react");
  await act(async () => {
    action?.();
    if (element) root!.render(element);
  });
  // Let async fetch chains and the state updates they trigger settle.
  for (let i = 0; i < 6; i++) {
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }
}

function memoryStorage(): Storage {
  const values = new Map<string, string>();
  return {
    get length() {
      return values.size;
    },
    clear() {
      values.clear();
    },
    getItem(key: string) {
      return values.get(key) ?? null;
    },
    key(index: number) {
      return Array.from(values.keys())[index] ?? null;
    },
    removeItem(key: string) {
      values.delete(key);
    },
    setItem(key: string, value: string) {
      values.set(key, value);
    },
  };
}
