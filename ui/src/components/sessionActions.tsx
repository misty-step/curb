import { OctagonX } from "lucide-react";
import { useState, type ReactNode } from "react";
import type { SessionView } from "../types";

export function SessionActionStrip({
  session,
  onAck,
  onStop,
}: {
  session: SessionView;
  onAck: (session: SessionView) => void;
  onStop: (session: SessionView) => void;
}): ReactNode {
  const [armed, setArmed] = useState(false);
  if (!session.can_acknowledge && !session.can_stop) return null;
  return (
    <div className="action-strip">
      {session.can_stop ? <StopIdentityChecks session={session} /> : null}
      <div className="row-actions">
        {session.can_acknowledge ? (
          <button type="button" className="btn btn-ack" onClick={() => onAck(session)}>
            Acknowledge
          </button>
        ) : null}
        {session.can_stop ? (
          <StopActionButton
            armed={armed}
            onArm={() => setArmed(true)}
            onCancel={() => setArmed(false)}
            onConfirm={() => onStop(session)}
          />
        ) : null}
      </div>
    </div>
  );
}

function StopActionButton({
  armed,
  onArm,
  onCancel,
  onConfirm,
}: {
  armed: boolean;
  onArm: () => void;
  onCancel: () => void;
  onConfirm: () => void;
}): ReactNode {
  if (!armed) {
    return (
      <button type="button" className="btn btn-stop" onClick={onArm}>
        <OctagonX size={14} />
        Stop now
      </button>
    );
  }
  return (
    <span className="stop-confirm">
      <button type="button" className="btn btn-ghost" onClick={onCancel}>
        Cancel
      </button>
      <button type="button" className="btn btn-stop btn-stop-confirm" onClick={onConfirm}>
        <OctagonX size={14} />
        Confirm stop
      </button>
    </span>
  );
}

function StopIdentityChecks({ session }: { session: SessionView }): ReactNode {
  return (
    <div className="stop-checks" aria-label="Stop identity checks">
      <span className="stop-check-title">Stop requires</span>
      {stopIdentityChecks(session).map((check) => (
        <span className={`stop-check ${check.ready ? "ready" : "missing"}`} key={check.label}>
          {check.label}
        </span>
      ))}
    </div>
  );
}

function stopIdentityChecks(session: SessionView): Array<{ label: string; ready: boolean }> {
  return [
    { label: "PID", ready: Boolean(session.pid) },
    { label: "start time", ready: Boolean(session.process_started_at) },
    { label: "owner", ready: Boolean(session.owner) },
    { label: "executable", ready: Boolean(session.executable || session.bundle_id || session.team_id) },
  ];
}
