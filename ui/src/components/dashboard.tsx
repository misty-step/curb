import { CircleDot, OctagonX, ShieldCheck, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { commas, numberValue, relativeTime, tokens } from "../format";
import { fillRatio, type SelectedSessionExplanation, warnRatio } from "../readModel";
import {
  type ConfigUpdate,
  type ConfigView,
  type Mode,
  type NotificationView,
  type SessionView,
  type Status,
  modeFromConfig,
  modeToConfig,
} from "../types";
import { SelectedSessionPanel } from "./sessionPanels";
import { SessionActionStrip } from "./sessionActions";

export function StatusPill({ status }: { status: Status }): ReactNode {
  const icon = status === "ACTION" ? <OctagonX size={14} /> : status === "WATCH" ? <TriangleAlert size={14} /> : <ShieldCheck size={14} />;
  return (
    <span className={`pill pill-${status.toLowerCase()}`}>
      {icon}
      {status}
    </span>
  );
}

export function connectionMessage(error: string): string {
  if (!error) return "Unable to reach the local Curb API.";
  if (error.includes("<!doctype") || error.includes("not valid JSON")) {
    return "The dashboard reached the dev server instead of the Curb API. Run curb app for live data.";
  }
  return error;
}

export function ConnectionBanner({ error }: { error: string }): ReactNode {
  return (
    <div className="connection-banner" role="status">
      <TriangleAlert size={16} />
      <div>
        <strong>Live data unavailable</strong>
        <span>{connectionMessage(error)}</span>
      </div>
    </div>
  );
}

export function providerLabel(provider: string): string {
  if (provider === "codex") return "Codex";
  if (provider === "claude") return "Claude Code";
  if (provider === "pi") return "Pi";
  if (provider === "antigravity") return "Antigravity";
  return provider;
}

// Active rows are running agents, so an in-limits row is "working" (green) for
// its whole on-screen life — it does not gray out between model calls. Quiet
// agents live in the idle fold, which does not use this.
function tone(session: SessionView): "kill" | "warn" | "working" {
  if (session.alert === "kill") return "kill";
  if (session.alert === "warn") return "warn";
  return "working";
}

function SpendBar({ session, config }: { session: SessionView; config: ConfigView }): ReactNode {
  const fill = fillRatio(session.turn_tokens, config.kill_turn_tokens) * 100;
  const warnAt = warnRatio(config.warn_turn_tokens, config.kill_turn_tokens) * 100;
  return (
    <div
      className={`bar bar-${tone(session)}`}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={config.kill_turn_tokens}
      aria-valuenow={session.turn_tokens}
      aria-label={`${tokens(session.turn_tokens)} of ${tokens(config.kill_turn_tokens)} tokens this turn; warn at ${tokens(config.warn_turn_tokens)}`}
    >
      <div className="bar-fill" style={{ width: `${fill}%` }} />
      <span className="bar-tick" style={{ left: `${warnAt}%` }} title={`warn ${tokens(config.warn_turn_tokens)}`} />
    </div>
  );
}

// Derives the chip from the same `tone` the row colour uses, so label and colour
// can never drift apart.
function StatusChip({ session }: { session: SessionView }): ReactNode {
  if (session.acknowledged_until) return <span className="chip chip-ack">acknowledged</span>;
  const state = tone(session);
  const label = state === "kill" ? "over kill" : state === "warn" ? "over warn" : "working";
  return <span className={`chip chip-${state}`}>{label}</span>;
}

interface RowProps {
  session: SessionView;
  config: ConfigView;
  selected: boolean;
  onSelect: (key: string) => void;
  onAck: (session: SessionView) => void;
  onStop: (session: SessionView) => void;
  busy: string;
  detail: SelectedSessionExplanation | undefined;
}

function AgentRow({ session, config, selected, onSelect, onAck, onStop, busy, detail }: RowProps): ReactNode {
  return (
    <div className={`row row-${tone(session)} ${selected ? "row-open" : ""}`}>
      <button type="button" className="row-head" onClick={() => onSelect(selected ? "" : session.key)}>
        <div className="row-top">
          <div className="row-id">
            <span className="row-project">{session.project ?? session.id}</span>
            <span className="row-meta">
              {providerLabel(session.provider)} · {relativeTime(session.last_activity_at)}
            </span>
          </div>
          <div className="row-spend">
            <span className="row-tokens">{tokens(session.turn_tokens)}</span>
            <StatusChip session={session} />
          </div>
        </div>
        <SpendBar session={session} config={config} />
      </button>
      {selected ? (
        <div className="row-detail">
          <p className="row-why">{session.explanation}</p>
          <dl className="row-facts">
            <Fact label="This turn" value={tokens(session.turn_tokens)} />
            <Fact label="Total spent" value={tokens(session.total_tokens)} />
            <Fact label="Model calls" value={String(session.calls)} />
            <Fact label="Last activity" value={relativeTime(session.last_activity_at)} />
            {session.models.length ? <Fact label="Model" value={session.models.join(", ")} /> : null}
            {session.pid ? <Fact label="Worker" value={`pid ${session.pid}`} /> : null}
          </dl>
          {detail ? <SelectedSessionPanel detail={detail} /> : <p className="row-busy">Loading session detail…</p>}
          {session.cwd ? <p className="row-cwd">{session.cwd}</p> : null}
          <SessionActionStrip session={session} onAck={onAck} onStop={onStop} />
          {busy ? <p className="row-busy">{busy}</p> : null}
        </div>
      ) : null}
    </div>
  );
}

function Fact({ label, value }: { label: string; value: string }): ReactNode {
  return (
    <div className="fact">
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  );
}

interface AgentListProps {
  active: SessionView[];
  idle: SessionView[];
  config: ConfigView;
  selectedKey: string;
  onSelect: (key: string) => void;
  onAck: (session: SessionView) => void;
  onStop: (session: SessionView) => void;
  busyKey: string;
  busyMessage: string;
  selectedDetail: SelectedSessionExplanation | undefined;
}

export function AgentList(props: AgentListProps): ReactNode {
  const { active, idle, config, selectedKey, onSelect, onAck, onStop, busyKey, busyMessage, selectedDetail } = props;
  return (
    <section className="agents">
      {active.length === 0 ? (
        <EmptyState config={config} />
      ) : (
        active.map((session) => (
          <AgentRow
            key={session.key}
            session={session}
            config={config}
            selected={session.key === selectedKey}
            onSelect={onSelect}
            onAck={onAck}
            onStop={onStop}
            busy={session.key === busyKey ? busyMessage : ""}
            detail={session.key === selectedKey ? selectedDetail : undefined}
          />
        ))
      )}
      {idle.length ? (
        <details className="idle-fold">
          <summary>
            {idle.length} idle {idle.length === 1 ? "agent" : "agents"} — active in the last{" "}
            {Math.max(1, Math.round(config.usage_window_seconds / 60))} min, not spending now
          </summary>
          <div className="idle-list">
            {idle.map((session) => (
              <div className="idle-row" key={session.key}>
                <span className="row-project">{session.project ?? session.id}</span>
                <span className="row-meta">{providerLabel(session.provider)}</span>
                <span className="idle-tokens">{tokens(session.turn_tokens)} last turn</span>
                <span className="idle-when">{relativeTime(session.last_activity_at)}</span>
              </div>
            ))}
          </div>
        </details>
      ) : null}
    </section>
  );
}

// The calm, good state: nothing is spending. Instead of a bare line, it
// confirms Curb is armed — what it watches and at what limits — so the quiet is
// reassuring rather than ambiguous (is it working? is it connected?).
function EmptyState({ config }: { config: ConfigView }): ReactNode {
  const enforce = modeFromConfig(config.mode) === "enforce";
  return (
    <div className="empty">
      <span className="empty-gauge" aria-hidden="true">
        <span className="empty-gauge-tick" />
      </span>
      <p className="empty-title">
        <ShieldCheck size={15} /> Nothing spending right now
      </p>
      <p className="empty-sub">
        Watching Codex and Claude Code. Curb warns over {commas(config.warn_turn_tokens)} tokens a
        turn{enforce ? ` and stops a runaway over ${commas(config.kill_turn_tokens)}` : ""}.
      </p>
    </div>
  );
}

interface SettingsProps {
  config: ConfigView;
  notifications: NotificationView;
  message: string;
  onSave: (update: ConfigUpdate) => void;
  onTestNotification: () => void;
}

export function Settings({ config, notifications, message, onSave, onTestNotification }: SettingsProps): ReactNode {
  const mode = modeFromConfig(config.mode);
  return (
    <div className="settings">
      <p className="setting-hint">
        A turn is the work an agent does between your inputs. Limits apply to each turn.
      </p>
      <LimitField
        id="warn"
        label="Warn at"
        value={config.warn_turn_tokens}
        onSave={(value) => onSave({ warn_turn_tokens: value })}
      />
      <LimitField
        id="kill"
        label="Kill at"
        value={config.kill_turn_tokens}
        onSave={(value) => onSave({ kill_turn_tokens: value })}
      />
      <div className="setting">
        <span className="setting-label">When an agent crosses the kill line</span>
        <ModeToggle mode={mode} onChange={(next) => onSave({ mode: modeToConfig(next) })} />
        {mode === "enforce" ? (
          <label className="setting-check">
            <input
              type="checkbox"
              checked={config.escalate_supervised}
              onChange={(event) => onSave({ escalate_supervised: event.target.checked })}
            />
            <span className="setting-check-text">
              Also stop supervised desktop agents
              <span className="note">
                Desktop apps can respawn workers. This stops the supervisor and every task running under it.
              </span>
            </span>
          </label>
        ) : null}
      </div>
      <div className="setting setting-row">
        <label htmlFor="notify">Notify me</label>
        <input
          id="notify"
          type="checkbox"
          checked={config.local_notifications}
          onChange={(event) => onSave({ local_notifications: event.target.checked })}
        />
        <button type="button" className="btn btn-ghost" onClick={onTestNotification}>
          Test
        </button>
        <span className={`note ${notifications.available ? "" : "note-warn"}`}>{notifications.message}</span>
      </div>
      {message ? <p className="settings-msg">{message}</p> : null}
    </div>
  );
}

// A token limit shown with thousands separators (1,000,000) and parsed back to
// a plain number on blur.
function LimitField({
  id,
  label,
  value,
  onSave,
}: {
  id: string;
  label: string;
  value: number;
  onSave: (value: number) => void;
}): ReactNode {
  return (
    <div className="setting">
      <label htmlFor={id}>{label}</label>
      <div className="token-input">
        <input
          id={id}
          type="text"
          inputMode="numeric"
          defaultValue={commas(value)}
          onBlur={(event) => {
            const parsed = numberValue(event.target.value.replace(/[^0-9]/g, ""));
            event.target.value = commas(parsed);
            onSave(parsed);
          }}
        />
        <span>tokens / turn</span>
      </div>
    </div>
  );
}

function ModeToggle({ mode, onChange }: { mode: Mode; onChange: (mode: Mode) => void }): ReactNode {
  return (
    <div className="toggle" role="group" aria-label="Mode">
      <button
        type="button"
        aria-pressed={mode === "watch"}
        className={mode === "watch" ? "on" : ""}
        onClick={() => onChange("watch")}
      >
        Warn only
      </button>
      <button
        type="button"
        aria-pressed={mode === "enforce"}
        className={mode === "enforce" ? "on enforce" : ""}
        onClick={() => onChange("enforce")}
      >
        Stop runaways
      </button>
    </div>
  );
}

export function ConnectionNote({
  connection,
  error,
}: {
  connection: "demo" | "live" | "error";
  error: string;
}): ReactNode {
  const label = connection === "live" ? "Live local daemon" : connection === "error" ? "Connection issue" : "Demo data";
  return (
    <div className={`connection connection-${connection}`}>
      <CircleDot size={12} />
      <span>{label}</span>
      {connection === "demo" ? <span className="note">Run curb app for live agent data.</span> : null}
      {error ? <span className="note note-warn">{connectionMessage(error)}</span> : null}
    </div>
  );
}
