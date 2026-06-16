import { Check, Minus, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { relativeTime, tokens } from "../format";
import type { ReadinessModel, RecoveryModel, SelectedSessionExplanation } from "../readModel";

export function SelectedSessionPanel({ detail }: { detail: SelectedSessionExplanation }): ReactNode {
  return (
    <div className="session-panel">
      <section className="evidence-block">
        <h3 className="ae-plate-cap">ALERT &amp; CORRELATION EVIDENCE</h3>
        <div className="evidence-grid">
          {[...detail.actionEvidence, ...detail.correlationEvidence].map((entry) => (
            <div className="evidence-item" key={`${entry.label}:${entry.value}`}>
              <span>{entry.label} </span>
              <strong>{entry.value}</strong>
            </div>
          ))}
        </div>
      </section>
      <section className="evidence-block">
        <h3 className="ae-plate-cap">TURN TIMELINE</h3>
        {detail.turns.length ? (
          <ol className="turn-list">
            {detail.turns.map((turn) => (
              <li className="turn-item" key={`${turn.label}:${turn.at ?? ""}`}>
                <div className="turn-head">
                  <span>{turn.label}</span>
                  <span>
                    {turn.model ?? turn.provider} · {relativeTime(turn.at)}
                  </span>
                </div>
                <div className="turn-breakdown">
                  <span>Input {tokens(turn.inputTokens)}</span>
                  <span>Cached {tokens(turn.cachedInputTokens)}</span>
                  <span>Created {tokens(turn.cacheCreationTokens)}</span>
                  <span>Output {tokens(turn.outputTokens)}</span>
                  <span>Reasoning {tokens(turn.reasoningTokens)}</span>
                  <span>Total {tokens(turn.totalTokens)}</span>
                  <span>Spent {tokens(turn.spentTokens)}</span>
                  <span>Cumulative {tokens(turn.cumulativeTokens)}</span>
                  <span>Source {turn.source}</span>
                </div>
              </li>
            ))}
          </ol>
        ) : (
          <p className="row-busy">No turn records returned for this session yet.</p>
        )}
      </section>
    </div>
  );
}

// Tone → glyph: the hue rides the icon, the words stay ink.
function ToneGlyph({ tone }: { tone: "ok" | "attention" | "warn" | "muted" }): ReactNode {
  if (tone === "ok") return <Check className="ae-icon ae-ok" />;
  if (tone === "muted") return <Minus className="ae-icon" />;
  return <TriangleAlert className="ae-icon ae-warn" />;
}

export function ReadinessPanel({ model }: { model: ReadinessModel }): ReactNode {
  return (
    <section className="readiness ae-group">
      <div className="panel-head">
        <div>
          <h2 className="ae-h">SETUP</h2>
          <p className="ae-item">{model.summary}</p>
          <p className="ae-dim">{model.nextStep}</p>
        </div>
        <span className="status-word readiness-status">
          <ToneGlyph tone={model.primary.tone} />
          <span className="ae-tag">{readinessStatus(model.primary.status)}</span>
        </span>
      </div>
      <details className="ae-fold readiness-details">
        <summary>
          Diagnostics <span className="ae-setting-val">Optional</span>
        </summary>
        <div className="readiness-list">
          {model.details.map((item) => (
            <div className="readiness-item" key={item.label}>
              <ToneGlyph tone={item.tone} />
              <div className="readiness-copy">
                <span>{item.label}</span>
                <em>{item.message}</em>
              </div>
              <strong>{item.status}</strong>
            </div>
          ))}
        </div>
      </details>
    </section>
  );
}

export function RecoveryPanel({ model }: { model: RecoveryModel }): ReactNode {
  if (!model.attention) return null;
  return (
    <section className="recovery ae-group">
      <h2 className="ae-h">RECOVERY</h2>
      <p className="ae-item">{model.summary}</p>
      <p className="ae-dim">{model.nextStep}</p>
      <div className="recovery-list">
        {model.items.map((item) => (
          <article className="recovery-item" key={item.id}>
            <div className="recovery-copy">
              <p className="recovery-title">
                <TriangleAlert className="ae-icon ae-warn" />
                <span className="ae-item">{item.label}</span>
                <span className="ae-tag">{item.status}</span>
              </p>
              <p>{item.message}</p>
              <p>{item.action}</p>
            </div>
            <div className="recovery-facts">
              {item.command ? (
                <span>
                  <b>COMMAND</b>
                  <code>{item.command}</code>
                </span>
              ) : null}
              {item.path ? (
                <span>
                  <b>PATH</b>
                  <code>{item.path}</code>
                </span>
              ) : null}
              {item.runbook ? (
                <span>
                  <b>RUNBOOK</b>
                  <code>{item.runbook}</code>
                </span>
              ) : null}
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

function readinessStatus(status: string): string {
  if (status === "required" || status === "ready") return "OK";
  return status;
}
