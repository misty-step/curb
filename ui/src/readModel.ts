import type { AgentView, SessionView, Snapshot } from "./types";

export interface AliveAgentGroup {
  id: string;
  provider: string;
  label: string;
  project: string;
  cwd: string;
  count: number;
  runningForSeconds: number;
  latestStarted: number;
}

export interface OperatorSummaryModel {
  aliveAgents: AgentView[];
  spendingAgents: AgentView[];
  recentUncorrelated: SessionView[];
  latestSpentTokens: number;
  recentUncorrelatedTokens: number;
  aliveRows: AliveAgentGroup[];
  quietRows: AliveAgentGroup[];
  spendingRows: AgentView[];
  headline: string;
}

export function selectOperatorSummary(snapshot: Snapshot): OperatorSummaryModel {
  const aliveAgents = snapshot.agents.filter(isAliveAgent);
  const spendingAgents = aliveAgents.filter(isSpendingAgent);
  const spendingRows = uniqueSpendingRows(spendingAgents);
  const recentUncorrelated = snapshot.sessions.filter(isRecentUncorrelatedUsage);
  const latestSpentTokens = spendingRows.reduce((sum, agent) => sum + (agent.latest_spent_tokens ?? agent.latest_turn_tokens ?? 0), 0);
  const recentUncorrelatedTokens = recentUncorrelated.reduce((sum, session) => sum + (session.window_spent_tokens ?? session.window_tokens ?? 0), 0);

  return {
    aliveAgents,
    spendingAgents,
    recentUncorrelated,
    latestSpentTokens,
    recentUncorrelatedTokens,
    aliveRows: aliveAgentGroups(aliveAgents).slice(0, 6),
    quietRows: aliveAgentGroups(aliveAgents.filter((agent) => !isSpendingAgent(agent))).slice(0, 4),
    spendingRows: spendingRows.slice(0, 5),
    headline:
      spendingRows.length > 0
        ? `${spendingRows.length} agent${spendingRows.length === 1 ? "" : "s"} with fresh token usage`
        : "No fresh token usage right now",
  };
}

export function isAliveAgent(agent: AgentView): boolean {
  return agent.state !== "ended" && agent.pid > 0;
}

export function isSpendingAgent(agent: AgentView): boolean {
  return agent.activity_state === "spending";
}

function uniqueSpendingRows(agents: AgentView[]): AgentView[] {
  const rows = new Map<string, AgentView>();
  for (const agent of agents) {
    const key = agent.latest_session_id
      ? `${agent.provider}:${agent.latest_session_id}`
      : `${agent.provider}:${agent.project || agent.cwd || agent.pid}:${agent.latest_spent_tokens ?? agent.latest_turn_tokens ?? 0}`;
    const current = rows.get(key);
    if (!current || (agent.window_spent_tokens ?? agent.window_tokens ?? 0) > (current.window_spent_tokens ?? current.window_tokens ?? 0)) {
      rows.set(key, agent);
    }
  }
  return Array.from(rows.values());
}

export function isRecentUncorrelatedUsage(session: SessionView): boolean {
  return session.state === "uncorrelated" || (session.correlated_pid === undefined && (session.window_spent_tokens ?? session.window_tokens ?? 0) > 0);
}

export function sessionForAgent(agent: AgentView, sessions: SessionView[]): SessionView | undefined {
  return sessions.find((session) => session.correlated_pid === agent.pid && session.id === agent.latest_session_id) ??
    sessions.find((session) => session.correlated_pid === agent.pid);
}

export function aliveAgentSummary(agents: AgentView[]): string {
  const counts = new Map<string, number>();
  for (const agent of agents) {
    const label = agent.label || agent.id;
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  return Array.from(counts.entries())
    .slice(0, 3)
    .map(([label, count]) => `${count} ${label}`)
    .join(" · ");
}

export function aliveAgentGroups(agents: AgentView[]): AliveAgentGroup[] {
  const groups = new Map<string, AliveAgentGroup>();
  for (const agent of agents) {
    const key = `${agent.id}:${agent.cwd || agent.project || agent.pid}`;
    const current = groups.get(key);
    const started = agent.process_started_at ? new Date(agent.process_started_at).getTime() : 0;
    if (!current) {
      groups.set(key, {
        id: agent.id,
        provider: agent.provider,
        label: agent.label || agent.id,
        project: agent.project || "",
        cwd: agent.cwd || "",
        count: 1,
        runningForSeconds: agent.running_for_seconds ?? 0,
        latestStarted: started,
      });
      continue;
    }
    current.count += 1;
    current.runningForSeconds = Math.max(current.runningForSeconds, agent.running_for_seconds ?? 0);
    current.latestStarted = Math.max(current.latestStarted, started);
  }
  return Array.from(groups.values()).sort((left, right) => {
    if (left.provider !== right.provider) {
      if (left.provider === "antigravity") return -1;
      if (right.provider === "antigravity") return 1;
    }
    if (left.latestStarted !== right.latestStarted) return right.latestStarted - left.latestStarted;
    return right.count - left.count;
  });
}
