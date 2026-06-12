import type {
  CapabilityView,
  NotificationView,
  OnboardingView,
  PlatformCapabilities,
  ReadinessView,
  RecoveryItemView,
  SessionView,
  Snapshot,
  TurnView,
} from "./types";

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

export interface EvidenceItem {
  label: string;
  value: string;
}

export interface TimelineTurn {
  label: string;
  provider: string;
  at?: string;
  model?: string;
  source: string;
  inputTokens: number;
  cachedInputTokens: number;
  cacheCreationTokens: number;
  outputTokens: number;
  reasoningTokens: number;
  totalTokens: number;
  spentTokens: number;
  cumulativeTokens: number;
}

export interface SelectedSessionExplanation {
  session: SessionView;
  turns: TimelineTurn[];
  correlationEvidence: EvidenceItem[];
  actionEvidence: EvidenceItem[];
}

export interface ReadinessItem {
  label: string;
  status: string;
  message: string;
  attention: boolean;
  tone: "ok" | "attention" | "warn" | "muted";
}

export interface ReadinessModel {
  attention: boolean;
  summary: string;
  nextStep: string;
  readyCount: number;
  primary: ReadinessItem;
  details: ReadinessItem[];
  items: ReadinessItem[];
}

export interface RecoveryItem {
  id: string;
  label: string;
  status: string;
  message: string;
  action: string;
  command?: string;
  path?: string;
  runbook?: string;
}

export interface RecoveryModel {
  attention: boolean;
  summary: string;
  nextStep: string;
  items: RecoveryItem[];
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

export function selectSessionExplanation(
  session: SessionView | undefined,
  turns: TurnView[],
): SelectedSessionExplanation | undefined {
  if (!session) return undefined;
  return {
    session,
    turns: turns.map((turn, index) => ({
      label: turn.id || turn.request_id || `turn ${index + 1}`,
      provider: turn.provider,
      at: turn.at ?? undefined,
      model: turn.model ?? undefined,
      source: turn.source,
      inputTokens: turn.input_tokens,
      cachedInputTokens: turn.cached_input_tokens,
      cacheCreationTokens: turn.cache_creation_input_tokens,
      outputTokens: turn.output_tokens,
      reasoningTokens: turn.reasoning_output_tokens,
      totalTokens: turn.total_tokens,
      spentTokens: turn.spent_tokens,
      cumulativeTokens: turn.cumulative_tokens,
    })),
    correlationEvidence: correlationEvidence(session),
    actionEvidence: actionEvidence(session),
  };
}

export function selectReadiness(
  onboarding: OnboardingView | undefined,
  notifications: NotificationView,
  capabilities: PlatformCapabilities,
): ReadinessModel {
  const items: ReadinessItem[] = [
    onboardingItem(onboarding),
    {
      label: "Notifications",
      status: notifications.status,
      message: notifications.message,
      attention: notifications.enabled && !notifications.available,
      tone: notifications.enabled && !notifications.available ? "attention" : notifications.enabled ? "ok" : "muted",
    },
    capabilityItem("Process capture", capabilities.process_capture),
    capabilityItem("Identity", capabilities.process_identity),
    capabilityItem("Enforcement", capabilities.enforcement),
  ];
  const attention = items.some((item) => item.attention);
  const firstAttention = items.find((item) => item.attention);
  const setup = items[0];
  const primary = firstAttention ?? setup;
  const readyCount = items.filter((item) => item.tone === "ok").length;
  return {
    attention,
    summary: firstAttention
      ? setupSummary(firstAttention.status)
      : setup.status === "required"
        ? setupSummary(setup.status)
        : "Monitoring is ready",
    nextStep: firstAttention
      ? firstAttention.message
      : setup.status === "required"
        ? setup.message
      : "Curb is watching agent spend with your current limits.",
    readyCount,
    primary,
    details: items.filter((item) => item !== primary),
    items,
  };
}

export function selectRecovery(
  onboarding: OnboardingView | undefined,
  readiness: ReadinessView | undefined,
  connectionError = "",
  configPath?: string,
): RecoveryModel {
  const items = dedupeRecovery([
    ...connectionRecovery(connectionError, configPath ?? onboarding?.config_path),
    ...(onboarding?.recovery ?? []),
    ...(readiness?.recovery ?? []),
  ]);
  const attention = items.length > 0;
  return {
    attention,
    summary: attention ? `${items.length} recovery ${items.length === 1 ? "item" : "items"}` : "No recovery needed",
    nextStep: items[0]?.action ?? "Curb has no operator recovery items.",
    items,
  };
}

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}

function correlationEvidence(session: SessionView): EvidenceItem[] {
  return [
    session.pid ? { label: "PID", value: String(session.pid) } : undefined,
    session.process_started_at ? { label: "Start-time seal", value: session.process_started_at } : undefined,
    session.owner ? { label: "Owner", value: session.owner } : undefined,
    session.executable ? { label: "Executable", value: session.executable } : undefined,
    session.bundle_id ? { label: "Bundle", value: session.bundle_id } : undefined,
    session.team_id ? { label: "Team", value: session.team_id } : undefined,
  ].filter((item): item is EvidenceItem => Boolean(item));
}

function actionEvidence(session: SessionView): EvidenceItem[] {
  const stop = session.can_stop
    ? "Available after live identity revalidation."
    : `Unavailable: ${session.explanation}`;
  const ack = session.can_acknowledge
    ? "Available for a bounded grace window."
    : "Unavailable for this session state.";
  return [
    { label: "Alert", value: session.alert },
    { label: "Stop", value: stop },
    { label: "Acknowledge", value: ack },
  ];
}

function onboardingItem(onboarding: OnboardingView | undefined): ReadinessItem {
  if (!onboarding) {
    return {
      label: "Setup",
      status: "unknown",
      message: "Connect to the local Curb API to confirm setup.",
      attention: true,
      tone: "attention",
    };
  }
  if (onboarding.required) {
    return {
      label: "Setup",
      status: "required",
      message: onboarding.final_sentence || "Curb is using safe defaults. Review setup when you want to tune it.",
      attention: false,
      tone: "ok",
    };
  }
  return {
    label: "Setup",
    status: "ready",
    message: onboarding.final_sentence || "First-run setup complete",
    attention: false,
    tone: "ok",
  };
}

function capabilityItem(label: string, capability: CapabilityView): ReadinessItem {
  const attention = !capability.available && capability.status !== "disabled";
  const disabled = capability.status === "disabled";
  return {
    label,
    status: label === "Enforcement" && disabled ? "watch mode" : capability.status,
    message: capability.message,
    attention,
    tone: attention ? "attention" : disabled ? "muted" : capability.available ? "ok" : "warn",
  };
}

function setupSummary(status: string): string {
  if (status === "unknown") return "Setup status unavailable";
  if (status === "required") return "Using safe defaults";
  return "Setup needs attention";
}

function dedupeRecovery(items: RecoveryItemView[]): RecoveryItem[] {
  const seen = new Set<string>();
  const deduped: RecoveryItem[] = [];
  for (const item of items) {
    if (seen.has(item.id)) continue;
    seen.add(item.id);
    deduped.push(item);
  }
  return deduped;
}

function connectionRecovery(error: string, configPath: string | undefined): RecoveryItemView[] {
  if (!error) return [];
  const hasConfigPath = Boolean(configPath && configPath !== "demo");
  const command = hasConfigPath ? `curb serve --config ${configPath}` : "curb app";
  const tokenPath = hasConfigPath ? `${parentPath(configPath!)}/api.token` : "<state_dir>/api.token";
  const authFailure = /^40[13]\b/.test(error);
  const devServer = error.includes("<!doctype") || error.includes("not valid JSON");
  const message = authFailure
    ? "The dashboard reached the Curb API, but the browser session is not authenticated for protected routes."
    : devServer
      ? "The dashboard reached the frontend dev server instead of the Curb API."
      : "The dashboard could not reach the local Curb API.";
  return [
    {
      id: "api-connection",
      label: authFailure ? "API authentication" : "API connection",
      status: authFailure ? "auth required" : "unavailable",
      message,
      action: `Run \`${command}\` from the same config and inspect ${tokenPath}.`,
      command,
      path: tokenPath,
      runbook: "docs/user-guide.md#local-ui-api",
    },
  ];
}

function parentPath(path: string): string {
  const normalized = path.replace(/\/+$/, "");
  const index = normalized.lastIndexOf("/");
  if (index <= 0) return ".curb";
  return normalized.slice(0, index);
}
