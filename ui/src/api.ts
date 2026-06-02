import { demoConfig, demoNotifications, demoSnapshot } from "./demo";
import type {
  AckView,
  ConfigUpdate,
  ConfigView,
  NotificationView,
  OnboardingView,
  SessionView,
  Snapshot,
  StopExpectedIdentity,
  StopView,
  TurnView,
} from "./types";

// Thin client over the loopback API. With no base URL it returns demo data, so
// the dashboard renders something safe before it connects.

export interface ApiSettings {
  baseUrl: string;
  token: string;
}

export async function fetchSnapshot(settings: ApiSettings): Promise<Snapshot> {
  if (!settings.baseUrl) return demoSnapshot;
  return getJSON(settings, "/v1/snapshot") as Promise<Snapshot>;
}

export async function fetchSession(settings: ApiSettings, sessionKey: string): Promise<SessionView> {
  if (!settings.baseUrl) {
    return demoSnapshot.sessions.find((session) => session.key === sessionKey) ?? demoSnapshot.sessions[0];
  }
  return getJSON(settings, `/v1/sessions/${encodeURIComponent(sessionKey)}`) as Promise<SessionView>;
}

export async function fetchSessionTurns(
  settings: ApiSettings,
  sessionKey: string,
  limit = 20,
): Promise<TurnView[]> {
  if (!settings.baseUrl) return demoSnapshot.turns.filter((turn) => turn.session_key === sessionKey).slice(0, limit);
  return getJSON(settings, `/v1/sessions/${encodeURIComponent(sessionKey)}/turns?limit=${limit}`) as Promise<TurnView[]>;
}

export async function rescanService(settings: ApiSettings): Promise<Snapshot> {
  if (!settings.baseUrl) return demoSnapshot;
  return getJSON(settings, "/v1/service/rescan", { method: "POST" }) as Promise<Snapshot>;
}

export async function fetchConfig(settings: ApiSettings): Promise<ConfigView> {
  if (!settings.baseUrl) return demoConfig;
  return getJSON(settings, "/v1/config") as Promise<ConfigView>;
}

export async function saveConfig(settings: ApiSettings, update: ConfigUpdate): Promise<ConfigView> {
  if (!settings.baseUrl) return { ...demoConfig, ...update };
  return getJSON(settings, "/v1/config", {
    method: "PUT",
    body: JSON.stringify(update),
  }) as Promise<ConfigView>;
}

export async function fetchNotificationHealth(settings: ApiSettings): Promise<NotificationView> {
  if (!settings.baseUrl) return demoNotifications;
  return getJSON(settings, "/v1/notifications/health") as Promise<NotificationView>;
}

export async function fetchOnboarding(settings: ApiSettings): Promise<OnboardingView> {
  if (!settings.baseUrl) {
    return {
      required: true,
      config_path: demoConfig.path,
      mode: demoConfig.mode,
      action: "notify only; never kill",
      mode_can_terminate: false,
      detected_providers: demoSnapshot.overview.sources.map((source) => source.provider),
      detected_workers: demoSnapshot.agents.map((agent) => agent.label),
      enforceable_agent_types: demoConfig.agents.filter((agent) => agent.terminates).length,
      watch_only_agent_types: demoConfig.agents.filter((agent) => !agent.terminates).length,
      notifications: demoNotifications,
      capabilities: demoSnapshot.overview.capabilities,
      sources: demoSnapshot.overview.sources,
      final_sentence: "Curb will notify on high-token turns.",
      steps: [],
    };
  }
  return getJSON(settings, "/v1/onboarding") as Promise<OnboardingView>;
}

export async function testNotification(settings: ApiSettings): Promise<NotificationView> {
  if (!settings.baseUrl) {
    return { ...demoNotifications, status: "delivered", message: "Demo notification delivered." };
  }
  try {
    return (await getJSON(settings, "/v1/notifications/test", { method: "POST" })) as NotificationView;
  } catch (error) {
    if (error instanceof ApiError && isNotificationView(error.body)) return error.body;
    throw error;
  }
}

export async function acknowledgeSession(
  settings: ApiSettings,
  sessionKey: string,
  extendSeconds: number,
): Promise<AckView> {
  if (!settings.baseUrl) {
    return {
      session_key: sessionKey,
      extend_seconds: extendSeconds,
      until: new Date(Date.now() + extendSeconds * 1000).toISOString(),
    };
  }
  return getJSON(settings, `/v1/sessions/${encodeURIComponent(sessionKey)}/ack`, {
    method: "POST",
    body: JSON.stringify({ extend_seconds: extendSeconds, reason: "Acknowledged in Curb" }),
  }) as Promise<AckView>;
}

export async function stopSession(
  settings: ApiSettings,
  sessionKey: string,
  expected: StopExpectedIdentity,
): Promise<StopView> {
  if (!settings.baseUrl) {
    return { session_key: sessionKey, pid: expected.pid, scope_pids: [expected.pid], result: { soft_signaled: [expected.pid] } };
  }
  return getJSON(settings, `/v1/sessions/${encodeURIComponent(sessionKey)}/stop`, {
    method: "POST",
    body: JSON.stringify({ confirm: true, scope: "tree", reason: "Manual stop from Curb", expected }),
  }) as Promise<StopView>;
}

class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly body: unknown,
  ) {
    super(message);
  }
}

async function getJSON(settings: ApiSettings, path: string, init: RequestInit = {}): Promise<unknown> {
  const headers: Record<string, string> = { ...(init.headers as Record<string, string>) };
  if (settings.token) headers.Authorization = `Bearer ${settings.token}`;
  if (init.body) headers["Content-Type"] = "application/json";
  const response = await fetch(`${settings.baseUrl.replace(/\/$/, "")}${path}`, { ...init, headers });
  if (!response.ok) {
    const detail = await response.text();
    throw new ApiError(
      `${response.status} ${response.statusText}${detail ? `: ${detail}` : ""}`,
      response.status,
      parseJSON(detail),
    );
  }
  return response.json();
}

function parseJSON(raw: string): unknown {
  if (!raw) return undefined;
  try {
    return JSON.parse(raw);
  } catch {
    return undefined;
  }
}

function isNotificationView(value: unknown): value is NotificationView {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<NotificationView>;
  return (
    typeof candidate.status === "string" &&
    typeof candidate.available === "boolean" &&
    typeof candidate.enabled === "boolean" &&
    typeof candidate.message === "string"
  );
}
