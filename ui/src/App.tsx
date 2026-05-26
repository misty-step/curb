import { Activity, AlertTriangle, CheckCircle2, CircleDot, RefreshCw, Save, Shield, SlidersHorizontal, SquareActivity } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
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
import { demoAlerts, demoConfig, demoSnapshot } from "./demo";
import { formatDuration, formatTokens, relativeTime, stateLabel, statusTone } from "./format";
import type { AgentView, AlertView, ConfigUpdate, ConfigView, NotificationView, OnboardingView, SessionView, Snapshot, TurnView } from "./types";

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

function ReadinessStrip({
  snapshot,
  notifications,
  connection,
  error,
}: {
  snapshot: Snapshot;
  notifications: NotificationView;
  connection: "demo" | "live" | "error";
  error: string;
}) {
  const caps = snapshot.overview.capabilities;
  const items =
    connection === "live"
      ? [
          { label: "Data", state: "ready", value: connectionLabel(connection) },
          { label: "Notifications", state: notifications.status, value: notifications.message },
          { label: "Sources", state: sourceState(snapshot.overview.sources), value: sourceLabel(snapshot.overview.sources) },
          { label: "Platform", state: caps.process_capture.status, value: caps.process_capture.message },
          { label: "Identity", state: caps.process_identity.status, value: caps.process_identity.message },
          { label: "Enforcement", state: caps.enforcement.status, value: caps.enforcement.message },
          { label: "Last scan", state: "ready", value: relativeTime(snapshot.overview.last_scan) },
        ]
      : staleReadinessItems(connection);
  return (
    <section className="readiness-strip" aria-label="Dashboard readiness">
      <div className="readiness-summary">
        <StateChip state={connectionState(connection)} />
        <strong>{safetySentence(snapshot, connection, error)}</strong>
      </div>
      <div className="readiness-grid">
        {items.map((item) => (
          <div className="readiness-item" key={item.label}>
            <span>{item.label}</span>
            <StateChip state={item.state || "unknown"} />
            <strong>{item.value}</strong>
          </div>
        ))}
      </div>
    </section>
  );
}

function staleReadinessItems(connection: "demo" | "live" | "error") {
  const state = connectionState(connection);
  const qualifier = connection === "demo" ? "demo only" : "not current";
  return [
    { label: "Data", state, value: connectionLabel(connection) },
    { label: "Notifications", state, value: qualifier },
    { label: "Sources", state, value: qualifier },
    { label: "Platform", state, value: qualifier },
    { label: "Identity", state, value: qualifier },
    { label: "Enforcement", state, value: connection === "demo" ? "inactive in demo" : "not active" },
    { label: "Last scan", state, value: qualifier },
  ];
}

function connectionState(connection: "demo" | "live" | "error"): string {
  if (connection === "live") return "ready";
  if (connection === "error") return "error";
  return "demo";
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

function safetySentence(snapshot: Snapshot, connection: "demo" | "live" | "error", error: string): string {
  if (connection === "demo") return "Demo data is illustrative; connect to the live daemon before relying on Curb.";
  if (connection === "error") return error || "Curb cannot reach the daemon, so this dashboard is not current.";
  return `${snapshot.overview.action}. ${snapshot.overview.message}`;
}

function sourceState(sources: Snapshot["overview"]["sources"]): string {
  if (sources.some((source) => source.error)) return "error";
  if (sources.length === 0) return "unknown";
  return "ready";
}

function sourceLabel(sources: Snapshot["overview"]["sources"]): string {
  if (sources.length === 0) return "no usage sources";
  const errors = sources.filter((source) => source.error).length;
  const events = sources.reduce((sum, source) => sum + source.events, 0);
  const files = sources.reduce((sum, source) => sum + source.files, 0);
  if (errors > 0) return `${errors} source error${errors === 1 ? "" : "s"} · ${formatTokens(events)} events`;
  return `${sources.length} source${sources.length === 1 ? "" : "s"} · ${formatTokens(events)} events · ${files} files`;
}

function PolicyStrip({ config, snapshot }: { config: ConfigView; snapshot: Snapshot }) {
  const sources = snapshot.overview.sources
    .map((source) => `${source.provider}: ${source.events} events`)
    .join("  ");
  return (
    <section className="policy-strip">
      <div>
        <span>Mode</span>
        <strong>{config.mode}</strong>
      </div>
      <div>
        <span>Turn policy</span>
        <strong>
          warn {formatTokens(config.warn_turn_tokens)} · stop {formatTokens(config.kill_turn_tokens)}
        </strong>
      </div>
      <div>
        <span>Window</span>
        <strong>{formatDuration(config.usage_window_seconds)}</strong>
      </div>
      <div>
        <span>Sources</span>
        <strong>{sources || "none"}</strong>
      </div>
      <div>
        <span>Last scan</span>
        <strong>{relativeTime(snapshot.overview.last_scan)}</strong>
      </div>
    </section>
  );
}

function OnboardingPanel({
  onboarding,
  onTestNotification,
  onComplete,
}: {
  onboarding: OnboardingView;
  onTestNotification: () => void;
  onComplete: () => void;
}) {
  return (
    <section className="onboarding-panel" aria-label="Curb onboarding">
      <div className="onboarding-heading">
        <div>
          <span>First run</span>
          <h2>Start watching safely</h2>
          <p>{onboarding.final_sentence}</p>
        </div>
        <div className="onboarding-actions">
          <button
            type="button"
            className="secondary-button"
            onClick={onTestNotification}
            disabled={!onboarding.notifications.enabled || !onboarding.notifications.available}
          >
            Test notification
          </button>
          <button type="button" onClick={onComplete}>Done</button>
        </div>
      </div>
      <div className="onboarding-summary">
        <MiniStat label="Mode" value={onboarding.mode} />
        <MiniStat label="Current mode stops" value={onboarding.mode_can_terminate ? "yes" : "no"} />
        <MiniStat label="Enforceable" value={`${onboarding.enforceable_agent_types}`} />
        <MiniStat label="Watch-only" value={`${onboarding.watch_only_agent_types}`} />
        <MiniStat label="Providers" value={listLabel(onboarding.detected_providers)} />
        <MiniStat label="Workers" value={listLabel(onboarding.detected_workers)} />
        <MiniStat label="Sources" value={sourceSummary(onboarding.sources)} />
        <MiniStat label="Notifications" value={onboarding.notifications.status} />
      </div>
      <p className={`onboarding-notification ${cssState(onboarding.notifications.status)}`}>{onboarding.notifications.message}</p>
      <div className="capability-list" aria-label="Platform capabilities">
        <CapabilityLine label="Platform" capability={{ status: "ready", message: onboarding.capabilities.platform }} />
        <CapabilityLine label="Process scan" capability={onboarding.capabilities.process_capture} />
        <CapabilityLine label="Identity evidence" capability={onboarding.capabilities.process_identity} />
        <CapabilityLine label="Enforcement" capability={onboarding.capabilities.enforcement} />
      </div>
      <p className="onboarding-path">Config: {onboarding.config_path || "default user config"}</p>
      <p className="onboarding-consequence">{onboardingConsequence(onboarding)}</p>
      <div className="onboarding-steps">
        {onboarding.steps.map((step) => (
          <div className={`onboarding-step ${step.status}`} key={step.id}>
            <StateChip state={step.status} />
            <strong>{step.label}</strong>
            <span>{step.message}</span>
          </div>
        ))}
      </div>
    </section>
  );
}

function CapabilityLine({ label, capability }: { label: string; capability: { status: string; message: string } }) {
  return (
    <div className="capability-line">
      <StateChip state={capability.status} />
      <strong>{label}</strong>
      <span>{capability.message}</span>
    </div>
  );
}

function onboardingConsequence(onboarding: OnboardingView): string {
  if (onboarding.capabilities.enforcement.available) {
    return "Done completes setup; Curb continues with enforcement available for revalidated worker processes.";
  }
  return `Done continues in ${onboarding.mode} mode with enforcement unavailable: ${onboarding.capabilities.enforcement.message}.`;
}

function ConfigPanel({
  config,
  snapshot,
  alerts,
  notifications,
  message,
  onChange,
  onTestNotification,
  onSave,
}: {
  config: ConfigView;
  snapshot: Snapshot;
  alerts: AlertView[];
  notifications: NotificationView;
  message: string;
  onChange: (config: ConfigView) => void;
  onTestNotification: () => void;
  onSave: () => void;
}) {
  const [tab, setTab] = useState("mode");
  const [enforcementConfirmed, setEnforcementConfirmed] = useState(config.mode !== "enforcement");
  const enforceable = config.agents.filter((agent) => agent.terminates);
  const watchOnly = config.agents.filter((agent) => !agent.terminates);
  const liveEnforceable = snapshot.agents.filter((agent) => enforceable.some((candidate) => candidate.id === agent.id) && agent.pid > 0);
  const liveActionable = liveEnforceable.filter((agent) => agent.actionable);
  const dryRun = [...alerts].reverse().find((alert) => alert.category === "would_stop" || alert.label === "would stop");
  const saveDisabled = config.mode === "enforcement" && !enforcementConfirmed;

  function updateConfig(next: ConfigView) {
    setEnforcementConfirmed(next.mode !== "enforcement");
    onChange(next);
  }

  function selectMode(mode: string) {
    updateConfig({ ...config, mode });
  }

  return (
    <div className="config-panel">
      <div className="config-tabs" role="tablist" aria-label="Policy settings sections">
        {["mode", "usage", "agents", "notifications"].map((name) => (
          <button
            type="button"
            role="tab"
            aria-selected={tab === name}
            className={tab === name ? "selected" : ""}
            key={name}
            onClick={() => setTab(name)}
          >
            {name}
          </button>
        ))}
      </div>

      {tab === "mode" ? (
        <section className="config-section" aria-label="Mode settings">
          <div className="mode-options">
            <button type="button" className={config.mode === "visibility" ? "selected" : ""} onClick={() => selectMode("visibility")}>
              <strong>Observe</strong>
              <span>Record only. No notifications or stops from policy thresholds.</span>
            </button>
            <button type="button" className={config.mode === "alert" ? "selected" : ""} onClick={() => selectMode("alert")}>
              <strong>Alert</strong>
              <span>Notify only. Never stop a process.</span>
            </button>
            <button type="button" className={config.mode === "enforcement" ? "selected" : ""} onClick={() => selectMode("enforcement")}>
              <strong>Enforce</strong>
              <span>Warn, wait for grace, then stop revalidated correlated workers.</span>
            </button>
          </div>
          {config.mode === "enforcement" ? (
            <div className="enforcement-confirmation">
              <strong>Enforcement confirmation</strong>
              <p>
                Stop threshold {formatTokens(config.kill_turn_tokens)} per turn · warning {formatTokens(config.warn_turn_tokens)} ·
                process grace {formatDuration(config.process_kill_seconds - config.process_warn_seconds)} · extension {formatDuration(config.ack_extension_seconds)}
              </p>
              <p>
                {liveActionable.length} live actionable worker{liveActionable.length === 1 ? "" : "s"} · {liveEnforceable.length} live enforceable worker{liveEnforceable.length === 1 ? "" : "s"} ·{" "}
                {watchOnly.length} watch-only app root{watchOnly.length === 1 ? "" : "s"}
              </p>
              <p>
                Capability: {snapshot.overview.capabilities.enforcement.message}. Last alert-mode dry run {dryRun ? relativeTime(dryRun.at) : "not seen"}.
              </p>
              <label className="check-row">
                <input
                  type="checkbox"
                  checked={enforcementConfirmed}
                  onChange={(event) => setEnforcementConfirmed(event.target.checked)}
                />
                <span>Curb stops only revalidated worker processes, never watch-only desktop app roots.</span>
              </label>
            </div>
          ) : null}
        </section>
      ) : null}

      {tab === "usage" ? (
        <section className="config-section config-grid" aria-label="Usage settings">
          <label>
            <span>Warn turn tokens</span>
            <input
              aria-label="Warn turn tokens"
              type="number"
              min="1"
              step="1000"
              value={config.warn_turn_tokens}
              onChange={(event) => updateConfig({ ...config, warn_turn_tokens: Number(event.target.value) })}
            />
          </label>
          <label>
            <span>Stop turn tokens</span>
            <input
              aria-label="Stop turn tokens"
              type="number"
              min="1"
              step="1000"
              value={config.kill_turn_tokens}
              onChange={(event) => updateConfig({ ...config, kill_turn_tokens: Number(event.target.value) })}
            />
          </label>
          <label>
            <span>Window seconds</span>
            <input
              aria-label="Window seconds"
              type="number"
              min="1"
              value={config.usage_window_seconds}
              onChange={(event) => updateConfig({ ...config, usage_window_seconds: Number(event.target.value) })}
            />
          </label>
          <label>
            <span>Scan seconds</span>
            <input
              aria-label="Scan seconds"
              type="number"
              min="1"
              value={config.usage_scan_seconds}
              onChange={(event) => updateConfig({ ...config, usage_scan_seconds: Number(event.target.value) })}
            />
          </label>
          <label>
            <span>Process warn seconds</span>
            <input
              aria-label="Process warn seconds"
              type="number"
              min="1"
              value={config.process_warn_seconds}
              onChange={(event) => updateConfig({ ...config, process_warn_seconds: Number(event.target.value) })}
            />
          </label>
          <label>
            <span>Process stop seconds</span>
            <input
              aria-label="Process stop seconds"
              type="number"
              min="1"
              value={config.process_kill_seconds}
              onChange={(event) => updateConfig({ ...config, process_kill_seconds: Number(event.target.value) })}
            />
          </label>
        </section>
      ) : null}

      {tab === "agents" ? (
        <section className="config-section agent-config-list" aria-label="Agent settings">
          {config.agents.map((agent) => (
            <div className="agent-config-row" key={agent.id}>
              <StateChip state={agent.terminates ? "ready" : "watch-only"} />
              <strong>{agent.label || agent.id}</strong>
              <span>{agent.terminates ? "enforceable worker process" : "watch-only app root"}</span>
              <span>{agent.description}</span>
            </div>
          ))}
        </section>
      ) : null}

      {tab === "notifications" ? (
        <section className="config-section notification-config" aria-label="Notification settings">
          <CapabilityLine label="Local notifications" capability={{ status: notifications.status, message: notificationLabel(notifications) }} />
          <label className="check-row">
            <input
              type="checkbox"
              checked={config.local_notifications}
              onChange={(event) => updateConfig({ ...config, local_notifications: event.target.checked })}
            />
            <span>Local notifications</span>
          </label>
          <button type="button" className="secondary-button" onClick={onTestNotification} disabled={!config.local_notifications}>
            Test notification
          </button>
        </section>
      ) : null}

      <div className="config-footer">
        <span>{config.agents.filter((agent) => agent.terminates).length} enforceable agent types</span>
        <span className={`notification-status ${notifications.status}`}>{notificationLabel(notifications)}</span>
        <button type="button" className="secondary-button" onClick={onTestNotification} disabled={!config.local_notifications}>
          Test notification
        </button>
        <button type="button" onClick={onSave} disabled={saveDisabled}>
          <Save size={15} />
          Save policy
        </button>
        {message ? <span className="config-message">{message}</span> : null}
      </div>
    </div>
  );
}

function notificationLabel(view: NotificationView) {
  const tested = view.last_test_at ? ` · tested ${relativeTime(view.last_test_at)}` : "";
  return `${view.status}: ${view.message}${tested}`;
}

function OperatorSummary({
  snapshot,
  config,
  selectedKey,
  onSelect,
}: {
  snapshot: Snapshot;
  config: ConfigView;
  selectedKey?: string;
  onSelect: (key: string) => void;
}) {
  const aliveAgents = snapshot.agents.filter(isAliveAgent);
  const spendingAgents = aliveAgents.filter(isSpendingAgent);
  const recentUncorrelated = snapshot.sessions.filter(isRecentUncorrelatedUsage);
  const latestInputTokens = spendingAgents.reduce((sum, agent) => sum + (agent.latest_turn_tokens ?? 0), 0);
  const recentUncorrelatedTokens = recentUncorrelated.reduce((sum, session) => sum + (session.window_tokens ?? 0), 0);
  const aliveRows = aliveAgentGroups(aliveAgents).slice(0, 6);
  const spendingRows = spendingAgents.slice(0, 5);
  const headline =
    spendingAgents.length > 0
      ? `${spendingAgents.length} agent${spendingAgents.length === 1 ? "" : "s"} actively consuming tokens`
      : "No agents are actively consuming tokens";
  const subline =
    spendingAgents.length > 0
      ? `${formatTokens(latestInputTokens)} since latest user input across active sessions`
      : recentUncorrelated.length > 0
        ? `${formatTokens(recentUncorrelatedTokens)} recent tokens are uncorrelated to a live worker`
        : `${aliveAgents.length} worker process${aliveAgents.length === 1 ? "" : "es"} are alive but idle`;

  return (
    <section className="operator-summary" aria-label="Agent activity summary">
      <div className="operator-headline">
        <div>
          <span>Right now</span>
          <h2>{headline}</h2>
          <p>{subline}</p>
        </div>
        <div className="operator-stats">
          <MiniStat label="Active runs" value={`${spendingAgents.length}`} />
          <MiniStat label="Alive workers" value={`${aliveAgents.length}`} />
          <MiniStat label="Tokens this turn" value={formatTokens(latestInputTokens)} />
          <MiniStat label="Unmatched logs" value={formatTokens(recentUncorrelatedTokens)} />
          <MiniStat label="Policy" value={summaryPolicy(config)} />
        </div>
      </div>
      <div className="operator-list" aria-label="Current agent runs">
        {spendingRows.length > 0 ? (
          spendingRows.map((agent) => {
            const session = sessionForAgent(agent, snapshot.sessions);
            return (
            <button
              type="button"
              className={`operator-row ${session?.key === selectedKey ? "selected" : ""}`}
              key={`${agent.id}-${agent.pid}`}
              onClick={() => session ? onSelect(session.key) : undefined}
            >
              <span className="operator-state">
                <StateChip state={agent.usage_state || "spending"} />
              </span>
              <span className="operator-main">
                <strong>{agent.project || agent.label}</strong>
                <span>{agent.provider} · {agent.label}</span>
              </span>
              <span>
                <strong>{formatTokens(agent.latest_turn_tokens ?? 0)}</strong>
                <span>since input</span>
              </span>
              <span>
                <strong>{formatTokens(agent.window_tokens ?? 0)}</strong>
                <span>current window</span>
              </span>
              <span>
                <strong>{formatDuration(agent.running_for_seconds)}</strong>
                <span>running</span>
              </span>
            </button>
            );
          })
        ) : aliveRows.length > 0 ? (
          aliveRows.map((group) => (
            <div className="operator-row passive" key={`${group.id}-${group.project}-${group.cwd}`}>
              <span className="operator-state">
                <StateChip state="idle" />
              </span>
              <span className="operator-main">
                <strong>{group.project || group.label}</strong>
                <span>{group.provider} · {group.label}</span>
              </span>
              <span>
                <strong>{group.count}</strong>
                <span>worker process{group.count === 1 ? "" : "es"}</span>
              </span>
              <span>
                <strong>{formatDuration(group.runningForSeconds)}</strong>
                <span>alive</span>
              </span>
              <span>
                <strong>idle</strong>
                <span>no matched token use</span>
              </span>
            </div>
          ))
        ) : (
          <div className="operator-empty">
            <strong>No live agent run is correlated to usage yet.</strong>
            <span>{aliveAgents.length > 0 ? aliveAgentSummary(aliveAgents) : "No watched worker processes are alive."}</span>
          </div>
        )}
      </div>
    </section>
  );
}

function isAliveAgent(agent: AgentView): boolean {
  return agent.state !== "ended" && agent.pid > 0;
}

function isSpendingAgent(agent: AgentView): boolean {
  return agent.state === "spending" || agent.state === "warn" || agent.state === "stop";
}

function isRecentUncorrelatedUsage(session: SessionView): boolean {
  return session.state === "uncorrelated" || (session.correlated_pid === undefined && (session.window_tokens ?? 0) > 0);
}

function sessionForAgent(agent: AgentView, sessions: SessionView[]): SessionView | undefined {
  return sessions.find((session) => session.correlated_pid === agent.pid && session.id === agent.latest_session_id) ??
    sessions.find((session) => session.correlated_pid === agent.pid);
}

function aliveAgentSummary(agents: AgentView[]): string {
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

interface AliveAgentGroup {
  id: string;
  provider: string;
  label: string;
  project: string;
  cwd: string;
  count: number;
  runningForSeconds: number;
  latestStarted: number;
}

function aliveAgentGroups(agents: AgentView[]): AliveAgentGroup[] {
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

function summaryPolicy(config: ConfigView): string {
  if (config.mode === "enforcement") return `stop over ${formatTokens(config.kill_turn_tokens)}`;
  if (config.mode === "alert") return "notify only";
  return "observe";
}

function MetricStrip({ snapshot }: { snapshot: Snapshot }) {
  const metrics = [
    ["Window tokens", formatTokens(snapshot.overview.window_tokens)],
    ["Since last scan", changeSummary(snapshot.overview.changes)],
    ["Active sessions", `${snapshot.overview.active_sessions}`],
    ["Warnings / stop", `${snapshot.overview.warning_sessions} / ${snapshot.overview.stop_sessions}`],
    ["Policy action", snapshot.overview.action],
  ];
  return (
    <section className="metric-strip">
      {metrics.map(([label, value]) => (
        <div className="metric" key={label}>
          <span>{label}</span>
          <strong>{value}</strong>
        </div>
      ))}
    </section>
  );
}

function changeSummary(changes: Snapshot["overview"]["changes"]) {
  const parts = [];
  if (changes.tokens_added > 0) {
    parts.push(`+${formatTokens(changes.tokens_added)}`);
  }
  if (changes.sessions_with_new_turns > 0) {
    parts.push(`${changes.sessions_with_new_turns} active`);
  }
  if (changes.new_sessions > 0) {
    parts.push(`${changes.new_sessions} new`);
  }
  if (changes.new_alerts > 0) {
    parts.push(`${changes.new_alerts} alert`);
  }
  if (changes.agents_started > 0 || changes.agents_ended > 0) {
    parts.push(`${changes.agents_started} started / ${changes.agents_ended} ended`);
  }
  if (changes.source_errors > 0) {
    parts.push(`${changes.source_errors} source error`);
  }
  return parts.length ? parts.join(" · ") : "no change";
}

function Panel({ title, icon, children }: { title: string; icon: React.ReactNode; children: React.ReactNode }) {
  return (
    <section className="panel">
      <div className="panel-title">
        {icon}
        <h2>{title}</h2>
      </div>
      {children}
    </section>
  );
}

function AgentTable({ agents }: { agents: AgentView[] }) {
  return (
    <div className="table-wrap">
      <table>
        <thead>
          <tr>
            <th>Process</th>
            <th>Usage</th>
            <th>Agent</th>
            <th>Project</th>
            <th>Running</th>
            <th>Latest</th>
            <th>PID</th>
            <th>Why</th>
          </tr>
        </thead>
        <tbody>
          {agents.map((agent, index) => (
            <tr key={`${agent.id}-${agent.pid}-${index}`}>
              <td data-label="Process">
                <StateChip state={agent.process_state} />
              </td>
              <td data-label="Usage">
                <StateChip state={agent.usage_state || "quiet"} />
              </td>
              <td data-label="Agent">
                <strong>{agent.label || agent.id}</strong>
                <span>{agent.provider}</span>
              </td>
              <td data-label="Project">{agent.project || "-"}</td>
              <td data-label="Running">{formatDuration(agent.running_for_seconds)}</td>
              <td data-label="Latest">{agent.latest_turn_tokens ? formatTokens(agent.latest_turn_tokens) : "-"}</td>
              <td data-label="PID">{agent.pid || "-"}</td>
              <td data-label="Why">{agent.explanation}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function SessionTable({
  sessions,
  config,
  selectedKey,
  onSelect,
}: {
  sessions: SessionView[];
  config: ConfigView;
  selectedKey?: string;
  onSelect: (key: string) => void;
}) {
  return (
    <div className="table-wrap">
      <table className="sessions-table">
        <thead>
          <tr>
            <th>Process</th>
            <th>Usage</th>
            <th>Spend</th>
            <th>Provider</th>
            <th>Model</th>
            <th>Project</th>
            <th>Last</th>
            <th>Total</th>
            <th>Action</th>
          </tr>
        </thead>
        <tbody>
          {sessions.map((session) => (
            <tr
              key={session.key}
              className={session.key === selectedKey ? "selected" : ""}
              onClick={() => onSelect(session.key)}
            >
              <td data-label="Process">
                <StateChip state={session.process_state} />
              </td>
              <td data-label="Usage">
                <StateChip state={session.usage_state || "quiet"} />
              </td>
              <td data-label="Spend">
                <SpendMeter
                  latest={session.latest_turn_tokens ?? 0}
                  window={session.window_tokens ?? 0}
                  warn={config.warn_turn_tokens}
                  stop={config.kill_turn_tokens}
                />
              </td>
              <td data-label="Provider">{session.provider}</td>
              <td data-label="Model">
                <ModelChips models={session.models} />
              </td>
              <td data-label="Project">{session.project || "-"}</td>
              <td data-label="Last">{relativeTime(session.last_usage_at ?? session.last_seen_at)}</td>
              <td data-label="Total">{formatTokens(session.total_tokens)}</td>
              <td data-label="Action">
                <strong>{session.action_state}</strong>
                <span>{session.explanation}</span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function SessionDetail({
  session,
  turns,
  config,
  ackSeconds,
  ackMessage,
  onAck,
  stopMessage,
  onStop,
}: {
  session?: SessionView;
  turns: TurnView[];
  config: ConfigView;
  ackSeconds: number;
  ackMessage: string;
  onAck: (session: SessionView) => void;
  stopMessage: string;
  onStop: (session: SessionView) => void;
}) {
  const [selectedTurnKey, setSelectedTurnKey] = useState("");
  const [confirmStop, setConfirmStop] = useState(false);
  useEffect(() => {
    if (turns.length === 0) {
      if (selectedTurnKey !== "") setSelectedTurnKey("");
      return;
    }
    if (!turns.some((turn) => turnKey(turn) === selectedTurnKey)) {
      setSelectedTurnKey(turnKey(turns[0]));
    }
  }, [turns, selectedTurnKey]);
  if (!session) {
    return (
      <aside className="detail">
        <h2>No session selected</h2>
      </aside>
    );
  }
  return (
    <aside className="detail">
      <div className="detail-heading">
        <div className="chip-row">
          <StateChip state={session.process_state} />
          <StateChip state={session.usage_state || "quiet"} />
          <StateChip state={session.action_state} />
        </div>
        <h2>{session.project || session.provider}</h2>
        <p>{session.key}</p>
      </div>
      {canAcknowledge(session) ? (
        <div className="session-action">
          <button type="button" onClick={() => onAck(session)}>
            <CheckCircle2 size={15} />
            Extend {formatDuration(ackSeconds || 1800)}
          </button>
          {ackMessage ? <span>{ackMessage}</span> : null}
        </div>
      ) : session.acknowledged_until ? (
        <p className="ack-note">Acknowledged until {new Date(session.acknowledged_until).toLocaleTimeString()}</p>
      ) : null}
      {canStop(session) ? (
        <div className="session-action destructive">
          <button type="button" onClick={() => setConfirmStop((open) => !open)}>
            <AlertTriangle size={15} />
            Stop now
          </button>
          {stopMessage ? <span>{stopMessage}</span> : null}
          {confirmStop ? (
            <div className="stop-confirmation" role="dialog" aria-label="Confirm session stop">
              <strong>Stop correlated process tree</strong>
              <dl>
                <dt>PID</dt>
                <dd>{session.correlated_pid}</dd>
                <dt>Started</dt>
                <dd>{session.correlated_process_started_at ? new Date(session.correlated_process_started_at).toLocaleString() : "-"}</dd>
                <dt>Owner</dt>
                <dd>{session.correlated_owner || "-"}</dd>
                <dt>Executable</dt>
                <dd>{session.correlated_executable || "-"}</dd>
                <dt>Bundle ID</dt>
                <dd>{session.correlated_bundle_id || "-"}</dd>
                <dt>Team ID</dt>
                <dd>{session.correlated_team_id || "-"}</dd>
                <dt>Scope</dt>
                <dd>process tree</dd>
              </dl>
              <button type="button" className="danger-button" onClick={() => onStop(session)}>
                Stop process tree
              </button>
            </div>
          ) : null}
        </div>
      ) : null}
      <div className="detail-stats">
        <MiniStat label="Latest turn" value={formatTokens(session.latest_turn_tokens)} />
        <MiniStat label="Window" value={formatTokens(session.window_tokens)} />
        <MiniStat label="Total" value={formatTokens(session.total_tokens)} />
        <MiniStat label="Calls" value={`${session.calls}`} />
      </div>
      <SpendMeter
        latest={session.latest_turn_tokens ?? 0}
        window={session.window_tokens ?? 0}
        warn={config.warn_turn_tokens}
        stop={config.kill_turn_tokens}
      />
      <TurnTimeline turns={turns} config={config} selectedTurnKey={selectedTurnKey} onSelectTurn={setSelectedTurnKey} />
      <TurnTable turns={turns} selectedTurnKey={selectedTurnKey} />
      <div className="detail-section">
        <h3>Correlation</h3>
        <dl>
          <dt>Process</dt>
          <dd>{session.correlated_pid ? `pid ${session.correlated_pid}` : "uncorrelated"}</dd>
          <dt>Reason</dt>
          <dd>{session.correlation_reason ?? "-"}</dd>
          <dt>Confidence</dt>
          <dd>{session.confidence ?? "-"}</dd>
          <dt>Actionable</dt>
          <dd>{session.actionable ? "yes" : "no"}</dd>
        </dl>
      </div>
      <div className="detail-section">
        <h3>Models</h3>
        <p>{session.models?.join(", ") || "-"}</p>
      </div>
    </aside>
  );
}

function canAcknowledge(session: SessionView): boolean {
  return session.can_acknowledge;
}

function canStop(session: SessionView): boolean {
  const hasAppIdentity = Boolean(session.correlated_executable || session.correlated_bundle_id || session.correlated_team_id);
  return Boolean(
    session.actionable &&
      session.action_state === "stop-pending" &&
      session.correlated_pid &&
      session.correlated_process_started_at &&
      session.correlated_owner &&
      hasAppIdentity,
  );
}

function ModelChips({ models }: { models?: string[] }) {
  if (!models || models.length === 0) return <span>-</span>;
  const [first, ...rest] = models;
  return (
    <div className="model-chips">
      <span>{first}</span>
      {rest.length > 0 ? <span>+{rest.length}</span> : null}
    </div>
  );
}

function SpendMeter({ latest, window, warn, stop }: { latest: number; window: number; warn: number; stop: number }) {
  const denominator = Math.max(stop, warn, latest, 1);
  const latestWidth = Math.max(3, Math.min(100, (latest / denominator) * 100));
  const warnLeft = Math.min(100, (warn / denominator) * 100);
  const stopLeft = Math.min(100, (stop / denominator) * 100);
  return (
    <div className="spend-meter" aria-label={`Latest turn ${formatTokens(latest)}, window ${formatTokens(window)}`}>
      <div className="spend-row">
        <strong>{formatTokens(latest)}</strong>
        <span>latest turn</span>
      </div>
      <div className="spend-track">
        <div className="spend-fill" style={{ width: `${latestWidth}%` }} />
        <i className="threshold warn" style={{ left: `${warnLeft}%` }} />
        <i className="threshold stop" style={{ left: `${stopLeft}%` }} />
      </div>
      <span className="window-context">{formatTokens(window)} in window</span>
    </div>
  );
}

function TurnTimeline({
  turns,
  config,
  selectedTurnKey,
  onSelectTurn,
}: {
  turns: TurnView[];
  config: ConfigView;
  selectedTurnKey: string;
  onSelectTurn: (key: string) => void;
}) {
  if (turns.length === 0) {
    return (
      <div className="turn-timeline" aria-label="Turn token timeline">
        <span className="empty">No recent turns in the current API response.</span>
      </div>
    );
  }
  const scale = Math.max(maxTurn(turns), config.kill_turn_tokens, config.warn_turn_tokens, 1);
  const warnPosition = thresholdPosition(config.warn_turn_tokens, scale);
  const stopPosition = thresholdPosition(config.kill_turn_tokens, scale);
  const selectedTurn = turns.find((turn) => turnKey(turn) === selectedTurnKey) ?? turns[0];
  return (
    <section
      className="turn-timeline"
      aria-label={`Turn token timeline. Warning threshold ${formatTokens(config.warn_turn_tokens)}. Stop threshold ${formatTokens(config.kill_turn_tokens)}.`}
    >
      <div className="timeline-heading">
        <h3>Turn Timeline</h3>
        <span>
          Window {formatDuration(config.usage_window_seconds)} · warn {formatTokens(config.warn_turn_tokens)} · stop{" "}
          {formatTokens(config.kill_turn_tokens)}
        </span>
      </div>
      <div className="timeline-bars">
        {turns.map((turn) => (
          <TurnTimelineRow
            key={turnKey(turn)}
            turn={turn}
            scale={scale}
            warnPosition={warnPosition}
            stopPosition={stopPosition}
            selected={turnKey(turn) === selectedTurnKey}
            onSelect={() => onSelectTurn(turnKey(turn))}
          />
        ))}
      </div>
      <SelectedTurnMix turn={selectedTurn} />
      <div className="timeline-legend" aria-label="Timeline legend">
        <span><i className="segment input" /> input</span>
        <span><i className="segment cached" /> cached</span>
        <span><i className="segment output" /> output</span>
        <span><i className="segment reasoning" /> reasoning</span>
      </div>
    </section>
  );
}

function TurnTimelineRow({
  turn,
  scale,
  warnPosition,
  stopPosition,
  selected,
  onSelect,
}: {
  turn: TurnView;
  scale: number;
  warnPosition: React.CSSProperties;
  stopPosition: React.CSSProperties;
  selected: boolean;
  onSelect: () => void;
}) {
  const total = turn.total_tokens ?? 0;
  const width = Math.max(2, Math.min(100, (total / scale) * 100));
  const segments = turnSegments(turn);
  return (
    <button
      type="button"
      className={`timeline-row ${selected ? "selected" : ""}`}
      aria-label={`${relativeTime(turn.at)} ${turn.model || "unknown model"} total ${formatTokens(total)} input ${formatTokens(
        turn.input_tokens,
      )} cached ${formatTokens(turn.cached_input_tokens)} output ${formatTokens(turn.output_tokens)} reasoning ${formatTokens(
        turn.reasoning_output_tokens,
      )}`}
      aria-current={selected ? "true" : undefined}
      onClick={onSelect}
    >
      <div className="timeline-meta">
        <strong>{formatTokens(total)}</strong>
        <span>{turn.model || turn.provider}</span>
        <span>{relativeTime(turn.at)}</span>
      </div>
      <div className="timeline-track">
        <i className="timeline-threshold warn" style={warnPosition} aria-hidden="true" />
        <i className="timeline-threshold stop" style={stopPosition} aria-hidden="true" />
        <div className="timeline-total" style={{ width: `${width}%` }}>
          {segments.map((segment) => (
            <i
              className={`timeline-segment ${segment.kind}`}
              key={segment.kind}
              style={{ width: `${segment.width}%` }}
              title={`${segment.kind}: ${formatTokens(segment.value)}`}
            />
          ))}
        </div>
      </div>
    </button>
  );
}

function SelectedTurnMix({ turn }: { turn: TurnView }) {
  return (
    <div className="selected-turn-mix" aria-label="Selected turn token fields">
      <span>Selected</span>
      <strong>{formatTokens(turn.total_tokens)} total</strong>
      <span>input {formatTokens(turn.input_tokens)}</span>
      <span>cached {formatTokens(turn.cached_input_tokens)}</span>
      <span>output {formatTokens(turn.output_tokens)}</span>
      <span>reasoning {formatTokens(turn.reasoning_output_tokens)}</span>
    </div>
  );
}

function AlertFeed({
  alerts,
  ackSeconds,
  ackMessage,
  onAck,
}: {
  alerts: AlertView[];
  ackSeconds: number;
  ackMessage: string;
  onAck: (sessionKey: string) => void;
}) {
  const visible = alerts.slice(-10).reverse();
  return (
    <aside className="event-feed">
      <div className="panel-title">
        <AlertTriangle size={16} />
        <h2>Alerts</h2>
      </div>
      {visible.length === 0 ? (
        <p className="empty">No recent warnings or stop events.</p>
      ) : (
        <ol>
          {visible.map((event) => (
            <li key={`${event.seq}-${event.category}`}>
              <span className={`event-type ${event.severity}`}>{event.label}</span>
              <strong>{event.message}</strong>
              <p>{eventMeta(event)}</p>
              {event.explanation ? <p>{event.explanation}</p> : null}
              {event.can_acknowledge && event.session_key ? (
                <button type="button" className="inline-action" onClick={() => onAck(event.session_key!)}>
                  <CheckCircle2 size={14} />
                  Extend {formatDuration(ackSeconds || 1800)}
                </button>
              ) : null}
            </li>
          ))}
        </ol>
      )}
      {ackMessage ? <p className="feed-message">{ackMessage}</p> : null}
    </aside>
  );
}

function eventMeta(event: AlertView): string {
  const parts = [relativeTime(event.at)];
  if (event.agent_id) parts.push(event.agent_id);
  if (event.mode) parts.push(event.mode);
  if (event.cwd) parts.push(shortPath(event.cwd));
  return parts.join(" · ");
}

function shortPath(path: string): string {
  const home = "/Users/phaedrus/";
  if (path.startsWith(home)) return `~/${path.slice(home.length)}`;
  return path;
}

function TurnTable({ turns, selectedTurnKey }: { turns: TurnView[]; selectedTurnKey: string }) {
  const rowRefs = useRef(new Map<string, HTMLTableRowElement>());

  useEffect(() => {
    const row = rowRefs.current.get(selectedTurnKey);
    if (!row) return;
    row.focus({ preventScroll: true });
    row.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }, [selectedTurnKey, turns]);

  if (turns.length === 0) {
    return null;
  }
  return (
    <div className="turn-table">
      <h3>Turn Breakdown</h3>
      <table>
        <thead>
          <tr>
            <th>When</th>
            <th>Model</th>
            <th>Input</th>
            <th>Cached</th>
            <th>Created</th>
            <th>Output</th>
            <th>Reasoning</th>
            <th>Total</th>
            <th>Cumulative</th>
          </tr>
        </thead>
        <tbody>
          {turns.map((turn) => {
            const key = turnKey(turn);
            const selected = key === selectedTurnKey;
            return (
              <tr
                className={selected ? "selected" : ""}
                key={key}
                aria-current={selected ? "true" : undefined}
                tabIndex={selected ? 0 : -1}
                ref={(node) => {
                  if (node) rowRefs.current.set(key, node);
                  else rowRefs.current.delete(key);
                }}
              >
                <td data-label="When">{relativeTime(turn.at)}</td>
                <td data-label="Model">{turn.model ?? "-"}</td>
                <td data-label="Input">{formatTokens(turn.input_tokens)}</td>
                <td data-label="Cached">{formatTokens(turn.cached_input_tokens)}</td>
                <td data-label="Created">{formatTokens(turn.cache_creation_input_tokens)}</td>
                <td data-label="Output">{formatTokens(turn.output_tokens)}</td>
                <td data-label="Reasoning">{formatTokens(turn.reasoning_output_tokens)}</td>
                <td data-label="Total">
                  <strong>{formatTokens(turn.total_tokens)}</strong>
                </td>
                <td data-label="Cumulative">{formatTokens(turn.cumulative_tokens)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function MiniStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="mini-stat">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function StatusPill({ status }: { status: string }) {
  const tone = statusTone(status);
  const Icon = tone === "ok" ? CheckCircle2 : tone === "watch" ? AlertTriangle : Activity;
  return (
    <span className={`status-pill ${tone}`}>
      <Icon size={15} />
      {status}
    </span>
  );
}

function StateChip({ state, usageState }: { state: string; usageState?: string }) {
  return <span className={`state-chip ${cssState(state)}`}>{stateLabel(state, usageState)}</span>;
}

function maxTurn(turns: TurnView[]): number {
  return Math.max(1, ...turns.map((turn) => turn.total_tokens ?? 0));
}

function turnKey(turn: TurnView): string {
  return turn.id || turn.request_id || `${turn.provider}:${turn.at}:${turn.total_tokens ?? 0}`;
}

function thresholdPosition(tokens: number, scale: number): React.CSSProperties {
  const position = (tokens / scale) * 100;
  if (position <= 0) return { left: 0 };
  if (position >= 100) return { right: 0 };
  return { left: `${position}%`, transform: "translateX(-50%)" };
}

function turnSegments(turn: TurnView): Array<{ kind: string; value: number; width: number }> {
  const raw = [
    { kind: "input", value: turn.input_tokens ?? 0 },
    { kind: "cached", value: turn.cached_input_tokens ?? 0 },
    { kind: "output", value: turn.output_tokens ?? 0 },
    { kind: "reasoning", value: turn.reasoning_output_tokens ?? 0 },
  ].filter((segment) => segment.value > 0);
  const total = Math.max(1, raw.reduce((sum, segment) => sum + segment.value, 0));
  if (raw.length === 0) {
    return [{ kind: "input", value: turn.total_tokens ?? 0, width: 100 }];
  }
  return raw.map((segment) => ({ ...segment, width: Math.max(2, Math.min(100, (segment.value / total) * 100)) }));
}

function listLabel(values: string[]): string {
  if (values.length === 0) return "none yet";
  if (values.length <= 2) return values.join(", ");
  return `${values.slice(0, 2).join(", ")} +${values.length - 2}`;
}

function sourceSummary(sources: { files: number; events: number }[]): string {
  if (sources.length === 0) return "not scanned";
  const files = sources.reduce((sum, source) => sum + source.files, 0);
  const events = sources.reduce((sum, source) => sum + source.events, 0);
  return `${formatTokens(events)} events / ${files} files`;
}

function numberValue(value: number): number {
  return Number.isFinite(value) ? value : 0;
}

function cssState(state: string): string {
  return state.toLowerCase().replace(/[^a-z0-9-]+/g, "-");
}
