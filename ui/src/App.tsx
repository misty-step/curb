import { RefreshCw, Shield, SlidersHorizontal } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  acknowledgeSession,
  fetchConfig,
  fetchNotificationHealth,
  fetchOnboarding,
  fetchSession,
  fetchSessionTurns,
  fetchSnapshot,
  rescanService,
  saveConfig,
  stopSession,
  testNotification,
  type ApiSettings,
} from "./api";
import { AgentList, ConnectionNote, Settings, StatusPill } from "./components/dashboard";
import { ReadinessPanel } from "./components/sessionPanels";
import { demoConfig, demoNotifications, demoSnapshot } from "./demo";
import { selectDashboard, selectReadiness, selectSessionExplanation } from "./readModel";
import type { ConfigUpdate, ConfigView, NotificationView, OnboardingView, SessionView, Snapshot, TurnView } from "./types";

// curb app serves the dashboard same-origin and authenticates with an HttpOnly
// cookie, so there is no URL or token to enter — we just talk to our own origin.
const SAME_ORIGIN = window.location.protocol.startsWith("http") ? window.location.origin : "";
const SETTINGS: ApiSettings = { baseUrl: SAME_ORIGIN, token: "" };
const POLL_MS = 2000;

export function App() {
  const [snapshot, setSnapshot] = useState<Snapshot>(demoSnapshot);
  const [config, setConfig] = useState<ConfigView>(demoConfig);
  const [notifications, setNotifications] = useState<NotificationView>(demoNotifications);
  const [onboarding, setOnboarding] = useState<OnboardingView>();
  const [connection, setConnection] = useState<"demo" | "live" | "error">(SAME_ORIGIN ? "live" : "demo");
  const [error, setError] = useState("");
  const [selectedKey, setSelectedKey] = useState("");
  const [selectedSession, setSelectedSession] = useState<SessionView>();
  const [selectedTurns, setSelectedTurns] = useState<TurnView[]>([]);
  const [settingsMsg, setSettingsMsg] = useState("");
  const [busyKey, setBusyKey] = useState("");
  const [busyMsg, setBusyMsg] = useState("");

  const refresh = useCallback(async (forceRescan = false) => {
    try {
      const data = forceRescan ? await rescanService(SETTINGS) : await fetchSnapshot(SETTINGS);
      const [nextConfig, nextNotifications] = await Promise.all([
        fetchConfig(SETTINGS),
        fetchNotificationHealth(SETTINGS).catch<NotificationView>(() => ({
          enabled: false,
          available: false,
          status: "error",
          message: "Notification health unavailable",
        })),
      ]);
      const nextOnboarding = await fetchOnboarding(SETTINGS).catch<OnboardingView | undefined>(() => undefined);
      setSnapshot(data);
      setConfig(nextConfig);
      setNotifications(nextNotifications);
      setOnboarding(nextOnboarding);
      setConnection(SAME_ORIGIN ? "live" : "demo");
      setError("");
    } catch (caught) {
      setConnection("error");
      setError(caught instanceof Error ? caught.message : "Unable to reach the Curb daemon");
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), POLL_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const model = useMemo(() => selectDashboard(snapshot, config.usage_window_seconds), [snapshot, config.usage_window_seconds]);
  const selectedDetail = useMemo(
    () => selectSessionExplanation(selectedSession, selectedTurns),
    [selectedSession, selectedTurns],
  );
  const readiness = useMemo(
    () => selectReadiness(onboarding, notifications, snapshot.overview.capabilities),
    [onboarding, notifications, snapshot.overview.capabilities],
  );

  useEffect(() => {
    if (!selectedKey) {
      setSelectedSession(undefined);
      setSelectedTurns([]);
      return;
    }
    let cancelled = false;
    const fallback = snapshot.sessions.find((session) => session.key === selectedKey);
    setSelectedSession(fallback);
    setSelectedTurns([]);
    void Promise.all([fetchSession(SETTINGS, selectedKey), fetchSessionTurns(SETTINGS, selectedKey)])
      .then(([session, turns]) => {
        if (cancelled) return;
        setSelectedSession(session);
        setSelectedTurns(turns);
      })
      .catch((caught) => {
        if (cancelled) return;
        setSelectedSession(fallback);
        setSelectedTurns([]);
        setBusyKey(selectedKey);
        setBusyMsg(caught instanceof Error ? caught.message : "Could not load session detail");
      });
    return () => {
      cancelled = true;
    };
  }, [selectedKey, snapshot.sessions]);

  async function persist(update: ConfigUpdate) {
    try {
      const saved = await saveConfig(SETTINGS, update);
      setConfig(saved);
      setSettingsMsg(SAME_ORIGIN ? "Saved." : "Demo only — run curb app to save.");
      await refresh();
    } catch (caught) {
      setSettingsMsg(caught instanceof Error ? caught.message : "Could not save settings");
    }
  }

  async function acknowledge(session: SessionView) {
    setBusyKey(session.key);
    setBusyMsg("Acknowledging…");
    try {
      const ack = await acknowledgeSession(SETTINGS, session.key, config.ack_extension_seconds || 1800);
      setBusyMsg(`Acknowledged until ${new Date(ack.until).toLocaleTimeString()}.`);
      await refresh(true);
    } catch (caught) {
      setBusyMsg(caught instanceof Error ? caught.message : "Could not acknowledge");
    }
  }

  async function stop(session: SessionView) {
    if (!session.pid || !session.process_started_at) return;
    setBusyKey(session.key);
    setBusyMsg("Stopping…");
    try {
      const stopped = await stopSession(SETTINGS, session.key, {
        pid: session.pid,
        started_at: session.process_started_at,
        owner: session.owner,
        executable: session.executable,
        bundle_id: session.bundle_id,
        team_id: session.team_id,
      });
      setBusyMsg(`Stopped — ${stopped.scope_pids.length} process${stopped.scope_pids.length === 1 ? "" : "es"} in scope.`);
      await refresh(true);
    } catch (caught) {
      setBusyMsg(caught instanceof Error ? caught.message : "Could not stop");
      await refresh(true);
    }
  }

  async function runTest() {
    setNotifications(await testNotification(SETTINGS));
  }

  return (
    <main className="shell">
      <header className="topbar">
        <div className="brand">
          <span className="brand-mark">
            <Shield size={17} />
          </span>
          <span>Curb</span>
        </div>
        <p className="headline">{model.headline}</p>
        <div className="top-actions">
          <StatusPill status={snapshot.overview.status} />
          <span className="mode-tag">{snapshot.overview.mode}</span>
          <button type="button" className="icon-btn" aria-label="Rescan now" onClick={() => void refresh(true)}>
            <RefreshCw size={15} />
          </button>
        </div>
      </header>

      <AgentList
        active={model.active}
        idle={model.idle}
        config={config}
        selectedKey={selectedKey}
        onSelect={setSelectedKey}
        onAck={acknowledge}
        onStop={stop}
        busyKey={busyKey}
        busyMessage={busyMsg}
        selectedDetail={selectedDetail}
      />

      <ReadinessPanel model={readiness} />

      <details className="drawer">
        <summary>
          <SlidersHorizontal size={15} />
          Limits &amp; mode
        </summary>
        <Settings
          config={config}
          notifications={notifications}
          message={settingsMsg}
          onSave={persist}
          onTestNotification={() => void runTest()}
        />
      </details>

      <footer className="footer">
        <ConnectionNote connection={connection} error={error} />
      </footer>
    </main>
  );
}
