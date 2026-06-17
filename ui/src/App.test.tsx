// @vitest-environment jsdom

import React from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { connectionMessage } from "./components/dashboard";
import { demoConfig, demoSnapshot } from "./demo";
import type { ReadinessView, Snapshot } from "./types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

interface RequestRecord {
  url: string;
  method: string;
  body: string;
}

interface FetchRoute {
  match: (url: string) => boolean;
  response: () => Response;
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
  it("turns dev-server HTML fetch failures into operator copy", () => {
    expect(connectionMessage('Unexpected token \'<\', "<!doctype "... is not valid JSON')).toBe(
      "The dashboard reached the dev server instead of the Curb API. Run curb app for live data.",
    );
  });

  it("leads with the status headline and one row per working agent, no jargon", async () => {
    installFetch(demoSnapshot);
    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const page = document.body.textContent ?? "";
    expect(page).toContain("1 over the kill line");
    expect(page).toContain("gradient");
    expect(page).toContain("1.4M");
    expect(page).toContain("over warn");
    expect(page).toContain("over kill");
    expect(page).toContain("Limits & mode");
    expect(page).toContain("Warn at 1,000,000 · stop disabled");
    // Idle agents fold into a count rather than cluttering the list.
    expect(page).toContain("idle agent");
    // The dashboard is the agent list. Operator consoles (recovery, setup,
    // diagnostics) and their CLI/runbook plumbing are stripped from the UI.
    expect(page).not.toContain("RECOVERY");
    expect(page).not.toContain("First-run setup");
    expect(page).not.toContain("Diagnostics");
    expect(page).not.toContain("RUNBOOK");
    expect(page).not.toContain("curb usage --since 24h");
    expect(page).not.toContain("curb init");
    // Redundant and confusing copy is gone.
    expect(page).not.toContain("all within limits");
    expect(page).not.toContain("checkpoint");
    expect(page).not.toContain("window spend");
    // The Advanced/token field was removed.
    expect(page).not.toContain("api.token");
  });

  it("shows an informative empty state — armed limits — when nothing is spending", async () => {
    const idleOnly: Snapshot = {
      ...demoSnapshot,
      overview: { ...demoSnapshot.overview, status: "OK", message: "Nothing spending" },
      sessions: demoSnapshot.sessions.filter((session) => session.id === "daybook"),
    };
    installFetch(idleOnly);
    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const page = document.body.textContent ?? "";
    expect(page).toContain("Nothing spending right now");
    expect(page).toContain("Watching Codex and Claude Code");
    expect(page).toContain("1,000,000"); // the warn limit, armed
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

  it("reflects mode changes immediately while config save is still pending", async () => {
    const pendingSave = deferred<Response>();
    installFetch(demoSnapshot, pendingSave.promise);
    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const drawer = document.querySelector(".drawer") as HTMLDetailsElement;
    await actRender(null, () => {
      drawer.open = true;
    });
    const stop = Array.from(document.querySelectorAll("label.ae-choice"))
      .find((label) => label.textContent?.includes("Stop runaways"))
      ?.querySelector("input");
    expect(stop).toBeTruthy();
    await actRender(null, () => (stop as HTMLInputElement).click());

    expect((stop as HTMLInputElement).checked).toBe(true);
    expect(document.body.textContent).toContain("Warn at 1,000,000 · stop at 3,000,000");

    pendingSave.resolve(jsonResponse({ ...demoConfig, mode: "enforcement" }));
    await actRender(null);
    await actRender(null);
  });

  it("opens a selected-session cockpit with turn timeline and evidence", async () => {
    const requests = installFetch(demoSnapshot);
    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const head = Array.from(document.querySelectorAll("button.row-head")).find((button) =>
      button.textContent?.includes("olympus"),
    );
    expect(head).toBeTruthy();
    await actRender(null, () => (head as HTMLButtonElement).click());

    const page = document.body.textContent ?? "";
    // The session cockpit is the detail surface; SETUP/Diagnostics consoles are gone.
    expect(page).not.toContain("SETUP");
    expect(page).not.toContain("Diagnostics");
    expect(page).toContain("TURN TIMELINE");
    expect(page).toContain("gpt-5.5");
    expect(page).toContain("Input 1.2M");
    expect(page).toContain("Cached 180k");
    expect(page).toContain("Reasoning 90k");
    expect(page).toContain("Source codex usage log");
    expect(page).toContain("PID 7731");
    expect(page).toContain("Start-time seal");
    expect(page).toContain("Executable /Applications/Codex.app/Contents/Resources/codex");
    expect(page).toContain("Stop Unavailable");
    expect(page).toContain("watch-only");
    expect(requests.some((request) => request.url.includes("/v1/onboarding"))).toBe(true);
    expect(requests.some((request) => request.url.includes("/v1/ready"))).toBe(true);
    expect(requests.some((request) => request.url.includes("/v1/sessions/codex%3Aolympus"))).toBe(true);
    expect(requests.some((request) => request.url.includes("/v1/sessions/codex%3Aolympus/turns"))).toBe(true);
  });

  it("shows the identity checklist beside a destructive stop action", async () => {
    const requests = installFetch(stoppableSnapshot());
    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const head = Array.from(document.querySelectorAll("button.row-head")).find((button) =>
      button.textContent?.includes("olympus"),
    );
    expect(head).toBeTruthy();
    await actRender(null, () => (head as HTMLButtonElement).click());

    const page = document.body.textContent ?? "";
    expect(page).toContain("Stop requires");
    expect(page).toContain("PID");
    expect(page).toContain("start time");
    expect(page).toContain("owner");
    expect(page).toContain("executable");
    expect(page).toContain("Stop now");

    const stop = Array.from(document.querySelectorAll("button")).find((button) => button.textContent?.includes("Stop now"));
    expect(stop).toBeTruthy();
    await actRender(null, () => (stop as HTMLButtonElement).click());
    expect(requests.some((request) => request.url.includes("/stop"))).toBe(false);
    expect(document.body.textContent).toContain("Confirm stop");
  });

  it("posts the stop request only after inline confirmation", async () => {
    const snapshot = stoppableSnapshot();
    const expectedSession = snapshot.sessions[0];
    const requests = installFetch(snapshot);
    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const head = Array.from(document.querySelectorAll("button.row-head")).find((button) =>
      button.textContent?.includes("olympus"),
    );
    await actRender(null, () => (head as HTMLButtonElement).click());
    const stop = Array.from(document.querySelectorAll("button")).find((button) => button.textContent?.includes("Stop now"));
    await actRender(null, () => (stop as HTMLButtonElement).click());
    const confirm = Array.from(document.querySelectorAll("button")).find((button) =>
      button.textContent?.includes("Confirm stop"),
    );
    expect(confirm).toBeTruthy();
    await actRender(null, () => (confirm as HTMLButtonElement).click());

    const stopRequest = requests.find((request) => request.url.includes("/stop"));
    expect(stopRequest?.method).toBe("POST");
    expect(JSON.parse(stopRequest?.body ?? "{}")).toMatchObject({
      confirm: true,
      scope: "tree",
      expected: {
        pid: expectedSession.pid,
        owner: expectedSession.owner,
        executable: expectedSession.executable,
      },
    });
  });
});

function stoppableSnapshot(): Snapshot {
    const stoppable: Snapshot = {
      ...demoSnapshot,
      sessions: [
        {
          ...demoSnapshot.sessions[0],
          can_stop: true,
          can_acknowledge: false,
          explanation: "Over your kill line — stopping after the grace period.",
        },
      ],
    };
  return stoppable;
}

function installFetch(snapshot: Snapshot, pendingConfigSave?: Promise<Response>, readiness = readinessFixture()): RequestRecord[] {
  const requests: RequestRecord[] = [];
  const routes = fetchRoutes(snapshot, readiness);
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      requests.push({ url, method: init?.method ?? "GET", body: String(init?.body ?? "") });
      if (pendingConfigSave && url.includes("/v1/config") && init?.method === "PUT") return pendingConfigSave;
      return routes.find((route) => route.match(url))?.response() ?? new Response("not found", { status: 404 });
    }),
  );
  return requests;
}

function fetchRoutes(snapshot: Snapshot, readiness: ReadinessView): FetchRoute[] {
  return [
    { match: (url) => url.includes("/v1/snapshot") || url.includes("/v1/service/rescan"), response: () => jsonResponse(snapshot) },
    { match: (url) => url.includes("/v1/ready"), response: () => jsonResponse(readiness) },
    { match: (url) => url.includes("/v1/config"), response: () => jsonResponse(demoConfig) },
    { match: (url) => url.includes("/v1/onboarding"), response: () => jsonResponse(onboardingFixture(snapshot)) },
    { match: (url) => url.includes("/v1/notifications"), response: () => jsonResponse(notificationFixture()) },
    { match: (url) => url.includes("/ack"), response: () => jsonResponse(ackFixture(snapshot)) },
    { match: (url) => url.includes("/stop"), response: () => jsonResponse(stopFixture(snapshot)) },
    { match: (url) => url.includes("/v1/sessions/") && url.includes("/turns"), response: () => jsonResponse(turnFixtures()) },
    { match: (url) => url.includes("/v1/sessions/"), response: () => jsonResponse(snapshot.sessions[0]) },
  ];
}

function onboardingFixture(snapshot: Snapshot) {
  return {
    required: true,
    config_path: "/tmp/curb/config.yaml",
    mode: "alert",
    action: "notify only; never kill",
    mode_can_terminate: false,
    detected_providers: ["codex"],
    detected_workers: ["Codex Worker"],
    enforceable_agent_types: 1,
    watch_only_agent_types: 1,
    notifications: { enabled: true, available: true, status: "ready", message: "notifications ready" },
    capabilities: snapshot.overview.capabilities,
    sources: snapshot.overview.sources,
    final_sentence: "Curb will notify on high-token turns.",
    steps: [],
    recovery: [
      {
      id: "setup",
      label: "First-run setup",
      status: "required",
      message: "Curb is using safe defaults until setup is confirmed at /tmp/curb/config.yaml.",
      action: "Run `curb init --config /tmp/curb/config.yaml` and inspect /tmp/curb/config.yaml.",
      command: "curb init --config /tmp/curb/config.yaml",
      path: "/tmp/curb/config.yaml",
      runbook: "docs/user-guide.md#recovery-surface",
      },
    ],
  };
}

function readinessFixture(): ReadinessView {
  return {
    status: "ready",
    app: "curb",
    api_version: 1,
    checks: [],
    recovery: [],
  };
}

function notificationFixture() {
  return { enabled: true, available: true, status: "ready", message: "ready" };
}

function turnFixtures() {
  return [
    {
      id: "turn-42",
      request_id: "req-42",
      session_key: "codex:olympus",
      session_id: "olympus",
      provider: "codex",
      at: "2026-05-29T17:00:00Z",
      model: "gpt-5.5",
      input_tokens: 1_200_000,
      cached_input_tokens: 180_000,
      cache_creation_input_tokens: 25_000,
      output_tokens: 240_000,
      reasoning_output_tokens: 90_000,
      total_tokens: 1_555_000,
      spent_tokens: 1_375_000,
      cumulative_tokens: 3_300_000,
      source: "codex usage log",
    },
  ];
}

function ackFixture(snapshot: Snapshot) {
  return { session_key: snapshot.sessions[0].key, extend_seconds: 1800, until: "2026-05-29T18:00:00Z" };
}

function stopFixture(snapshot: Snapshot) {
  return { session_key: snapshot.sessions[0].key, pid: 4242, scope_pids: [4242], result: {} };
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

function jsonResponse(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), { status, headers: { "Content-Type": "application/json" } });
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
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
