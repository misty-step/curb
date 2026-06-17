// @vitest-environment jsdom

import React from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { demoConfig, demoSnapshot } from "./demo";
import type { ReadinessView, Snapshot } from "./types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | undefined;

beforeEach(() => {
  vi.resetModules();
  document.body.innerHTML = '<div id="root"></div>';
});

afterEach(() => {
  root?.unmount();
  root = undefined;
  vi.restoreAllMocks();
});

describe("source health", () => {
  it("collapses a source problem to one plain line — no operator console or leakage", async () => {
    installRecoveryFetch();
    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const page = document.body.textContent ?? "";
    // The operator console and its CLI/runbook plumbing are not on a user's dashboard.
    expect(page).not.toContain("RECOVERY");
    expect(page).not.toContain("Process correlation");
    expect(page).not.toContain("Watcher runtime");
    expect(page).not.toContain("curb usage --since 24h");
    expect(page).not.toContain("RUNBOOK");
    // Raw provider paths and payloads never reach the UI.
    expect(page).not.toContain("/Users/phaedrus/.codex/private");
    expect(page).not.toContain("prompt payload");
    expect(page).not.toContain("Failed to fetch");
    // The single user-facing fact — Curb may be missing spend — survives in plain
    // words, above the fold, with the provider name resolved (not a raw id).
    expect(page).toContain("Codex");
    expect(page).toContain("read all of your");
    expect(page).toContain("may miss spend");
  });

  it("shows a plain connection banner when the local API is unreachable", async () => {
    installConnectionFailureFetch();
    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const page = document.body.textContent ?? "";
    expect(page).toContain("Live data unavailable");
    expect(page).toContain("Curb's local service isn't responding");
    expect(page).not.toContain("RECOVERY");
    expect(page).not.toContain("Failed to fetch");
  });
});

function installRecoveryFetch() {
  const snapshot = recoverySnapshot();
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes("/v1/snapshot") || url.includes("/v1/service/rescan")) return jsonResponse(snapshot);
      if (url.includes("/v1/config")) return jsonResponse(demoConfig);
      if (url.includes("/v1/ready")) return jsonResponse(readinessFixture());
      if (url.includes("/v1/onboarding")) return jsonResponse(onboardingFixture(snapshot));
      if (url.includes("/v1/notifications")) {
        return jsonResponse({ enabled: true, available: true, status: "ready", message: "ready" });
      }
      if (url.includes("/v1/sessions/")) return jsonResponse(snapshot.sessions[0]);
      return new Response("not found", { status: 404 });
    }),
  );
}

function installConnectionFailureFetch() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes("/v1/ready")) return jsonResponse(readinessFixture());
      if (url.includes("/v1/snapshot")) throw new TypeError("Failed to fetch");
      return new Response("not reached", { status: 500 });
    }),
  );
}

function recoverySnapshot(): Snapshot {
  return {
    ...demoSnapshot,
    overview: {
      ...demoSnapshot.overview,
      sources: [
        {
          provider: "codex",
          files: 1,
          events: 0,
          error: "provider usage metadata failed a metadata-only read",
        },
      ],
      recovery: [
        {
          id: "source-codex",
          label: "codex source",
          status: "error",
          message:
            "codex usage metadata could not be read: provider usage metadata failed a metadata-only read. Raw provider paths and payloads are not shown in recovery.",
          action: "Run `curb usage --since 24h`.",
          command: "curb usage --since 24h",
          runbook: "docs/user-guide.md#recovery-surface",
        },
      ],
    },
    sessions: [
      {
        ...demoSnapshot.sessions[0],
        alert: "kill",
        can_stop: false,
        pid: undefined,
        process_started_at: undefined,
        explanation: "Over your kill line, but no live process matched to stop.",
      },
    ],
  };
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
        id: "process-correlation",
        label: "Process correlation",
        status: "uncorrelated",
        message: "One or more over-limit sessions do not have a sealed live worker identity, so stop is disabled.",
        action: "Run `curb scan --json --config /tmp/curb/config.yaml` and inspect /tmp/curb/config.yaml.",
        command: "curb scan --json --config /tmp/curb/config.yaml",
        path: "/tmp/curb/config.yaml",
        runbook: "docs/user-guide.md#recovery-surface",
      },
    ],
  };
}

function readinessFixture(): ReadinessView {
  return {
    status: "degraded",
    app: "curb",
    api_version: 1,
    checks: [{ name: "watcher_runtime", status: "error", reason: "cache busy" }],
    recovery: [
      {
        id: "readiness-watcher_runtime",
        label: "Watcher runtime",
        status: "error",
        message: "The daemon snapshot cache is not ready: cache busy",
        action: "Run `curb watch --once` and inspect /tmp/curb/state.",
        command: "curb watch --once",
        path: "/tmp/curb/state",
        runbook: "docs/user-guide.md#recovery-surface",
      },
    ],
  };
}

async function actRender(element: React.ReactElement) {
  const { act } = await import("react");
  await act(async () => {
    root!.render(element);
  });
  for (let i = 0; i < 6; i++) {
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }
}

function jsonResponse(value: unknown): Response {
  return new Response(JSON.stringify(value), { status: 200, headers: { "Content-Type": "application/json" } });
}
