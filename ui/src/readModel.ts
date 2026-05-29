import type { SessionView, Snapshot } from "./types";

// Turns the service snapshot into what the dashboard renders. Three buckets:
//
//   active  - spending now, or sitting over a line. These get rows.
//   idle    - an agent you are still using: it has a live worker, or it spent
//             something within the live window, but it is not spending now.
//   (ended) - last activity older than the window with no live worker. These
//             are finished runs, not agents. They are dropped, not shown — that
//             is the difference between an idle agent and a dead session.

export interface DashboardModel {
  active: SessionView[];
  idle: SessionView[];
  headline: string;
}

const ALERT_RANK: Record<string, number> = { kill: 0, warn: 1, ok: 2 };

// A working agent's checkpoints arrive in bursts with gaps between model calls
// (tool runs, thinking). Keep its row on screen across those gaps so it does not
// flicker in and out; only when it has been quiet this long does it fall to the
// idle fold. This is row-presence, distinct from the tighter "spending now" pulse.
const ACTIVE_WINDOW_SECONDS = 120;

export function selectDashboard(snapshot: Snapshot, liveWindowSeconds: number): DashboardModel {
  const now = Date.parse(snapshot.overview.last_scan) || Date.now();
  // On screen as a row: spending, over a line, or active in the last ~2 min.
  const onScreen = (session: SessionView) => isActive(session) || isLive(session, now, ACTIVE_WINDOW_SECONDS);
  const sessions = [...snapshot.sessions];
  return {
    active: sessions.filter(onScreen).sort(compareSessions),
    idle: sessions
      .filter((session) => !onScreen(session) && isLive(session, now, liveWindowSeconds))
      .sort(compareSessions),
    headline: snapshot.overview.message,
  };
}

/** Spending now, or over a line — needs a row. */
export function isActive(session: SessionView): boolean {
  return session.status === "working" || session.alert !== "ok";
}

/** A still-relevant agent has spent something within the live window. Recency
 * alone — not process correlation — decides this: correlation is cwd-based, so
 * a long-finished run in an active repo would otherwise borrow the liveness of
 * whatever agent is running there now and never age out. Anything older is a
 * finished run, not an idle agent. */
export function isLive(session: SessionView, now: number, liveWindowSeconds: number): boolean {
  if (!session.last_activity_at) return false;
  const age = now - Date.parse(session.last_activity_at);
  return Number.isFinite(age) && age <= liveWindowSeconds * 1000;
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
