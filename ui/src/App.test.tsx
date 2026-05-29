// @vitest-environment jsdom

import React from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { demoConfig, demoSnapshot } from "./demo";
import type { Snapshot } from "./types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

interface RequestRecord {
  url: string;
  method: string;
  body: string;
}

let root: Root | undefined;

beforeEach(() => {
  vi.resetModules();
  vi.stubGlobal("localStorage", memoryStorage());
  localStorage.clear();
  document.body.innerHTML = '<div id="root"></div>';
});

afterEach(() => {
  root?.unmount();
  root = undefined;
  vi.restoreAllMocks();
});

describe("Curb dashboard", () => {
  it("leads with the status headline and one row per working agent, no jargon", async () => {
    installFetch(demoSnapshot);
    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const page = document.body.textContent ?? "";
    expect(page).toContain("1 agent past your warn line");
    expect(page).toContain("gradient");
    expect(page).toContain("1.4M");
    expect(page).toContain("over warn");
    expect(page).toContain("Limits & mode");
    // Idle agents fold into a count rather than cluttering the list.
    expect(page).toContain("idle agent");
    // The old vocabulary is gone.
    expect(page).not.toContain("checkpoint");
    expect(page).not.toContain("window spend");
    expect(page).not.toContain("Unmatched logs");
  });

  it("acknowledges a warning session through the ack endpoint", async () => {
    const requests = installFetch(demoSnapshot);
    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const head = Array.from(document.querySelectorAll("button.row-head")).find((button) =>
      button.textContent?.includes("gradient"),
    );
    expect(head).toBeTruthy();
    await actRender(null, () => (head as HTMLButtonElement).click());

    const ack = Array.from(document.querySelectorAll("button")).find((button) => button.textContent === "Acknowledge");
    expect(ack).toBeTruthy();
    await actRender(null, () => (ack as HTMLButtonElement).click());

    expect(requests.some((request) => request.url.includes("/ack") && request.method === "POST")).toBe(true);
  });
});

function installFetch(snapshot: Snapshot): RequestRecord[] {
  const requests: RequestRecord[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      requests.push({ url, method: init?.method ?? "GET", body: String(init?.body ?? "") });
      if (url.includes("/v1/snapshot") || url.includes("/v1/service/rescan")) return jsonResponse(snapshot);
      if (url.includes("/v1/config")) return jsonResponse(demoConfig);
      if (url.includes("/v1/notifications")) {
        return jsonResponse({ enabled: true, available: true, status: "ready", message: "ready" });
      }
      if (url.includes("/ack")) {
        return jsonResponse({ session_key: snapshot.sessions[0].key, extend_seconds: 1800, until: "2026-05-29T18:00:00Z" });
      }
      if (url.includes("/stop")) {
        return jsonResponse({ session_key: snapshot.sessions[0].key, pid: 4242, scope_pids: [4242], result: {} });
      }
      return new Response("not found", { status: 404 });
    }),
  );
  return requests;
}

async function actRender(element: React.ReactElement | null, action?: () => void) {
  const { act } = await import("react");
  await act(async () => {
    action?.();
    if (element) root!.render(element);
  });
  // Let async fetch chains and the state updates they trigger settle.
  for (let i = 0; i < 3; i++) {
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }
}

function jsonResponse(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), { status, headers: { "Content-Type": "application/json" } });
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
