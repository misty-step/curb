import { describe, expect, it } from "vitest";
import { demoSnapshot } from "./demo";
import {
  compareSessions,
  fillRatio,
  isActive,
  selectDashboard,
  selectReadiness,
  selectRecovery,
  selectSessionExplanation,
  warnRatio,
} from "./readModel";
import type { NotificationView, OnboardingView, ReadinessView, SessionView, TurnView } from "./types";

function session(overrides: Partial<SessionView>): SessionView {
  return {
    key: overrides.key ?? "k",
    id: overrides.id ?? "k",
    provider: "codex",
    status: "idle",
    alert: "ok",
    can_stop: false,
    can_acknowledge: false,
    models: [],
    turn_tokens: 0,
    turn_context_tokens: 0,
    total_tokens: 0,
    calls: 0,
    explanation: "",
    ...overrides,
  };
}

describe("selectDashboard", () => {
  it("splits working/alerting agents from recently-idle ones", () => {
    const model = selectDashboard(demoSnapshot, 900);
    // sorted by urgency: kill, then warn, then working
    expect(model.active.map((entry) => entry.id)).toEqual(["olympus", "gradient", "curb"]);
    expect(model.idle.map((entry) => entry.id)).toEqual(["daybook"]);
    expect(model.headline).toBe("1 over the kill line");
  });

  it("drops finished sessions: old activity, no live worker, is not an agent", () => {
    const dead = session({ id: "dead", key: "codex:dead", last_activity_at: "2026-05-28T17:00:00Z" });
    const snapshot = { ...demoSnapshot, sessions: [...demoSnapshot.sessions, dead] };
    const model = selectDashboard(snapshot, 900);
    expect(model.idle.map((entry) => entry.id)).toEqual(["daybook"]);
    expect([...model.active, ...model.idle].some((entry) => entry.id === "dead")).toBe(false);
  });
});

describe("isActive", () => {
  it("treats working or over-a-line sessions as active", () => {
    expect(isActive(session({ status: "working" }))).toBe(true);
    expect(isActive(session({ alert: "warn" }))).toBe(true);
    expect(isActive(session({ alert: "kill" }))).toBe(true);
    expect(isActive(session({ status: "idle", alert: "ok" }))).toBe(false);
  });
});

describe("compareSessions", () => {
  it("orders kill before warn before working before idle, then by spend", () => {
    const rows = [
      session({ id: "ok-busy", status: "working", turn_tokens: 10 }),
      session({ id: "kill", alert: "kill", turn_tokens: 1 }),
      session({ id: "warn", alert: "warn", turn_tokens: 1 }),
      session({ id: "ok-quiet", status: "working", turn_tokens: 99 }),
    ];
    const ordered = [...rows].sort(compareSessions).map((entry) => entry.id);
    expect(ordered).toEqual(["kill", "warn", "ok-quiet", "ok-busy"]);
  });
});

describe("bar ratios", () => {
  it("fills toward the kill line and places the warn tick", () => {
    expect(fillRatio(1_500_000, 3_000_000)).toBeCloseTo(0.5);
    expect(fillRatio(9_000_000, 3_000_000)).toBe(1);
    expect(warnRatio(1_000_000, 3_000_000)).toBeCloseTo(1 / 3);
    expect(fillRatio(100, 0)).toBe(0);
  });
});

describe("selectSessionExplanation", () => {
  it("builds selected-session evidence and a per-turn token timeline", () => {
    const selected = selectSessionExplanation(
      session({
        alert: "kill",
        can_stop: false,
        can_acknowledge: true,
        pid: 4242,
        process_started_at: "2026-05-29T16:00:00Z",
        owner: "phaedrus",
        executable: "/usr/local/bin/codex",
        bundle_id: "com.openai.codex",
        team_id: "OPENAI",
        explanation: "Over kill, but this is watch-only.",
      }),
      [
        turn({
          id: "turn-2",
          request_id: "req-2",
          model: "gpt-5.5",
          input_tokens: 100,
          cached_input_tokens: 25,
          cache_creation_input_tokens: 5,
          output_tokens: 40,
          reasoning_output_tokens: 10,
          total_tokens: 180,
          spent_tokens: 150,
          cumulative_tokens: 330,
          source: "codex usage log",
        }),
      ],
    );

    expect(selected?.actionEvidence).toContainEqual({ label: "Stop", value: "Unavailable: Over kill, but this is watch-only." });
    expect(selected?.correlationEvidence.map((entry) => entry.label)).toEqual([
      "PID",
      "Start-time seal",
      "Owner",
      "Executable",
      "Bundle",
      "Team",
    ]);
    expect(selected?.turns[0]).toMatchObject({
      label: "turn-2",
      model: "gpt-5.5",
      source: "codex usage log",
      inputTokens: 100,
      cachedInputTokens: 25,
      cacheCreationTokens: 5,
      outputTokens: 40,
      reasoningTokens: 10,
      totalTokens: 180,
      spentTokens: 150,
      cumulativeTokens: 330,
    });
  });
});

describe("selectReadiness", () => {
  it("surfaces first-run, notification, and platform capability state", () => {
    const model = selectReadiness(onboarding(true), notification(false), onboarding(true).capabilities);

    expect(model.attention).toBe(false);
    expect(model.summary).toBe("Using safe defaults");
    expect(model.nextStep).toBe("Curb will notify on high-token turns.");
    expect(model.primary).toMatchObject({ label: "Setup", status: "required" });
    expect(model.details.map((item) => item.label)).toEqual(["Notifications", "Process capture", "Identity", "Enforcement"]);
    expect(model.items.map((item) => item.label)).toEqual([
      "Setup",
      "Notifications",
      "Process capture",
      "Identity",
      "Enforcement",
    ]);
    expect(model.items[0]).toMatchObject({
      status: "required",
      message: "Curb will notify on high-token turns.",
      attention: false,
      tone: "ok",
    });
    expect(model.items[1]).toMatchObject({ status: "disabled", message: "notifications disabled", tone: "muted" });
    expect(model.items[4]).toMatchObject({ status: "watch mode", tone: "muted", attention: false });
  });

  it("does not call setup ready until onboarding has answered", () => {
    const model = selectReadiness(undefined, notification(true), onboarding(false).capabilities);

    expect(model.attention).toBe(true);
    expect(model.summary).toBe("Setup status unavailable");
    expect(model.nextStep).toBe("Connect to the local Curb API to confirm setup.");
    expect(model.items[0]).toMatchObject({
      label: "Setup",
      status: "unknown",
      message: "Connect to the local Curb API to confirm setup.",
      tone: "attention",
    });
  });
});

describe("selectRecovery", () => {
  it("renders service-owned recovery items from onboarding and readiness without duplicating them", () => {
    const currentOnboarding = onboarding(true);
    currentOnboarding.recovery = [
      {
        id: "source-codex",
        label: "codex source",
        status: "error",
        message: "codex usage metadata could not be read. Raw provider paths and payloads are not shown in recovery.",
        action: "Run `curb usage --since 24h`.",
        command: "curb usage --since 24h",
      },
      {
        id: "readiness-watcher_runtime",
        label: "Watcher runtime",
        status: "error",
        message: "duplicate should lose",
        action: "Run `curb watch --once`.",
      },
    ];
    const currentReadiness: ReadinessView = {
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
          action: "Run `curb watch --once`.",
          command: "curb watch --once",
        },
      ],
    };

    const model = selectRecovery(currentOnboarding, currentReadiness);

    expect(model.attention).toBe(true);
    expect(model.summary).toBe("2 recovery items");
    expect(model.nextStep).toBe("Run `curb usage --since 24h`.");
    expect(model.items.map((item) => item.id)).toEqual(["source-codex", "readiness-watcher_runtime"]);
    expect(model.items[0].message).not.toContain("/Users/");
  });

  it("turns API failures into sanitized recovery actions", () => {
    const model = selectRecovery(undefined, undefined, "Failed to fetch", "/tmp/curb/config.yaml");

    expect(model.items).toEqual([
      expect.objectContaining({
        id: "api-connection",
        label: "API connection",
        message: "The dashboard could not reach the local Curb API.",
        command: "curb serve --config /tmp/curb/config.yaml",
        path: "/tmp/curb/api.token",
        runbook: "docs/user-guide.md#local-ui-api",
      }),
    ]);
    expect(model.nextStep).toBe("Run `curb serve --config /tmp/curb/config.yaml` from the same config and inspect /tmp/curb/api.token.");
    expect(model.nextStep).not.toContain("Failed to fetch");
  });
});

function turn(overrides: Partial<TurnView>): TurnView {
  return {
    id: "turn-1",
    request_id: "req-1",
    session_key: "k",
    session_id: "k",
    provider: "codex",
    at: "2026-05-29T17:00:00Z",
    model: "model",
    input_tokens: 0,
    cached_input_tokens: 0,
    output_tokens: 0,
    cache_creation_input_tokens: 0,
    reasoning_output_tokens: 0,
    total_tokens: 0,
    spent_tokens: 0,
    cumulative_tokens: 0,
    source: "test",
    ...overrides,
  };
}

function onboarding(required: boolean): OnboardingView {
  return {
    required,
    config_path: "/tmp/curb/config.yaml",
    mode: "alert",
    action: "notify only; never kill",
    mode_can_terminate: false,
    detected_providers: ["codex"],
    detected_workers: ["Codex Worker"],
    enforceable_agent_types: 1,
    watch_only_agent_types: 1,
    notifications: notification(true),
    capabilities: {
      platform: "test",
      notifications: { available: true, status: "ready", message: "notifications ready" },
      process_capture: { available: true, status: "ready", message: "process capture available" },
      process_identity: { available: true, status: "ready", message: "identity evidence available" },
      enforcement: { available: false, status: "disabled", message: "current mode never terminates processes" },
    },
    sources: [],
    final_sentence: "Curb will notify on high-token turns.",
    steps: [],
    recovery: [],
  };
}

function notification(available: boolean): NotificationView {
  return {
    enabled: available,
    available,
    status: available ? "ready" : "disabled",
    message: available ? "notifications ready" : "notifications disabled",
  };
}
