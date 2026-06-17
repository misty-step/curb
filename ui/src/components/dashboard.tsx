import { Check, OctagonX, ShieldCheck, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { commas, relativeTime, tokens } from "../format";
import { fillRatio, type SelectedSessionExplanation, warnRatio } from "../readModel";
import { type ConfigView, type SessionView, type Status, modeFromConfig } from "../types";
import { SelectedSessionPanel } from "./sessionPanels";
import { SessionActionStrip } from "./sessionActions";

// Status rides the glyph — the word stays a mono tag, never a filled pill.
export function StatusWord({ status }: { status: Status }): ReactNode {
  const icon =
    status === "ACTION" ? (
      <OctagonX className="ae-icon ae-err" />
    ) : status === "WATCH" ? (
      <TriangleAlert className="ae-icon ae-warn" />
    ) : (
      <ShieldCheck className="ae-icon ae-ok" />
    );
  // A bordered tag means an active alert; a bare word is steady-state. OK is
  // calm — let the glyph carry it so ACTION/WATCH read louder.
  return (
    <span className="status-word">
      {icon}
      <span className={status === "OK" ? "ae-tag ae-tag-bare" : "ae-tag"}>{status}</span>
    </span>
  );
}

export function connectionMessage(error: string): string {
  if (!error) return "Unable to reach the local Curb API.";
  if (error.includes("<!doctype") || error.includes("not valid JSON")) {
    return "The dashboard reached the dev server instead of the Curb API. Run curb app for live data.";
  }
  if (error === "Failed to fetch" || error.includes("NetworkError")) {
    return "Curb's local service isn't responding. Start it from your terminal, then Rescan.";
  }
  if (/^40[13]\b/.test(error)) {
    return "Curb's local service is running but this window isn't authenticated. Reopen Curb from your terminal.";
  }
  return error;
}

export function ConnectionBanner({ error }: { error: string }): ReactNode {
  return (
    <div className="connection-banner" role="status">
      <TriangleAlert className="ae-icon ae-err" />
      <span>
        <span className="ae-item">Live data unavailable.</span>{" "}
        <span className="ae-dim">{connectionMessage(error)}</span>
      </span>
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

// Active rows are running agents, so an in-limits row is "working" for its
// whole on-screen life — it does not gray out between model calls. Quiet
// agents live in the idle fold, which does not use this.
function tone(session: SessionView): "kill" | "warn" | "working" {
  if (session.alert === "kill") return "kill";
  if (session.alert === "warn") return "warn";
  return "working";
}

// The spend meter: a ruled ink line against the kill line, hairline marks at
// the warn gate and the kill edge. The fill takes a status ink only once a
// threshold is the signal — in limits it stays plain ink.
function SpendBar({ session, config }: { session: SessionView; config: ConfigView }): ReactNode {
  const fill = fillRatio(session.turn_tokens, config.kill_turn_tokens) * 100;
  const warnAt = warnRatio(config.warn_turn_tokens, config.kill_turn_tokens) * 100;
  const state = tone(session);
  const fillClass = state === "kill" ? " ae-err" : state === "warn" ? " ae-warn" : "";
  return (
    <div
      className="ae-meter spend"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={config.kill_turn_tokens}
      aria-valuenow={session.turn_tokens}
      aria-label={`${tokens(session.turn_tokens)} of ${tokens(config.kill_turn_tokens)} tokens this turn; warn at ${tokens(config.warn_turn_tokens)}`}
    >
      <span className={`ae-meter-fill${fillClass}`} style={{ width: `${fill}%` }} />
      <span className="ae-meter-mark" style={{ left: `${warnAt}%` }} title={`warn ${tokens(config.warn_turn_tokens)}`} />
      <span className="ae-meter-mark spend-kill" title={`kill ${tokens(config.kill_turn_tokens)}`} />
    </div>
  );
}

// Derives the glyph and word from the same `tone` the meter uses, so label and
// hue can never drift apart. The hue is on the glyph; the word is a tag.
function StatusChip({ session }: { session: SessionView }): ReactNode {
  if (session.acknowledged_until) return <span className="ae-tag ae-tag-bare">acknowledged</span>;
  const state = tone(session);
  if (state === "kill") {
    return (
      <span className="status-word">
        <OctagonX className="ae-icon ae-err" />
        <span className="ae-tag">over kill</span>
      </span>
    );
  }
  if (state === "warn") {
    return (
      <span className="status-word">
        <TriangleAlert className="ae-icon ae-warn" />
        <span className="ae-tag">over warn</span>
      </span>
    );
  }
  return (
    <span className="status-word">
      <Check className="ae-icon ae-ok" />
      <span className="ae-tag ae-tag-bare">working</span>
    </span>
  );
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
    <article className="row">
      <button type="button" className="row-head" onClick={() => onSelect(selected ? "" : session.key)}>
        <span className="row-top">
          <span className="row-id">
            <span className="ae-item">{session.project ?? session.id}</span>
            <span className="ae-chrome row-meta">
              {providerLabel(session.provider)} · {relativeTime(session.last_activity_at)}
            </span>
          </span>
          <span className="row-spend">
            <span className="ae-num ae-item">{tokens(session.turn_tokens)}</span>
            <StatusChip session={session} />
          </span>
        </span>
        <SpendBar session={session} config={config} />
      </button>
      {selected ? (
        <div className="row-detail ae-view">
          <p>{session.explanation}</p>
          <dl className="row-facts">
            <Fact label="THIS TURN" value={tokens(session.turn_tokens)} />
            <Fact label="TOTAL SPENT" value={tokens(session.total_tokens)} />
            <Fact label="MODEL CALLS" value={String(session.calls)} />
            <Fact label="LAST ACTIVITY" value={relativeTime(session.last_activity_at)} />
            {session.models.length ? <Fact label="MODEL" value={session.models.join(", ")} /> : null}
            {session.pid ? <Fact label="WORKER" value={`pid ${session.pid}`} /> : null}
          </dl>
          {detail ? <SelectedSessionPanel detail={detail} /> : <p className="row-busy">Loading session detail…</p>}
          {session.cwd ? <p className="row-cwd">{session.cwd}</p> : null}
          <SessionActionStrip session={session} onAck={onAck} onStop={onStop} />
          {busy ? (
            <p className="row-busy" role="status">
              {busy}
            </p>
          ) : null}
        </div>
      ) : null}
    </article>
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
    <section className="agents ae-group">
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
        <details className="ae-fold idle-fold">
          <summary>
            {idle.length} idle {idle.length === 1 ? "agent" : "agents"} — active in the last{" "}
            {Math.max(1, Math.round(config.usage_window_seconds / 60))} min, not spending now
          </summary>
          <div className="idle-list">
            {idle.map((session) => (
              <div className="idle-row" key={session.key}>
                <span>{session.project ?? session.id}</span>
                <span>{providerLabel(session.provider)}</span>
                <span className="ae-num">{tokens(session.turn_tokens)} last turn</span>
                <span className="ae-num">{relativeTime(session.last_activity_at)}</span>
              </div>
            ))}
          </div>
        </details>
      ) : null}
    </section>
  );
}

// The calm, good state: nothing is spending. The product's instrument at
// rest — a zero meter with the armed thresholds still marked — and an honest
// sentence about what Curb is watching.
function EmptyState({ config }: { config: ConfigView }): ReactNode {
  const enforce = modeFromConfig(config.mode) === "enforce";
  const warnAt = warnRatio(config.warn_turn_tokens, config.kill_turn_tokens) * 100;
  return (
    <div className="empty ae-empty">
      <div className="ae-meter" aria-hidden="true">
        <span className="ae-meter-fill" style={{ width: "0%" }} />
        <span className="ae-meter-mark" style={{ left: `${warnAt}%` }} />
        <span className="ae-meter-mark spend-kill" />
      </div>
      <p>
        <ShieldCheck className="ae-icon ae-ok" /> <span className="ae-item">Nothing spending right now</span>
      </p>
      <p className="ae-dim">
        Watching Codex and Claude Code. Curb warns over {commas(config.warn_turn_tokens)} tokens a
        turn{enforce ? ` and stops a runaway over ${commas(config.kill_turn_tokens)}` : ""}.
      </p>
    </div>
  );
}

