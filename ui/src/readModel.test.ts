import { describe, expect, it } from "vitest";
import { demoSnapshot } from "./demo";
import { compareSessions, fillRatio, isActive, selectDashboard, warnRatio } from "./readModel";
import type { SessionView } from "./types";

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
    expect(model.active.map((entry) => entry.id)).toEqual(["gradient", "curb"]);
    expect(model.idle.map((entry) => entry.id)).toEqual(["daybook"]);
    expect(model.headline).toBe("1 over the warn line");
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
