import { describe, expect, it } from "vitest";
import { demoSnapshot } from "./demo";
import {
  aliveAgentGroups,
  isRecentUncorrelatedUsage,
  selectOperatorSummary,
  sessionActivityRows,
  sessionForAgent,
} from "./readModel";
import type { AgentView, SessionView, Snapshot } from "./types";

const baseAgent: AgentView = {
  id: "codex-desktop-worker",
  provider: "codex",
  label: "Codex Desktop Worker",
  state: "running",
  activity_state: "idle",
  data_recency: "none",
  activity_basis: "process is running with no correlated usage",
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
  activity_state: "idle",
  data_recency: "recent",
  activity_basis: "recent completed usage checkpoint correlated to a live worker",
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
          activity_state: "spending",
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
        {
          ...baseSession,
          id: "s1",
          key: "codex:s1",
          activity_state: "spending",
          correlated_pid: 100,
          latest_turn_tokens: 125_000,
          window_tokens: 150_000,
        },
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
    expect(model.activeSessionRows).toHaveLength(1);
    expect(model.freshSessionRows).toHaveLength(1);
    expect(model.latestSpentTokens).toBe(125_000);
    expect(model.recentUncorrelated).toHaveLength(1);
    expect(model.recentUncorrelatedTokens).toBe(80_000);
    expect(model.headline).toBe("1 run with fresh usage checkpoints");
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

  it("uses sessions as primary active rows when multiple workers point at the same run", () => {
    const snapshot: Snapshot = {
      ...demoSnapshot,
      agents: [
        {
          ...baseAgent,
          pid: 100,
          state: "warn",
          activity_state: "spending",
          usage_state: "warn",
          latest_session_id: "canary-session",
          latest_turn_tokens: 29_000,
          window_tokens: 100_000,
          project: "canary",
          cwd: "/work/canary",
        },
        {
          ...baseAgent,
          pid: 101,
          state: "warn",
          activity_state: "spending",
          usage_state: "warn",
          latest_session_id: "canary-session",
          latest_turn_tokens: 29_000,
          window_tokens: 100_000,
          project: "canary",
          cwd: "/work/canary",
        },
      ],
      sessions: [
        {
          ...baseSession,
          id: "canary-session",
          key: "codex:canary-session",
          activity_state: "spending",
          process_state: "running",
          usage_state: "warn",
          correlated_pid: 100,
          latest_turn_tokens: 29_000,
          window_tokens: 100_000,
        },
      ],
    };

    const model = selectOperatorSummary(snapshot);

    expect(model.spendingAgents).toHaveLength(2);
    expect(model.activeSessionRows).toHaveLength(1);
    expect(model.freshSessionRows).toHaveLength(1);
    expect(model.activeSessionRows[0]).toMatchObject({ workerCount: 2 });
    expect(model.latestSpentTokens).toBe(29_000);
    expect(model.headline).toBe("1 run with fresh usage checkpoints");
  });

  it("does not treat a stale policy warning as fresh token usage", () => {
    const snapshot: Snapshot = {
      ...demoSnapshot,
      agents: [
        {
          ...baseAgent,
          state: "warn",
          activity_state: "idle",
          usage_state: "warn",
          latest_session_id: "old-warning",
          latest_turn_tokens: 250_000,
          window_tokens: 250_000,
        },
      ],
      sessions: [
        {
          ...baseSession,
          id: "old-warning",
          key: "codex:old-warning",
          state: "warn",
          activity_state: "idle",
          usage_state: "warn",
          correlated_pid: 100,
          latest_turn_tokens: 250_000,
          window_tokens: 250_000,
        },
      ],
    };

    const model = selectOperatorSummary(snapshot);

    expect(model.spendingAgents).toHaveLength(0);
    expect(model.activeSessionRows).toHaveLength(1);
    expect(model.freshSessionRows).toHaveLength(0);
    expect(model.spendingRows).toHaveLength(0);
    expect(model.headline).toBe("No fresh token usage right now");
  });

  it("sorts active session rows by fresh activity and window spend", () => {
    const rows = sessionActivityRows(
      [
        {
          ...baseSession,
          id: "quiet",
          key: "codex:quiet",
          activity_state: "idle",
          process_state: "running",
          correlated_pid: 10,
          window_tokens: 900_000,
          last_seen_at: "2026-05-26T16:00:00Z",
        },
        {
          ...baseSession,
          id: "active",
          key: "codex:active",
          activity_state: "spending",
          process_state: "running",
          correlated_pid: 11,
          window_tokens: 100_000,
          last_seen_at: "2026-05-26T16:01:00Z",
        },
      ],
      [
        { ...baseAgent, pid: 10, latest_session_id: "quiet" },
        { ...baseAgent, pid: 11, latest_session_id: "active", running_for_seconds: 42 },
      ],
    );

    expect(rows.map((row) => row.session.id)).toEqual(["active", "quiet"]);
    expect(rows[0]).toMatchObject({ workerCount: 1, workerLabel: "Codex Desktop Worker" });
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
