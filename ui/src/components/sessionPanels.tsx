import type { ReactNode } from "react";
import { relativeTime, tokens } from "../format";
import type { ReadinessModel, RecoveryModel, SelectedSessionExplanation } from "../readModel";

export function SelectedSessionPanel({ detail }: { detail: SelectedSessionExplanation }): ReactNode {
  return (
    <div className="session-panel">
      <section className="evidence-block">
        <h3>Alert &amp; correlation evidence</h3>
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
        <h3>Turn timeline</h3>
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

export function ReadinessPanel({ model }: { model: ReadinessModel }): ReactNode {
  return (
    <section className={`readiness ${model.attention ? "readiness-attention" : ""}`}>
      <div className="readiness-head">
        <div>
          <span>Setup</span>
          <strong>{model.summary}</strong>
          <p>{model.nextStep}</p>
        </div>
        <strong className={`readiness-status readiness-status-${model.primary.tone}`}>{readinessStatus(model.primary.status)}</strong>
      </div>
      <details className="readiness-details">
        <summary>
          <span>Diagnostics</span>
          <em>Optional</em>
        </summary>
        <div className="readiness-list">
          {model.details.map((item) => (
            <div className={`readiness-item readiness-item-${item.tone}`} key={item.label}>
              <span className="readiness-dot" aria-hidden="true" />
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
    <section className="recovery">
      <div className="recovery-head">
        <div>
          <span>Recovery</span>
          <strong>{model.summary}</strong>
          <p>{model.nextStep}</p>
        </div>
      </div>
      <div className="recovery-list">
        {model.items.map((item) => (
          <article className="recovery-item" key={item.id}>
            <div className="recovery-copy">
              <div className="recovery-title">
                <strong>{item.label}</strong>
                <em>{item.status}</em>
              </div>
              <p>{item.message}</p>
              <p>{item.action}</p>
            </div>
            <div className="recovery-facts">
              {item.command ? (
                <span>
                  <b>Command</b>
                  <code>{item.command}</code>
                </span>
              ) : null}
              {item.path ? (
                <span>
                  <b>Path</b>
                  <code>{item.path}</code>
                </span>
              ) : null}
              {item.runbook ? (
                <span>
                  <b>Runbook</b>
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
