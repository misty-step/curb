// @vitest-environment jsdom

import React from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { demoConfig, demoSnapshot } from "./demo";
import type { AlertView, OnboardingView, Snapshot, TurnView } from "./types";

let root: Root | undefined;

beforeEach(() => {
  vi.resetModules();
  vi.useFakeTimers();
  vi.stubGlobal("localStorage", memoryStorage());
  localStorage.clear();
  document.body.innerHTML = '<div id="root"></div>';
});

afterEach(() => {
  root?.unmount();
  root = undefined;
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("App alert feed", () => {
  it("prioritizes at-a-glance agent activity before dense tables and policy configuration", async () => {
    installFetch([]);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const headings = Array.from(document.querySelectorAll("h2")).map((heading) => heading.textContent);
    expect(headings[0]).toBe("1 run with fresh usage checkpoints");
    const page = document.body.textContent ?? "";
    expect(page).toContain("Right now");
    expect(page).toContain("Fresh runs");
    expect(page).toContain("Alive workers");
    expect(page).toContain("Fresh spend");
    expect(page).toContain("checkpoint spend");
    expect(page).toContain("Policy settings");
    expect(page).toContain("latest checkpoint");
    expect(page).toContain("620k in window");
    expect(document.querySelector(".operator-summary")).toBeTruthy();
    expect(document.querySelector(".drilldown-drawer")).toBeTruthy();
    expect(document.querySelector(".app-shell > .connect-row")).toBeNull();
    expect(page).toContain("No token paste is needed");
    expect(page).toContain("Advanced");
    expect(document.querySelector(".system-drawer input[aria-label='API base URL']")).toBeTruthy();
  });

  it("does not count recent uncorrelated usage as agents spending now", async () => {
    const snapshot: Snapshot = {
      ...demoSnapshot,
      agents: [
        ...demoSnapshot.agents.map((agent) => ({
          ...agent,
          state: "running" as const,
          activity_state: "idle" as const,
          usage_state: "quiet" as const,
          action_state: "none" as const,
          latest_session_id: undefined,
          latest_turn_tokens: undefined,
          window_tokens: undefined,
          explanation: "process is running with no correlated usage",
        })),
        {
          ...demoSnapshot.agents[0],
          pid: demoSnapshot.agents[0].pid + 1000,
          state: "running",
          activity_state: "idle",
          usage_state: "quiet",
          action_state: "none",
          latest_session_id: undefined,
          latest_turn_tokens: undefined,
          window_tokens: undefined,
          explanation: "another worker in the same project is alive but idle",
        },
      ],
      sessions: [
        {
          ...demoSnapshot.sessions[0],
          key: "codex:uncorrelated",
          process_state: "no-process",
          usage_state: "spending",
          state: "active",
          correlated_agent_id: undefined,
          correlated_pid: undefined,
          window_tokens: 220_000,
          latest_turn_tokens: 220_000,
        },
      ],
    };
    installFetch([], snapshot);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const page = document.body.textContent ?? "";
    expect(page).toContain("No fresh token usage right now");
    expect(page).toContain("220k recent tokens are uncorrelated to a live worker");
    expect(page).toContain("Unmatched logs");
    expect(page).toContain("worker processes");
    expect(page).toContain("no fresh checkpoint");
    expect(page).not.toContain("confirmed input");
    expect(page).not.toContain("1 agent with fresh token usage");
  });

  it("keeps provider usage sessions stable while showing the correlated worker", async () => {
    const now = new Date().toISOString();
    const snapshot: Snapshot = {
      ...demoSnapshot,
      agents: [
        {
          ...demoSnapshot.agents[0],
          id: "codex-cli",
          provider: "codex",
          label: "Codex CLI",
          state: "spending",
          activity_state: "spending",
          process_state: "running",
          usage_state: "spending",
          pid: 100,
          project: "spellbook",
          cwd: "/work/spellbook",
          latest_session_id: "claude-a",
          latest_turn_tokens: 77_000,
          window_tokens: 180_000,
          explanation: "correlated session has usage in current window",
        },
      ],
      sessions: [
        {
          ...demoSnapshot.sessions[0],
          key: "claude:claude-a",
          id: "claude-a",
          provider: "claude",
          project: "spellbook",
          cwd: "/work/spellbook",
          state: "active",
          activity_state: "spending",
          process_state: "running",
          usage_state: "spending",
          correlated_agent_id: "codex-cli",
          correlated_pid: 100,
          last_seen_at: now,
          last_usage_at: now,
          latest_turn_tokens: 77_000,
          window_tokens: 120_000,
        },
        {
          ...demoSnapshot.sessions[0],
          key: "claude:claude-b",
          id: "claude-b",
          provider: "claude",
          project: "spellbook",
          cwd: "/work/spellbook",
          state: "active",
          activity_state: "spending",
          process_state: "running",
          usage_state: "spending",
          correlated_agent_id: "codex-cli",
          correlated_pid: 100,
          last_seen_at: now,
          last_usage_at: now,
          latest_turn_tokens: 66_000,
          window_tokens: 60_000,
        },
      ],
    };
    installFetch([], snapshot);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const summary = document.querySelector(".operator-summary")?.textContent ?? "";
    expect(summary).toContain("2 runs with fresh usage checkpoints");
    expect(summary).toContain("Codex CLI");
    expect(summary).toContain("Fresh spend143k");
    expect(summary).not.toContain("2 agents with fresh token usage");
  });

  it("summarizes live readiness before connection plumbing", async () => {
    installFetch([]);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const readiness = document.querySelector(".readiness-strip")?.textContent ?? "";
    expect(readiness).toContain("Live API");
    expect(readiness).toContain("Notifications");
    expect(readiness).toContain("Sources");
    expect(readiness).toContain("Platform");
    expect(readiness).toContain("Identity");
    expect(readiness).toContain("Enforcement");
    expect(readiness).toContain("notify only; never kill");
    expect(readiness).toContain("usage is over a warning threshold");
  });

  it("marks demo readiness as illustrative instead of active protection", async () => {
    localStorage.setItem("curb.baseUrl", "");

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const readiness = document.querySelector(".readiness-strip")?.textContent ?? "";
    expect(readiness).toContain("Demo data");
    expect(readiness).toContain("illustrative");
    expect(readiness).toContain("demo only");
    expect(readiness).toContain("inactive in demo");
    expect(readiness).not.toContain("demo notifications are ready");
    expect(readiness).not.toContain("4 processes captured");
  });

  it("marks failed live readiness as not current", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        throw new Error("daemon offline");
      }),
    );

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const readiness = document.querySelector(".readiness-strip")?.textContent ?? "";
    expect(readiness).toContain("Connection issue");
    expect(readiness).toContain("not current");
    expect(readiness).toContain("not active");
    expect(readiness).not.toContain("demo notifications are ready");
    expect(readiness).not.toContain("4 processes captured");
  });

  it("requires explicit confirmation before saving enforcement mode", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const requests = installFetch([]);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const enforce = Array.from(document.querySelectorAll(".mode-options button")).find((button) =>
      button.textContent?.includes("Enforce"),
    ) as HTMLButtonElement | undefined;
    expect(enforce).toBeTruthy();
    await actRender(null, () => enforce!.click());

    const panel = document.querySelector(".enforcement-confirmation")?.textContent ?? "";
    expect(panel).toContain("Stop threshold 3.0M");
    expect(panel).toContain("extension 30m");
    expect(panel).toContain("live actionable worker");
    expect(panel).toContain("watch-only app root");
    expect(panel).toContain("current mode will not terminate processes");
    expect(panel).toContain("revalidated worker processes");

    const save = Array.from(document.querySelectorAll("button")).find((button) =>
      button.textContent?.includes("Save policy"),
    ) as HTMLButtonElement | undefined;
    expect(save?.disabled).toBe(true);

    const confirm = document.querySelector(".enforcement-confirmation input[type='checkbox']") as HTMLInputElement | null;
    expect(confirm).toBeTruthy();
    await actRender(null, () => confirm!.click());
    expect(save?.disabled).toBe(false);

    await actRender(null, () => save!.click());
    const update = requests.find((request) => request.method === "PUT" && request.url.endsWith("/v1/config"));
    expect(update?.body).toContain('"mode":"enforcement"');
  });

  it("clears enforcement confirmation when policy values change", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    installFetch([]);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const enforce = Array.from(document.querySelectorAll(".mode-options button")).find((button) =>
      button.textContent?.includes("Enforce"),
    ) as HTMLButtonElement | undefined;
    await actRender(null, () => enforce!.click());
    const confirm = document.querySelector(".enforcement-confirmation input[type='checkbox']") as HTMLInputElement | null;
    await actRender(null, () => confirm!.click());

    const usageTab = Array.from(document.querySelectorAll(".config-tabs button")).find((button) =>
      button.textContent === "usage",
    ) as HTMLButtonElement | undefined;
    await actRender(null, () => usageTab!.click());
    const warnInput = document.querySelector("input[aria-label='Warn turn tokens']") as HTMLInputElement | null;
    expect(warnInput).toBeTruthy();
    await actRender(null, () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(warnInput, "1200000");
      warnInput!.dispatchEvent(new Event("input", { bubbles: true }));
      warnInput!.dispatchEvent(new Event("change", { bubbles: true }));
    });

    const save = Array.from(document.querySelectorAll("button")).find((button) =>
      button.textContent?.includes("Save policy"),
    ) as HTMLButtonElement | undefined;
    expect(save?.disabled).toBe(true);
  });

  it("shows process, usage, and action state separately for a selected session", async () => {
    installFetch([]);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const selectedDetail = document.querySelector(".detail")?.textContent ?? "";
    expect(selectedDetail).toContain("running");
    expect(selectedDetail).toContain("warn");
    expect(selectedDetail).toContain("acknowledge");
  });

  it("shows session model chips and full turn token breakdown fields", async () => {
    installFetch([]);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const page = document.body.textContent ?? "";
    expect(page).toContain("gpt-5.3-codex");
    expect(page).toContain("Created");
    expect(page).toContain("Cumulative");
    expect(page).toContain("Spend Timeline");
    expect(page).toContain("warn 1.0M");
    expect(page).toContain("stop 3.0M");
    expect(page).toContain("10k");
    expect(page).toContain("3.5M");
  });

  it("focuses the matching turn table row from the timeline", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const serviceTurns: TurnView[] = [
      {
        id: "turn-a",
        session_key: demoSnapshot.sessions[0].key,
        provider: "codex",
        at: "2026-05-21T12:00:00Z",
        model: "gpt-5.4-codex",
        input_tokens: 100_000,
        cached_input_tokens: 50_000,
        output_tokens: 25_000,
        reasoning_output_tokens: 10_000,
        total_tokens: 185_000,
      },
      {
        id: "turn-b",
        session_key: demoSnapshot.sessions[0].key,
        provider: "codex",
        at: "2026-05-21T12:01:00Z",
        model: "gpt-5.4-codex",
        input_tokens: 900_000,
        output_tokens: 87_000,
        total_tokens: 987_000,
      },
    ];
    installFetch([], demoSnapshot, serviceTurns);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    expect(document.querySelector(".turn-timeline")?.textContent).toContain("input");
    expect(document.querySelector(".turn-timeline")?.textContent).toContain("reasoning");
    const warnMarker = document.querySelector(".timeline-threshold.warn") as HTMLElement | null;
    const stopMarker = document.querySelector(".timeline-threshold.stop") as HTMLElement | null;
    expect(warnMarker?.style.transform).toBe("translateX(-50%)");
    expect(stopMarker?.style.right).toBe("0px");
    expect(stopMarker?.style.left).toBe("");
    const buttons = Array.from(document.querySelectorAll(".timeline-row")) as HTMLButtonElement[];
    expect(buttons).toHaveLength(2);
    await actRender(null, () => buttons[1].click());

    const selectedRow = document.querySelector(".turn-table tbody tr.selected");
    expect(selectedRow?.textContent).toContain("987k");
    expect(document.activeElement).toBe(selectedRow);
    expect(buttons[1].getAttribute("aria-current")).toBe("true");
  });

  it("uses same-origin cookie auth without requiring a pasted token", async () => {
    const requests = installFetch([]);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const initialAPIRequests = requests.filter((request) => request.url.includes("/v1/"));
    expect(initialAPIRequests.map((request) => request.url).sort()).toEqual([
      `${window.location.origin}/v1/alerts?limit=25`,
      `${window.location.origin}/v1/config`,
      `${window.location.origin}/v1/notifications/health`,
      `${window.location.origin}/v1/onboarding`,
      `${window.location.origin}/v1/sessions/${encodeURIComponent(demoSnapshot.sessions[0].key)}/turns?limit=200`,
      `${window.location.origin}/v1/snapshot`,
    ]);
    expect(initialAPIRequests.every((request) => request.authorization === undefined)).toBe(true);
    expect(document.body.textContent).toContain("Live API");
    expect(document.body.textContent).toContain("Connected through the local dashboard cookie");
    expect(document.body.textContent).not.toContain("Using an advanced bearer-token connection");
  });

  it("fetches service-projected alerts with bearer auth and renders them", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765/");
    localStorage.setItem("curb.token", "live-token");
    const alerts: AlertView[] = [
      {
        severity: "warn",
        label: "warning",
        category: "warning",
        message: "live warning",
        at: "2026-05-21T12:00:00Z",
        seq: 7,
        agent_id: "codex-cli",
        mode: "alert",
        cwd: "/tmp/curb",
        actionable: false,
        can_acknowledge: false,
        explanation: "Usage crossed warning policy.",
      },
    ];
    const requests = installFetch(alerts);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const alertRequest = requests.find((request) => request.url.endsWith("/v1/alerts?limit=25"));
    expect(alertRequest?.authorization).toBe("Bearer live-token");
    expect(document.body.textContent).toContain("live warning");
    expect(document.body.textContent).toContain("Usage crossed warning policy.");
  });

  it("polls alerts and renders the empty state when the service returns none", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    let alerts: AlertView[] = [
      {
        severity: "watch",
        label: "would stop",
        category: "would_stop",
        message: "initial alert",
        at: "2026-05-21T12:00:00Z",
        seq: 8,
        actionable: false,
        can_acknowledge: false,
        explanation: "Alert mode.",
      },
    ];
    installFetch(() => alerts);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);
    expect(document.body.textContent).toContain("initial alert");

    alerts = [];
    await actRender(null, async () => {
      vi.advanceTimersByTime(5000);
    });
    expect(document.body.textContent).toContain("No recent warnings or stop events.");
  });

  it("acknowledges the selected usage session and refreshes", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const requests = installFetch([]);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const button = Array.from(document.querySelectorAll("button")).find((candidate) =>
      candidate.textContent?.includes("Extend"),
    ) as HTMLButtonElement | undefined;
    expect(button).toBeTruthy();
    await actRender(null, () => button!.click());

    const ackRequest = requests.find((request) => request.method === "POST" && request.url.includes("/ack"));
    expect(ackRequest?.authorization).toBe("Bearer live-token");
    expect(ackRequest?.body).toContain('"extend_seconds":1800');
    expect(document.body.textContent).toContain("Acknowledged until");
    expect(requests.filter((request) => request.url.endsWith("/v1/snapshot")).length).toBeGreaterThan(1);
  });

  it("acknowledges an alert through the service-projected session key", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const alerts: AlertView[] = [
      {
        severity: "warn",
        label: "warning",
        category: "warning",
        message: "alert can be extended",
        at: "2026-05-21T12:00:00Z",
        seq: 12,
        provider: "codex",
        session_id: "provider-session-id",
        session_key: "codex:provider-session-id",
        actionable: false,
        can_acknowledge: true,
        explanation: "Usage crossed warning policy.",
      },
    ];
    const requests = installFetch(alerts);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const button = document.querySelector(".event-feed .inline-action") as HTMLButtonElement | null;
    expect(button?.textContent).toContain("Extend");
    await actRender(null, () => button!.click());

    const ackRequest = requests.find((request) => request.method === "POST" && request.url.includes("/ack"));
    expect(ackRequest?.url).toContain(encodeURIComponent("codex:provider-session-id"));
    expect(ackRequest?.body).toContain('"extend_seconds":1800');
    expect(document.querySelector(".event-feed")?.textContent).toContain("Acknowledged until");
  });

  it("loads selected-session turn history from the service", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const serviceTurns: TurnView[] = [
      {
        session_key: demoSnapshot.sessions[0].key,
        session_id: demoSnapshot.sessions[0].id,
        provider: "codex",
        at: "2026-05-21T12:00:00Z",
        model: "gpt-5.4-codex",
        total_tokens: 987_000,
        input_tokens: 900_000,
        output_tokens: 87_000,
      },
    ];
    const requests = installFetch([], demoSnapshot, serviceTurns);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const turnRequest = requests.find((request) => request.url.includes("/turns?limit=200"));
    expect(turnRequest?.authorization).toBe("Bearer live-token");
    expect(turnRequest?.url).toContain(encodeURIComponent(demoSnapshot.sessions[0].key));
    expect(document.body.textContent).toContain("987k");
    expect(document.body.textContent).toContain("gpt-5.4-codex");
  });

  it("ignores stale selected-session turn responses", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    let resolveFirst: ((turns: TurnView[]) => void) | undefined;
    const firstTurns = new Promise<TurnView[]>((resolve) => {
      resolveFirst = resolve;
    });
    const secondTurns: TurnView[] = [
      {
        id: "second-session-turn",
        session_key: demoSnapshot.sessions[1].key,
        provider: "claude",
        at: "2026-05-21T12:02:00Z",
        model: "claude-opus-4-7",
        input_tokens: 44_100,
        output_tokens: 11_223,
        total_tokens: 55_323,
      },
    ];
    const requests = installFetch([], demoSnapshot, (url) => {
      if (url.includes(encodeURIComponent(demoSnapshot.sessions[0].key))) return firstTurns;
      return secondTurns;
    });

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const sessionRows = Array.from(document.querySelectorAll(".sessions-table tbody tr")) as HTMLTableRowElement[];
    await actRender(null, () => sessionRows[1].dispatchEvent(new MouseEvent("click", { bubbles: true })));
    await actRender(null);
    expect(requests.some((request) => request.url.includes(encodeURIComponent(demoSnapshot.sessions[1].key)))).toBe(true);
    expect(document.querySelector(".turn-table")?.textContent).toContain("55k");

    await actRender(null, () =>
      resolveFirst!([
        {
          id: "stale-first-session-turn",
          session_key: demoSnapshot.sessions[0].key,
          provider: "codex",
          at: "2026-05-21T12:00:00Z",
          model: "gpt-5.4-codex",
          total_tokens: 987_000,
        },
      ]),
    );
    await actRender(null);

    expect(document.querySelector(".turn-table")?.textContent).toContain("55k");
    expect(document.querySelector(".turn-table")?.textContent).not.toContain("987k");
  });

  it("uses manual rescan for the refresh button without changing polling reads", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const requests = installFetch([]);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const refresh = document.querySelector('button[aria-label="Refresh"]') as HTMLButtonElement | null;
    expect(refresh).toBeTruthy();
    await actRender(null, () => refresh!.click());

    const rescans = requests.filter((request) => request.url.endsWith("/v1/service/rescan"));
    expect(rescans).toHaveLength(1);
    expect(rescans[0].method).toBe("POST");
    expect(rescans[0].authorization).toBe("Bearer live-token");

    await actRender(null, async () => {
      vi.advanceTimersByTime(5000);
    });
    const pollAfterManualRefresh = requests.filter((request) => request.url.endsWith("/v1/snapshot"));
    expect(pollAfterManualRefresh.length).toBeGreaterThan(1);
    expect(requests.filter((request) => request.url.endsWith("/v1/service/rescan"))).toHaveLength(1);
  });

  it("tests notifications through the service endpoint", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const requests = installFetch([]);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const button = Array.from(document.querySelectorAll("button")).find((candidate) =>
      candidate.textContent?.includes("Test notification"),
    ) as HTMLButtonElement | undefined;
    expect(button).toBeTruthy();
    await actRender(null, () => button!.click());

    const testRequest = requests.find((request) => request.url.endsWith("/v1/notifications/test"));
    expect(testRequest?.method).toBe("POST");
    expect(testRequest?.authorization).toBe("Bearer live-token");
    expect(document.body.textContent).toContain("delivered: test notification delivered");
  });

  it("renders and completes service-projected onboarding", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const onboarding: OnboardingView = {
      required: true,
      config_path: "/tmp/curb.yaml",
      mode: "alert",
      action: "notify only; never kill",
      mode_can_terminate: false,
      detected_providers: ["codex"],
      detected_workers: ["Codex Desktop Worker"],
      enforceable_agent_types: 2,
      watch_only_agent_types: 1,
      notifications: { enabled: true, available: true, status: "ready", message: "ready" },
      capabilities: demoSnapshot.overview.capabilities,
      sources: [],
      final_sentence: "Curb will notify on high-token turns. It will not stop any process in Alert mode. Desktop app roots are watch-only.",
      steps: [
        { id: "config", label: "Config", status: "done", message: "using /tmp/curb.yaml" },
        { id: "safety", label: "Safety", status: "done", message: "desktop app roots are watch-only" },
      ],
    };
    const requests = installFetch([], demoSnapshot, demoSnapshot.turns, undefined, onboarding);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    expect(document.querySelector(".onboarding-panel")?.textContent).toContain("Start watching safely");
    expect(document.body.textContent).toContain(onboarding.final_sentence);
    expect(document.querySelector(".onboarding-panel")?.textContent).toContain("codex");
    expect(document.querySelector(".onboarding-panel")?.textContent).toContain("Codex Desktop Worker");
    expect(document.querySelector(".onboarding-panel")?.textContent).toContain("/tmp/curb.yaml");
    expect(document.querySelector(".onboarding-panel")?.textContent).toContain("Process scan");
    expect(document.querySelector(".onboarding-panel")?.textContent).toContain("Identity evidence");
    expect(document.querySelector(".onboarding-panel")?.textContent).toContain("current mode will not terminate processes");
    expect(document.querySelector(".onboarding-panel")?.textContent).toContain("Done continues in alert mode");

    const testNotification = Array.from(document.querySelectorAll(".onboarding-actions button")).find((candidate) =>
      candidate.textContent?.includes("Test notification"),
    ) as HTMLButtonElement | undefined;
    expect(testNotification).toBeTruthy();
    await actRender(null, () => testNotification!.click());
    expect(requests.find((request) => request.url.endsWith("/v1/notifications/test"))?.method).toBe("POST");
    expect(document.querySelector(".onboarding-panel")?.textContent).toContain("test notification delivered");

    const done = Array.from(document.querySelectorAll(".onboarding-actions button")).find((candidate) =>
      candidate.textContent?.includes("Done"),
    ) as HTMLButtonElement | undefined;
    expect(done).toBeTruthy();
    await actRender(null, () => done!.click());

    const complete = requests.find((request) => request.url.endsWith("/v1/onboarding/complete"));
    expect(complete?.method).toBe("POST");
    expect(complete?.authorization).toBe("Bearer live-token");
    expect(document.querySelector(".onboarding-panel")).toBeNull();
  });

  it("renders onboarding degraded states without inventing policy", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const onboarding: OnboardingView = {
      required: true,
      config_path: "",
      mode: "enforcement",
      action: "enforcement enabled",
      mode_can_terminate: false,
      detected_providers: [],
      detected_workers: [],
      enforceable_agent_types: 0,
      watch_only_agent_types: 1,
      notifications: {
        enabled: true,
        available: false,
        status: "unavailable",
        message: "notification adapter unavailable",
      },
      capabilities: {
        platform: "test",
        notifications: { available: false, status: "unavailable", message: "notification adapter unavailable" },
        process_capture: { available: false, status: "error", message: "process capture failed" },
        process_identity: { available: false, status: "error", message: "process identity unavailable" },
        enforcement: { available: false, status: "blocked", message: "process identity is not strong enough for enforcement" },
      },
      sources: [],
      final_sentence: "Curb can stop only correlated enforceable workers after policy and grace checks. Desktop app roots are watch-only.",
      steps: [{ id: "sources", label: "Sources", status: "action", message: "unable to scan current agent state" }],
    };
    installFetch([], demoSnapshot, demoSnapshot.turns, undefined, onboarding);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const panel = document.querySelector(".onboarding-panel");
    expect(panel?.textContent).toContain("none yet");
    expect(panel?.textContent).toContain("not scanned");
    expect(panel?.textContent).toContain("notification adapter unavailable");
    expect(panel?.textContent).toContain("process capture failed");
    expect(panel?.textContent).toContain("process identity is not strong enough for enforcement");
    expect(panel?.textContent).toContain("Done continues in enforcement mode with enforcement unavailable");
    expect(panel?.textContent).toContain("Current mode stops");
    expect(panel?.textContent).toContain("no");
    const testNotification = Array.from(document.querySelectorAll(".onboarding-actions button")).find((candidate) =>
      candidate.textContent?.includes("Test notification"),
    ) as HTMLButtonElement | undefined;
    expect(testNotification?.disabled).toBe(true);
  });

  it("renders the enforcement-ready onboarding consequence accurately", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const onboarding: OnboardingView = {
      ...defaultOnboarding(),
      required: true,
      mode: "enforcement",
      mode_can_terminate: true,
      capabilities: {
        ...demoSnapshot.overview.capabilities,
        enforcement: {
          available: true,
          status: "ready",
          message: "enforcement can target revalidated worker processes only",
        },
      },
    };
    installFetch([], demoSnapshot, demoSnapshot.turns, undefined, onboarding);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    expect(document.querySelector(".onboarding-panel")?.textContent).toContain(
      "Done completes setup; Curb continues with enforcement available for revalidated worker processes.",
    );
    expect(document.querySelector(".onboarding-panel")?.textContent).not.toContain("Done starts Curb");
  });

  it("renders structured disabled notification test responses", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const requests = installFetch([], demoSnapshot, demoSnapshot.turns, {
      status: 409,
      body: {
        enabled: false,
        available: false,
        status: "disabled",
        message: "local notifications are disabled in Curb policy",
      },
    });

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const button = Array.from(document.querySelectorAll("button")).find((candidate) =>
      candidate.textContent?.includes("Test notification"),
    ) as HTMLButtonElement | undefined;
    expect(button).toBeTruthy();
    await actRender(null, () => button!.click());

    const testRequest = requests.find((request) => request.url.endsWith("/v1/notifications/test"));
    expect(testRequest?.method).toBe("POST");
    expect(document.body.textContent).toContain("disabled: local notifications are disabled in Curb policy");
  });

  it("uses service-projected acknowledgement affordance for stop sessions", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const stopSnapshot: Snapshot = {
      ...demoSnapshot,
      sessions: [
        {
          ...demoSnapshot.sessions[0],
          state: "stop",
          usage_state: "stop",
          action_state: "would-stop",
          can_acknowledge: true,
          explanation: "alert mode would stop this correlated worker",
        },
      ],
    };
    installFetch([], stopSnapshot);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    expect(document.querySelector(".detail")?.textContent).toContain("would-stop");
    expect(document.querySelector(".session-action")?.textContent).toContain("Extend");
  });

  it("confirms and submits manual stop with displayed identity evidence", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const stopSnapshot: Snapshot = {
      ...demoSnapshot,
      sessions: [
        {
          ...demoSnapshot.sessions[0],
          state: "stop",
          usage_state: "stop",
          action_state: "stop-pending",
          actionable: true,
          can_acknowledge: true,
          correlated_pid: 4242,
          correlated_process_started_at: "2026-05-21T12:00:00Z",
          correlated_owner: "phaedrus",
          correlated_executable: "/usr/local/bin/codex",
          explanation: "enforcement can stop this correlated worker",
        },
      ],
    };
    const requests = installFetch([], stopSnapshot);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    const review = Array.from(document.querySelectorAll("button")).find((button) => button.textContent?.includes("Stop now")) as
      | HTMLButtonElement
      | undefined;
    expect(review).toBeTruthy();
    await actRender(null, () => review!.click());

    const panel = document.querySelector(".stop-confirmation")?.textContent ?? "";
    expect(panel).toContain("4242");
    expect(panel).toContain("phaedrus");
    expect(panel).toContain("/usr/local/bin/codex");

    const stop = Array.from(document.querySelectorAll(".stop-confirmation button")).find((button) =>
      button.textContent?.includes("Stop process tree"),
    ) as HTMLButtonElement | undefined;
    expect(stop).toBeTruthy();
    await actRender(null, () => stop!.click());

    const stopRequest = requests.find((request) => request.url.includes("/v1/sessions/") && request.url.endsWith("/stop"));
    expect(stopRequest?.method).toBe("POST");
    expect(stopRequest?.authorization).toBe("Bearer live-token");
    expect(stopRequest?.body).toContain(`"pid":4242`);
    expect(stopRequest?.body).toContain(`"started_at":"2026-05-21T12:00:00Z"`);
    expect(stopRequest?.body).toContain(`"owner":"phaedrus"`);
    expect(stopRequest?.body).toContain(`"executable":"/usr/local/bin/codex"`);
    expect(document.querySelector(".detail")?.textContent).toContain("Stop sent to pid 4242");
  });

  it("hides manual stop when identity evidence is incomplete", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const incompleteSnapshot: Snapshot = {
      ...demoSnapshot,
      sessions: [
        {
          ...demoSnapshot.sessions[0],
          state: "stop",
          usage_state: "stop",
          action_state: "stop-pending",
          actionable: true,
          correlated_pid: 4242,
          correlated_process_started_at: "2026-05-21T12:00:00Z",
          correlated_owner: "",
          correlated_executable: "",
          explanation: "missing owner and executable evidence",
        },
      ],
    };
    installFetch([], incompleteSnapshot);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    expect(document.querySelector(".detail")?.textContent).not.toContain("Stop now");
    expect(document.querySelector(".stop-confirmation")).toBeNull();
  });

  it("hides manual stop when action state is not stop-pending", async () => {
    localStorage.setItem("curb.baseUrl", "http://127.0.0.1:8765");
    localStorage.setItem("curb.token", "live-token");
    const notActionableSnapshot: Snapshot = {
      ...demoSnapshot,
      sessions: [
        {
          ...demoSnapshot.sessions[0],
          state: "stop",
          usage_state: "stop",
          action_state: "would-stop",
          actionable: false,
          correlated_pid: 4242,
          correlated_process_started_at: "2026-05-21T12:00:00Z",
          correlated_owner: "phaedrus",
          correlated_executable: "/usr/local/bin/codex",
          explanation: "alert mode would stop this correlated worker",
        },
      ],
    };
    installFetch([], notActionableSnapshot);

    const { App } = await import("./App");
    root = createRoot(document.getElementById("root")!);
    await actRender(<App />);

    expect(document.querySelector(".detail")?.textContent).toContain("would-stop");
    expect(document.querySelector(".detail")?.textContent).not.toContain("Stop now");
    expect(document.querySelector(".stop-confirmation")).toBeNull();
  });
});

type RequestRecord = { url: string; method?: string; authorization?: string; body?: string };

function installFetch(
  alerts: AlertView[] | (() => AlertView[]),
  snapshot: Snapshot = demoSnapshot,
  turns: TurnView[] | ((url: string) => TurnView[] | Promise<TurnView[]>) = demoSnapshot.turns,
  notificationTest?: { status: number; body: unknown },
  onboarding?: OnboardingView,
): RequestRecord[] {
  const requests: RequestRecord[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      const headers = init?.headers as Record<string, string> | undefined;
      requests.push({ url, method: init?.method ?? "GET", authorization: headers?.Authorization, body: String(init?.body ?? "") });
      if (url.includes("/v1/snapshot")) return jsonResponse(snapshot);
      if (url.includes("/v1/service/rescan")) return jsonResponse(snapshot);
      if (url.includes("/v1/config")) return jsonResponse(demoConfig);
      if (url.includes("/v1/notifications/health")) {
        return jsonResponse({ enabled: true, available: true, status: "ready", message: "notifications ready" });
      }
      if (url.includes("/v1/notifications/test")) {
        if (notificationTest) {
          return jsonResponse(notificationTest.body, notificationTest.status);
        }
        return jsonResponse({
          enabled: true,
          available: true,
          status: "delivered",
          message: "test notification delivered",
          last_test_at: "2026-05-21T13:00:00Z",
        });
      }
      if (url.includes("/v1/onboarding/complete")) {
        return jsonResponse({ ...(onboarding ?? defaultOnboarding()), required: false });
      }
      if (url.includes("/v1/onboarding")) return jsonResponse(onboarding ?? defaultOnboarding());
      if (url.includes("/v1/alerts")) return jsonResponse(typeof alerts === "function" ? alerts() : alerts);
      if (url.includes("/turns")) return jsonResponse(typeof turns === "function" ? await turns(url) : turns);
      if (url.includes("/ack")) {
        return jsonResponse({
          session_key: demoSnapshot.sessions[0].key,
          extend_seconds: 1800,
          until: "2026-05-21T13:00:00Z",
        });
      }
      if (url.includes("/stop")) {
        return jsonResponse({
          session_key: demoSnapshot.sessions[0].key,
          agent_id: "codex-cli",
          pid: 4242,
          started_at: "2026-05-21T12:00:00Z",
          owner: "phaedrus",
          executable: "/usr/local/bin/codex",
          scope: "tree",
          scope_pids: [4242],
          result: { soft_signaled: [4242] },
        });
      }
      return new Response("not found", { status: 404 });
    }),
  );
  return requests;
}

function defaultOnboarding(): OnboardingView {
  return {
    required: false,
    config_path: demoConfig.path,
    mode: "alert",
    action: "notify only; never kill",
    mode_can_terminate: false,
    detected_providers: ["codex", "claude"],
    detected_workers: ["Codex Desktop Worker"],
    enforceable_agent_types: 2,
    watch_only_agent_types: 1,
    notifications: { enabled: true, available: true, status: "ready", message: "ready" },
    capabilities: demoSnapshot.overview.capabilities,
    sources: demoSnapshot.overview.sources,
    final_sentence: "Curb will notify on high-token turns. It will not stop any process in Alert mode. Desktop app roots are watch-only.",
    steps: [],
  };
}

async function actRender(element: React.ReactElement | null, action?: () => void) {
  const { act } = await import("react");
  await act(async () => {
    action?.();
    if (element) root!.render(element);
    for (let i = 0; i < 5; i++) {
      await Promise.resolve();
    }
  });
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
