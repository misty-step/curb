import type { ReactNode } from "react";
import { relativeTime, tokens } from "../format";
import type { ReadinessModel, SelectedSessionExplanation } from "../readModel";

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
        <span>Readiness</span>
        <strong>{model.summary}</strong>
      </div>
      <div className="readiness-grid">
        {model.items.map((item) => (
          <div className="readiness-item" key={item.label}>
            <span>{item.label}</span>
            <strong>{item.status}</strong>
            <em>{item.message}</em>
          </div>
        ))}
      </div>
    </section>
  );
}
