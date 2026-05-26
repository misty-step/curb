import { Activity, AlertTriangle, CircleDot, RefreshCw, Shield, SlidersHorizontal, SquareActivity } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import {
  acknowledgeSession,
  completeOnboarding,
  fetchAlerts,
  fetchConfig,
  fetchNotificationHealth,
  fetchOnboarding,
  fetchSessionTurns,
  fetchSnapshot,
  rescanService,
  saveConfig,
  stopSession,
  testNotification,
  type ApiSettings,
} from "./api";
import {
  AgentTable,
  AlertFeed,
  ConfigPanel,
  MetricStrip,
  numberValue,
  OnboardingPanel,
  OperatorSummary,
  Panel,
  PolicyStrip,
  ReadinessStrip,
  SessionDetail,
  SessionTable,
  StatusPill,
} from "./components/dashboard";
import { demoAlerts, demoConfig, demoSnapshot } from "./demo";
import type { AlertView, ConfigUpdate, ConfigView, NotificationView, OnboardingView, SessionView, Snapshot, TurnView } from "./types";

const sameOriginBaseUrl = window.location.protocol.startsWith("http") ? window.location.origin : "http://127.0.0.1:8765";
const savedBaseUrl = localStorage.getItem("curb.baseUrl") ?? sameOriginBaseUrl;
const savedToken = localStorage.getItem("curb.token") ?? "";

export function App() {
  const [settings, setSettings] = useState<ApiSettings>({ baseUrl: savedBaseUrl, token: savedToken });
  const [draft, setDraft] = useState<ApiSettings>(settings);
  const [snapshot, setSnapshot] = useState<Snapshot>(demoSnapshot);
  const [config, setConfig] = useState<ConfigView>(demoConfig);
  const [configDraft, setConfigDraft] = useState<ConfigView>(demoConfig);
  const [notifications, setNotifications] = useState<NotificationView>({
    enabled: demoConfig.local_notifications,
    available: true,
    status: "ready",
    message: "demo notifications are ready",
  });
  const [onboarding, setOnboarding] = useState<OnboardingView | null>(null);
  const [alerts, setAlerts] = useState<AlertView[]>(demoAlerts);
  const [selectedKey, setSelectedKey] = useState(demoSnapshot.sessions[0]?.key ?? "");
  const [selectedTurns, setSelectedTurns] = useState<TurnView[]>(demoSnapshot.turns);
  const [connection, setConnection] = useState<"demo" | "live" | "error">("demo");
  const [error, setError] = useState("");
  const [configMessage, setConfigMessage] = useState("");
  const [ackMessage, setAckMessage] = useState("");
  const [stopMessage, setStopMessage] = useState("");
  const [alertAckMessage, setAlertAckMessage] = useState("");

  const refresh = useCallback(async (next = settings, forceRescan = false) => {
    try {
      const data = forceRescan ? await rescanService(next) : await fetchSnapshot(next);
      const nextConfig = await fetchConfig(next);
      const nextAlerts = await fetchAlerts(next);
      const nextNotifications = await fetchNotificationHealth(next).catch((err) => ({
        enabled: false,
        available: false,
        status: "error",
        message: err instanceof Error ? err.message : "Unable to load notification health",
      }));
      const nextOnboarding = await fetchOnboarding(next).catch(() => null);
      setSnapshot(data);
      setConfig(nextConfig);
      setConfigDraft(nextConfig);
      setAlerts(nextAlerts);
      setNotifications(nextNotifications);
      setOnboarding(nextOnboarding);
      setConnection(next.baseUrl ? "live" : "demo");
      setError("");
      if (!data.sessions.some((session) => session.key === selectedKey)) {
        setSelectedKey(data.sessions[0]?.key ?? "");
      }
    } catch (err) {
      setConnection("error");
      setError(err instanceof Error ? err.message : "Unable to load Curb API");
    }
  }, [selectedKey, settings]);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 5000);
    return () => window.clearInterval(timer);
  }, [refresh, settings.baseUrl, settings.token]);

  useEffect(() => {
    if (!selectedKey) {
      setSelectedTurns([]);
      return;
    }
    let cancelled = false;
    setSelectedTurns([]);
    void fetchSessionTurns(settings, selectedKey)
      .then((turns) => {
        if (!cancelled) setSelectedTurns(turns);
      })
      .catch((err) => {
        if (!cancelled) {
          setSelectedTurns([]);
          setError(err instanceof Error ? err.message : "Unable to load session turns");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [settings, selectedKey]);

  const selected = snapshot.sessions.find((session) => session.key === selectedKey) ?? snapshot.sessions[0];

  function connect() {
    localStorage.setItem("curb.baseUrl", draft.baseUrl);
    localStorage.setItem("curb.token", draft.token);
    setSettings(draft);
    void refresh(draft);
  }

  async function persistConfig(update: ConfigUpdate) {
    try {
      const next = await saveConfig(settings, update);
      setConfig(next);
      setConfigDraft(next);
      setConfigMessage(settings.baseUrl ? "Saved" : "Demo only");
      await refresh();
    } catch (err) {
      setConfigMessage(err instanceof Error ? err.message : "Unable to save config");
    }
  }

  async function acknowledgeSessionKey(sessionKey: string, onMessage: (message: string) => void) {
    const extendSeconds = config.ack_extension_seconds || 1800;
    try {
      const ack = await acknowledgeSession(settings, sessionKey, extendSeconds);
      onMessage(`Acknowledged until ${new Date(ack.until).toLocaleTimeString()}`);
      await refresh();
    } catch (err) {
      onMessage(err instanceof Error ? err.message : "Unable to acknowledge session");
    }
  }

  async function acknowledgeSelected(session: SessionView) {
    await acknowledgeSessionKey(session.key, setAckMessage);
  }

  async function stopSelected(session: SessionView) {
    if (!session.correlated_pid || !session.correlated_process_started_at) return;
    try {
      const stopped = await stopSession(settings, session.key, {
        pid: session.correlated_pid,
        started_at: session.correlated_process_started_at,
        owner: session.correlated_owner,
        executable: session.correlated_executable,
        bundle_id: session.correlated_bundle_id,
        team_id: session.correlated_team_id,
      });
      setStopMessage(`Stop sent to pid ${stopped.pid}; ${stopped.scope_pids.length} process${stopped.scope_pids.length === 1 ? "" : "es"} in scope`);
      await refresh(settings, true);
    } catch (err) {
      setStopMessage(err instanceof Error ? err.message : "Unable to stop session");
      await refresh(settings, true);
    }
  }

  async function sendTestNotification() {
    try {
      const next = await testNotification(settings);
      setNotifications(next);
      setOnboarding((current) => (current ? { ...current, notifications: next } : current));
    } catch (err) {
      const next = {
        ...notifications,
        status: "error",
        message: err instanceof Error ? err.message : "Unable to test notifications",
      };
      setNotifications(next);
      setOnboarding((current) => (current ? { ...current, notifications: next } : current));
    }
  }

  async function finishOnboarding() {
    try {
      const next = await completeOnboarding(settings);
      setOnboarding(next);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unable to complete onboarding");
    }
  }

  return (
    <main className="app-shell">
      <header className="topbar">
        <div>
          <div className="brand">
            <Shield size={22} />
            <span>Curb</span>
          </div>
          <p>{snapshot.overview.message}</p>
        </div>
        <div className="top-actions">
          <StatusPill status={snapshot.overview.status} />
          <span className="mode">{snapshot.overview.mode}</span>
          <button type="button" className="icon-button" onClick={() => void refresh(settings, true)} aria-label="Refresh">
            <RefreshCw size={16} />
          </button>
        </div>
      </header>

      <OperatorSummary snapshot={snapshot} config={config} selectedKey={selected?.key} onSelect={setSelectedKey} />

      {snapshot.overview.status === "WATCH" || snapshot.overview.status === "ACTION" ? (
        <section className={`alert-banner ${snapshot.overview.status.toLowerCase()}`}>
          <AlertTriangle size={17} />
          <strong>{snapshot.overview.status === "ACTION" ? "Action threshold reached" : "Warning threshold reached"}</strong>
          <span>{snapshot.overview.message}</span>
        </section>
      ) : null}

      <details className="system-drawer">
        <summary>
          <CircleDot size={14} />
          Setup and health
        </summary>
        <ReadinessStrip snapshot={snapshot} notifications={notifications} connection={connection} error={error} />
        <section className="connect-row" aria-label="Curb API connection">
          <div className={`connection ${connection}`}>
            <CircleDot size={14} />
            <span>{connectionLabel(connection)}</span>
          </div>
          <p>{connectionHelp(settings, connection)}</p>
          <details className="advanced-connection">
            <summary>Advanced</summary>
            <div>
              <input
                aria-label="API base URL"
                value={draft.baseUrl}
                onChange={(event) => setDraft({ ...draft, baseUrl: event.target.value })}
              />
              <input
                aria-label="API token"
                value={draft.token}
                onChange={(event) => setDraft({ ...draft, token: event.target.value })}
                placeholder="optional api.token"
                type="password"
              />
              <button type="button" onClick={connect}>
                Connect
              </button>
            </div>
          </details>
          {error ? <span className="error-text">{error}</span> : null}
        </section>
        {onboarding?.required ? (
          <OnboardingPanel onboarding={onboarding} onTestNotification={() => void sendTestNotification()} onComplete={() => void finishOnboarding()} />
        ) : null}
        <PolicyStrip config={config} snapshot={snapshot} />
      </details>

      <details className="drilldown-drawer">
        <summary>
          <SquareActivity size={16} />
          Sessions, agents, and turn details
        </summary>
        <MetricStrip snapshot={snapshot} />
        <section className="dashboard-grid">
          <div className="tables">
            <Panel title="Sessions" icon={<SquareActivity size={16} />}>
              <SessionTable sessions={snapshot.sessions} config={config} selectedKey={selected?.key} onSelect={setSelectedKey} />
            </Panel>
            <Panel title="Agents" icon={<Activity size={16} />}>
              <AgentTable agents={snapshot.agents} />
            </Panel>
          </div>
          <div className="right-rail">
            <SessionDetail
              session={selected}
              turns={selectedTurns}
              config={config}
              ackSeconds={config.ack_extension_seconds}
              ackMessage={ackMessage}
              stopMessage={stopMessage}
              onAck={acknowledgeSelected}
              onStop={stopSelected}
            />
            <AlertFeed
              alerts={alerts}
              ackSeconds={config.ack_extension_seconds}
              ackMessage={alertAckMessage}
              onAck={(sessionKey) => acknowledgeSessionKey(sessionKey, setAlertAckMessage)}
            />
          </div>
        </section>
      </details>

      <details className="settings-drawer">
        <summary>
          <SlidersHorizontal size={16} />
          Policy settings
        </summary>
        <ConfigPanel
          config={configDraft}
          snapshot={snapshot}
          alerts={alerts}
          notifications={notifications}
          message={configMessage}
          onChange={setConfigDraft}
          onTestNotification={() => void sendTestNotification()}
          onSave={() =>
            persistConfig({
              mode: configDraft.mode,
              usage_enabled: configDraft.usage_enabled,
              warn_turn_tokens: numberValue(configDraft.warn_turn_tokens),
              kill_turn_tokens: numberValue(configDraft.kill_turn_tokens),
              usage_window_seconds: numberValue(configDraft.usage_window_seconds),
              usage_scan_seconds: numberValue(configDraft.usage_scan_seconds),
              process_warn_seconds: numberValue(configDraft.process_warn_seconds),
              process_kill_seconds: numberValue(configDraft.process_kill_seconds),
              local_notifications: configDraft.local_notifications,
            })
          }
        />
      </details>
    </main>
  );
}

function connectionLabel(connection: "demo" | "live" | "error"): string {
  if (connection === "live") return "Live API";
  if (connection === "error") return "Connection issue";
  return "Demo data";
}

function connectionHelp(settings: ApiSettings, connection: "demo" | "live" | "error"): string {
  if (settings.baseUrl === sameOriginBaseUrl && !settings.token) {
    return connection === "live"
      ? "Connected through the local dashboard cookie. No token paste is needed."
      : "Run curb app to start the local dashboard. It connects with a same-origin cookie automatically.";
  }
  if (!settings.baseUrl) return "Demo mode is illustrative only. Run curb app for live agent data.";
  return settings.token
    ? "Using an advanced bearer-token connection for a separate UI or API client."
    : "Using an advanced URL override. Same-origin cookie auth works only from the embedded dashboard.";
}
