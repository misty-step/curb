import type { SessionView, Snapshot } from "./types";

// Turns the service snapshot into exactly what the dashboard renders: a list of
// agents that need your eyes (working or over a line), and a quiet pile of idle
// ones folded into a count. No policy is recomputed here — only arranged.

export interface DashboardModel {
  active: SessionView[];
  idle: SessionView[];
  headline: string;
}

const ALERT_RANK: Record<string, number> = { kill: 0, warn: 1, ok: 2 };

export function selectDashboard(snapshot: Snapshot): DashboardModel {
  const sessions = [...snapshot.sessions];
  return {
    active: sessions.filter(isActive).sort(compareSessions),
    idle: sessions.filter((session) => !isActive(session)).sort(compareSessions),
    headline: snapshot.overview.message,
  };
}

/** A session earns a row when it is spending now or sitting over a line. */
export function isActive(session: SessionView): boolean {
  return session.status === "working" || session.alert !== "ok";
}

/** Most urgent first: kill before warn before ok, then working, then spend. */
export function compareSessions(left: SessionView, right: SessionView): number {
  const byAlert = (ALERT_RANK[left.alert] ?? 9) - (ALERT_RANK[right.alert] ?? 9);
  if (byAlert !== 0) return byAlert;
  const byStatus = Number(right.status === "working") - Number(left.status === "working");
  if (byStatus !== 0) return byStatus;
  return right.turn_tokens - left.turn_tokens;
}

/** Where the spend bar fills, as a 0..1 fraction of the kill line. */
export function fillRatio(turnTokens: number, killTokens: number): number {
  if (killTokens <= 0) return 0;
  return clamp01(turnTokens / killTokens);
}

/** Where the warn tick sits on the bar, as a 0..1 fraction of the kill line. */
export function warnRatio(warnTokens: number, killTokens: number): number {
  if (killTokens <= 0) return 0;
  return clamp01(warnTokens / killTokens);
}

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}
