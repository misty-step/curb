import { demoAlerts, demoConfig, demoSnapshot } from "./demo";
import type {
  AckView,
  AlertView,
  ConfigUpdate,
  ConfigView,
  NotificationView,
  OnboardingView,
  Snapshot,
  StopExpectedIdentity,
  StopView,
  TurnView,
} from "./types";

export interface ApiSettings {
  baseUrl: string;
  token: string;
}

export async function fetchSnapshot(settings: ApiSettings): Promise<Snapshot> {
  if (!settings.baseUrl) {
    return demoSnapshot;
  }
  const headers = authHeaders(settings);
  return getJSON(settings.baseUrl, "/v1/snapshot", { headers }) as Promise<Snapshot>;
}

export async function rescanService(settings: ApiSettings): Promise<Snapshot> {
  if (!settings.baseUrl) {
    return demoSnapshot;
  }
  const headers = authHeaders(settings);
  return getJSON(settings.baseUrl, "/v1/service/rescan", { method: "POST", headers }) as Promise<Snapshot>;
}

export async function fetchConfig(settings: ApiSettings): Promise<ConfigView> {
  if (!settings.baseUrl) {
    return demoConfig;
  }
  const headers = authHeaders(settings);
  return getJSON(settings.baseUrl, "/v1/config", { headers }) as Promise<ConfigView>;
}

export async function saveConfig(settings: ApiSettings, update: ConfigUpdate): Promise<ConfigView> {
  if (!settings.baseUrl) {
    return { ...demoConfig, ...update };
  }
  const headers = { ...authHeaders(settings), "Content-Type": "application/json" };
  return getJSON(settings.baseUrl, "/v1/config", {
    method: "PUT",
    headers,
    body: JSON.stringify(update),
  }) as Promise<ConfigView>;
}

export async function fetchNotificationHealth(settings: ApiSettings): Promise<NotificationView> {
  if (!settings.baseUrl) {
    return {
      enabled: demoConfig.local_notifications,
      available: true,
      status: demoConfig.local_notifications ? "ready" : "disabled",
      message: demoConfig.local_notifications ? "demo notifications are ready" : "local notifications are disabled",
    };
  }
  const headers = authHeaders(settings);
  return getJSON(settings.baseUrl, "/v1/notifications/health", { headers }) as Promise<NotificationView>;
}

export async function testNotification(settings: ApiSettings): Promise<NotificationView> {
  if (!settings.baseUrl) {
    return {
      enabled: demoConfig.local_notifications,
      available: true,
      status: "delivered",
      message: "demo notification delivered",
      last_test_at: new Date().toISOString(),
    };
  }
  const headers = authHeaders(settings);
  try {
    return await getJSON(settings.baseUrl, "/v1/notifications/test", { method: "POST", headers }) as NotificationView;
  } catch (err) {
    if (err instanceof ApiError && isNotificationView(err.body)) {
      return err.body;
    }
    throw err;
  }
}

export async function fetchOnboarding(settings: ApiSettings): Promise<OnboardingView> {
  if (!settings.baseUrl) {
    return demoOnboarding();
  }
  const headers = authHeaders(settings);
  return getJSON(settings.baseUrl, "/v1/onboarding", { headers }) as Promise<OnboardingView>;
}

export async function completeOnboarding(settings: ApiSettings): Promise<OnboardingView> {
  if (!settings.baseUrl) {
    return { ...demoOnboarding(), required: false };
  }
  const headers = authHeaders(settings);
  return getJSON(settings.baseUrl, "/v1/onboarding/complete", { method: "POST", headers }) as Promise<OnboardingView>;
}

export async function fetchAlerts(settings: ApiSettings, limit = 25): Promise<AlertView[]> {
  if (!settings.baseUrl) {
    return demoAlerts;
  }
  const headers = authHeaders(settings);
  return getJSON(settings.baseUrl, `/v1/alerts?limit=${limit}`, { headers }) as Promise<AlertView[]>;
}

export async function fetchSessionTurns(settings: ApiSettings, sessionKey: string, limit = 200): Promise<TurnView[]> {
  if (!settings.baseUrl) {
    return demoSnapshot.turns.filter((turn) => turn.session_key === sessionKey || turn.session_id === sessionKey);
  }
  const headers = authHeaders(settings);
  return getJSON(settings.baseUrl, `/v1/sessions/${encodeURIComponent(sessionKey)}/turns?limit=${limit}`, {
    headers,
  }) as Promise<TurnView[]>;
}

export async function acknowledgeSession(settings: ApiSettings, sessionKey: string, extendSeconds: number): Promise<AckView> {
  if (!settings.baseUrl) {
    return {
      session_key: sessionKey,
      extend_seconds: extendSeconds,
      until: new Date(Date.now() + extendSeconds * 1000).toISOString(),
      reason: "Demo acknowledgement",
    };
  }
  const headers = { ...authHeaders(settings), "Content-Type": "application/json" };
  return getJSON(settings.baseUrl, `/v1/sessions/${encodeURIComponent(sessionKey)}/ack`, {
    method: "POST",
    headers,
    body: JSON.stringify({ extend_seconds: extendSeconds, reason: "Acknowledged in Curb UI" }),
  }) as Promise<AckView>;
}

export async function stopSession(settings: ApiSettings, sessionKey: string, expected: StopExpectedIdentity): Promise<StopView> {
  if (!settings.baseUrl) {
    return {
      session_key: sessionKey,
      agent_id: "demo",
      pid: expected.pid,
      started_at: expected.started_at,
      owner: expected.owner,
      executable: expected.executable,
      bundle_id: expected.bundle_id,
      team_id: expected.team_id,
      scope: "tree",
      scope_pids: [expected.pid],
      result: { soft_signaled: [expected.pid] },
    };
  }
  const headers = { ...authHeaders(settings), "Content-Type": "application/json" };
  return getJSON(settings.baseUrl, `/v1/sessions/${encodeURIComponent(sessionKey)}/stop`, {
    method: "POST",
    headers,
    body: JSON.stringify({ confirm: true, scope: "tree", reason: "Manual stop from Curb UI", expected }),
  }) as Promise<StopView>;
}

function authHeaders(settings: ApiSettings): Record<string, string> {
  if (!settings.token) return {};
  return { Authorization: `Bearer ${settings.token}` };
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

async function getJSON(baseUrl: string, path: string, init: RequestInit) {
  const response = await fetch(`${baseUrl.replace(/\/$/, "")}${path}`, init);
  if (!response.ok) {
    const detail = await response.text();
    throw new ApiError(`${response.status} ${response.statusText}${detail ? `: ${detail}` : ""}`, response.status, parseJSON(detail));
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
  return typeof candidate.enabled === "boolean" && typeof candidate.available === "boolean" && typeof candidate.status === "string";
}

function demoOnboarding(): OnboardingView {
  return {
    required: true,
    config_path: demoConfig.path,
    mode: demoConfig.mode,
    action: "notify only; never kill",
    mode_can_terminate: false,
    detected_providers: ["codex", "claude"],
    detected_workers: ["Codex Desktop Worker", "Claude Code"],
    enforceable_agent_types: demoConfig.agents.filter((agent) => agent.terminates).length,
    watch_only_agent_types: demoConfig.agents.filter((agent) => !agent.terminates).length,
    notifications: {
      enabled: demoConfig.local_notifications,
      available: true,
      status: "ready",
      message: "demo notifications are ready",
    },
    capabilities: demoSnapshot.overview.capabilities,
    sources: demoSnapshot.overview.sources,
    final_sentence: "Curb will notify on high-token turns. It will not stop any process in Alert mode. Desktop app roots are watch-only.",
    steps: [
      { id: "config", label: "Config", status: "done", message: `using ${demoConfig.path}` },
      { id: "agents", label: "Agents", status: "done", message: "2 enforceable agents, 1 watch-only agent" },
      { id: "notifications", label: "Notifications", status: "done", message: "demo notifications are ready" },
      { id: "safety", label: "Safety", status: "done", message: "desktop app roots are watch-only" },
    ],
  };
}
