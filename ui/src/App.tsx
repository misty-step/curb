import { Maximize2, Minimize2, RefreshCw, TriangleAlert } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import {
  acknowledgeSession,
  fetchConfig,
  fetchNotificationHealth,
  fetchOnboarding,
  fetchReadiness,
  fetchSession,
  fetchSessionTurns,
  fetchSnapshot,
  rescanService,
  saveConfig,
  stopSession,
  testNotification,
  type ApiSettings,
} from "./api";
import { AgentList, ConnectionBanner, StatusWord, providerLabel } from "./components/dashboard";
import { ModeToggle } from "./components/mode";
import { Settings } from "./components/settings";
import { demoConfig, demoNotifications, demoSnapshot } from "./demo";
import { commas } from "./format";
import { selectDashboard, selectRecovery, selectSessionExplanation, type RecoveryModel } from "./readModel";
import { modeFromConfig, type CapabilityView, type ConfigUpdate, type ConfigView, type Mode, type NotificationView, type OnboardingView, type ReadinessView, type SessionView, type Snapshot, type Status, type TurnView } from "./types";

// curb app serves the dashboard same-origin and authenticates with an HttpOnly
// cookie, so there is no URL or token to enter — we just talk to our own origin.
const SAME_ORIGIN = window.location.protocol.startsWith("http") ? window.location.origin : "";
const SETTINGS: ApiSettings = { baseUrl: SAME_ORIGIN, token: "" };
const POLL_MS = 2000;

// Pure derivation of the dashboard's chrome from service state, kept out of the
// component so the branching lives in one testable place. It surfaces only what
// a user must see: whether enforcement can actually act (the lede never claims
// "armed" without evidence) and the broken-promise health lines (may miss spend
// / may not be warned). Operator plumbing stays out of the UI.
function dashboardChrome(args: {
  connection: "demo" | "live" | "error";
  mode: Mode;
  enforcement?: CapabilityView;
  recovery: RecoveryModel;
  notifications: NotificationView;
  config: ConfigView;
}): { headerDetail: string; policySummary: string; healthWarnings: string[] } {
  const { connection, mode, enforcement, recovery, notifications, config } = args;
  const enforceBlocked = mode === "enforce" ? !enforcement?.available : false;
  const headerDetail =
    connection === "error"
      ? "Showing demo data until the local API responds."
      : mode !== "enforce"
        ? "Warn only is armed; Curb will not stop processes."
        : enforceBlocked
          ? "Stop runaways is set, but Curb can't stop anything right now."
          : "Stop runaways is armed for correlated worker processes.";
  const policySummary =
    mode === "enforce"
      ? `Warn at ${commas(config.warn_turn_tokens)} · stop at ${commas(config.kill_turn_tokens)}`
      : `Warn at ${commas(config.warn_turn_tokens)} · stop disabled`;
  // No health lines while disconnected — the snapshot is stale demo data then.
  if (connection === "error") return { headerDetail, policySummary, healthWarnings: [] };
  const degradedProviders = recovery.items
    .filter((item) => item.id.startsWith("source-"))
    .map((item) => providerLabel(item.id.slice("source-".length)));
  const healthWarnings = [
    ...(degradedProviders.length
      ? [`Curb can't read all of your ${degradedProviders.join(" and ")} usage right now, so it may miss spend there.`]
      : []),
    ...(notifications.enabled && !notifications.available
      ? ["Notifications are on, but Curb can't show them right now — you may not be warned."]
      : []),
  ];
  return { headerDetail, policySummary, healthWarnings };
}

// The slim top bar: brand and live status on the left, controls on the right.
// In compact the mode tag and the Rescan label drop away to icon-only controls;
// the expand toggle flips between the table-only view and the full dashboard.
function DashboardBar({
  status,
  mode,
  expanded,
  onRescan,
  onToggleExpanded,
}: {
  status: Status;
  mode: Mode;
  expanded: boolean;
  onRescan: () => void;
  onToggleExpanded: () => void;
}): ReactNode {
  return (
    <header className="ae-bar topbar">
      <span className="ae-name">CURB</span>
      <span className="topbar-acts">
        <StatusWord status={status} />
        {expanded ? (
          <span className="ae-tag ae-tag-bare topbar-mode">{mode === "enforce" ? "enforce" : "watch"}</span>
        ) : null}
        <button
          type="button"
          className="ae-button ae-button-quiet ae-button-compact"
          onClick={onRescan}
          aria-label="Rescan usage"
          title="Rescan usage"
        >
          <RefreshCw className="ae-icon" />
          {expanded ? "Rescan" : null}
        </button>
        <button
          type="button"
          className="ae-mode"
          onClick={onToggleExpanded}
          aria-expanded={expanded}
          aria-label={expanded ? "Collapse to the compact agent table" : "Expand for limits and detail"}
          title={expanded ? "Compact view" : "Expand"}
        >
          {expanded ? <Minimize2 className="ae-icon" /> : <Maximize2 className="ae-icon" />}
        </button>
        <ModeToggle />
      </span>
    </header>
  );
}

export function App() {
  const [snapshot, setSnapshot] = useState<Snapshot>(demoSnapshot);
  const [config, setConfig] = useState<ConfigView>(demoConfig);
  const [notifications, setNotifications] = useState<NotificationView>(demoNotifications);
  const [onboarding, setOnboarding] = useState<OnboardingView>();
  const [readinessView, setReadinessView] = useState<ReadinessView>();
  const [connection, setConnection] = useState<"demo" | "live" | "error">(SAME_ORIGIN ? "live" : "demo");
  const [error, setError] = useState("");
  const [selectedKey, setSelectedKey] = useState("");
  const [selectedSession, setSelectedSession] = useState<SessionView>();
  const [selectedTurns, setSelectedTurns] = useState<TurnView[]>([]);
  const [settingsMsg, setSettingsMsg] = useState("");
  const [busyKey, setBusyKey] = useState("");
  const [busyMsg, setBusyMsg] = useState("");
  // Compact is the default: a slim bar over the agent table. Expand reveals the
  // headline lede and the Limits & mode drawer. The choice persists so the
  // window reopens the way you left it. The menu-bar app carries the at-a-glance
  // headline, so the in-window paragraph is redundant detail when compact.
  const [expanded, setExpanded] = useState(() => {
    try {
      return localStorage.getItem("curb-view") === "expanded";
    } catch {
      return false;
    }
  });

  const toggleExpanded = useCallback(() => {
    setExpanded((prev) => {
      const next = !prev;
      try {
        localStorage.setItem("curb-view", next ? "expanded" : "compact");
      } catch {
        // Private mode: the choice simply does not persist.
      }
      return next;
    });
  }, []);

  const refresh = useCallback(async (forceRescan = false) => {
    const nextReadiness = await fetchReadiness(SETTINGS).catch<ReadinessView | undefined>(() => undefined);
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
      setReadinessView(nextReadiness);
      setConnection(SAME_ORIGIN ? "live" : "demo");
      setError("");
    } catch (caught) {
      setReadinessView(nextReadiness);
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
  const recovery = useMemo(
    () =>
      selectRecovery(
        onboarding,
        readinessView,
        {
          connectionError: connection === "error" ? error : "",
          configPath: config.path ?? onboarding?.config_path,
          overviewRecovery: snapshot.overview.recovery,
        },
      ),
    [onboarding, readinessView, connection, error, config.path, snapshot.overview.recovery],
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
    const previous = config;
    setConfig((current) => ({ ...current, ...update }));
    setSettingsMsg("");
    try {
      const saved = await saveConfig(SETTINGS, update);
      setConfig(saved);
      setSettingsMsg(SAME_ORIGIN ? "Saved." : "Demo only — run curb app to save.");
      void refresh();
    } catch (caught) {
      setConfig(previous);
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
      setBusyMsg(`Stopped: ${stopped.scope_pids.length} process${stopped.scope_pids.length === 1 ? "" : "es"} in scope.`);
      await refresh(true);
    } catch (caught) {
      setBusyMsg(caught instanceof Error ? caught.message : "Could not stop");
      await refresh(true);
    }
  }

  async function runTest() {
    setNotifications(await testNotification(SETTINGS));
  }

  const mode = modeFromConfig(config.mode);
  // Never claim enforcement is armed when the platform can't actually stop:
  // a user who chose "Stop runaways" must not be told it works when it doesn't.
  const { headerDetail, policySummary, healthWarnings } = dashboardChrome({
    connection,
    mode,
    enforcement: snapshot.overview.capabilities?.enforcement,
    recovery,
    notifications,
    config,
  });

  return (
    <div className={expanded ? "ae-screen ae-wide" : "ae-screen curb-compact"}>
      <DashboardBar
        status={snapshot.overview.status}
        mode={mode}
        expanded={expanded}
        onRescan={() => void refresh(true)}
        onToggleExpanded={toggleExpanded}
      />

      <main className="ae-stage ae-stage-scroll">
        <div>
          {expanded ? (
            <section className="ae-group">
              <h1 className="ae-strong">{model.headline}</h1>
              <p className="ae-dim lede-detail">{headerDetail}</p>
            </section>
          ) : null}

          {/* Health warnings ride above the table in both views: a user must
              always see when Curb may miss spend or can't warn — that is the
              one thing more important than the table itself. */}
          {healthWarnings.length ? (
            <section className="health-block">
              {healthWarnings.map((warning) => (
                <p className="health-note" key={warning}>
                  <TriangleAlert className="ae-icon ae-warn" /> {warning}
                </p>
              ))}
            </section>
          ) : null}

          {connection === "error" ? <ConnectionBanner error={error} /> : null}

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

          {expanded ? (
            <details className="ae-fold drawer">
              <summary>
                Limits &amp; mode
                <em className="ae-num">{policySummary}</em>
              </summary>
              <Settings
                config={config}
                notifications={notifications}
                message={settingsMsg}
                onSave={persist}
                onTestNotification={() => void runTest()}
              />
            </details>
          ) : null}
        </div>
      </main>
    </div>
  );
}
