import { describe, expect, it } from "vitest";
import { demoSnapshot } from "./demo";
import {
  aliveAgentGroups,
  isRecentUncorrelatedUsage,
  selectOperatorSummary,
  sessionForAgent,
} from "./readModel";
import type { AgentView, SessionView, Snapshot } from "./types";

const baseAgent: AgentView = {
  id: "codex-desktop-worker",
  provider: "codex",
  label: "Codex Desktop Worker",
  state: "running",
  process_state: "running",
  usage_state: "quiet",
  action_state: "none",
  actionable: false,
  pid: 100,
  confidence: 95,
  running_for_seconds: 120,
  project: "curb",
  cwd: "/work/curb",
  explanation: "matched worker process",
};

const baseSession: SessionView = {
  key: "codex:s1",
  id: "s1",
  provider: "codex",
  state: "active",
  process_state: "running",
  usage_state: "quiet",
  action_state: "none",
  actionable: false,
  can_acknowledge: false,
  last_seen_at: "2026-05-26T16:00:00Z",
  calls: 1,
  total_tokens: 10_000,
  risk_rank: 0,
  acknowledged: false,
  explanation: "normal usage",
};

describe("read model selectors", () => {
  it("separates live workers from active token spend and unmatched usage", () => {
    const snapshot: Snapshot = {
      ...demoSnapshot,
      agents: [
        {
          ...baseAgent,
          state: "spending",
          usage_state: "spending",
          latest_session_id: "s1",
          latest_turn_tokens: 125_000,
          window_tokens: 150_000,
        },
        {
          ...baseAgent,
          pid: 101,
          latest_session_id: undefined,
          latest_turn_tokens: undefined,
          window_tokens: undefined,
        },
        {
          ...baseAgent,
          pid: 0,
          state: "ended",
          process_state: "ended",
        },
      ],
      sessions: [
        { ...baseSession, id: "s1", key: "codex:s1", correlated_pid: 100, latest_turn_tokens: 125_000, window_tokens: 150_000 },
        {
          ...baseSession,
          id: "unmatched",
          key: "codex:unmatched",
          state: "uncorrelated",
          process_state: "no-process",
          correlated_pid: undefined,
          latest_turn_tokens: 80_000,
          window_tokens: 80_000,
        },
      ],
    };

    const model = selectOperatorSummary(snapshot);

    expect(model.aliveAgents).toHaveLength(2);
    expect(model.spendingAgents).toHaveLength(1);
    expect(model.latestInputTokens).toBe(125_000);
    expect(model.recentUncorrelated).toHaveLength(1);
    expect(model.recentUncorrelatedTokens).toBe(80_000);
    expect(model.headline).toBe("1 agent actively consuming tokens");
  });

  it("groups multiple worker processes by agent and cwd without multiplying sessions", () => {
    const groups = aliveAgentGroups([
      {
        ...baseAgent,
        pid: 1,
        provider: "codex",
        id: "codex-desktop-worker",
        process_started_at: "2026-05-26T15:00:00Z",
      },
      {
        ...baseAgent,
        pid: 2,
        provider: "codex",
        id: "codex-desktop-worker",
        process_started_at: "2026-05-26T15:05:00Z",
        running_for_seconds: 300,
      },
      {
        ...baseAgent,
        pid: 3,
        provider: "antigravity",
        id: "antigravity-cli",
        label: "Anti-Gravity CLI",
        cwd: "/work/daybook",
        project: "daybook",
      },
    ]);

    expect(groups[0]).toMatchObject({ provider: "antigravity", project: "daybook", count: 1 });
    expect(groups[1]).toMatchObject({ provider: "codex", project: "curb", count: 2, runningForSeconds: 300 });
  });

  it("matches an agent to its exact latest session before pid fallback", () => {
    const agent = { ...baseAgent, pid: 200, latest_session_id: "new" };
    const oldSession = { ...baseSession, id: "old", key: "codex:old", correlated_pid: 200 };
    const newSession = { ...baseSession, id: "new", key: "codex:new", correlated_pid: 200 };

    expect(sessionForAgent(agent, [oldSession, newSession])?.key).toBe("codex:new");
    expect(sessionForAgent({ ...agent, latest_session_id: "missing" }, [oldSession, newSession])?.key).toBe("codex:old");
  });

  it("treats sessions with recent tokens and no pid as uncorrelated usage", () => {
    expect(isRecentUncorrelatedUsage({ ...baseSession, correlated_pid: undefined, window_tokens: 42 })).toBe(true);
    expect(isRecentUncorrelatedUsage({ ...baseSession, state: "uncorrelated", window_tokens: 0 })).toBe(true);
    expect(isRecentUncorrelatedUsage({ ...baseSession, correlated_pid: 100, window_tokens: 42 })).toBe(false);
  });
});
