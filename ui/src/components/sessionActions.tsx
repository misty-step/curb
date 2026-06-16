import { Check, OctagonX, X } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
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
  const [asking, setAsking] = useState(false);
  if (!session.can_acknowledge && !session.can_stop) return null;
  return (
    <div className="action-strip">
      {session.can_stop ? <StopIdentityChecks session={session} /> : null}
      <div className="row-actions">
        {session.can_acknowledge ? (
          <button type="button" className="ae-button ae-button-quiet ae-button-compact" onClick={() => onAck(session)}>
            Acknowledge
          </button>
        ) : null}
        {session.can_stop ? (
          <button type="button" className="ae-button ae-button-quiet ae-button-compact" onClick={() => setAsking(true)}>
            <OctagonX className="ae-icon ae-err" />
            Stop now
          </button>
        ) : null}
      </div>
      {asking ? (
        <StopDialog
          session={session}
          onCancel={() => setAsking(false)}
          onConfirm={() => {
            setAsking(false);
            onStop(session);
          }}
        />
      ) : null}
    </div>
  );
}

// The decision is asked in the panel costume: a modal dialog over a whisper
// of paper dim. Escape and the quiet button decline it.
function StopDialog({
  session,
  onCancel,
  onConfirm,
}: {
  session: SessionView;
  onCancel: () => void;
  onConfirm: () => void;
}): ReactNode {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
  }, []);
  return (
    <dialog className="ae-dialog" ref={ref} onClose={onCancel}>
      <p className="ae-dialog-title">Stop {session.project ?? session.id}?</p>
      <p className="ae-dim">
        Curb revalidates the worker's identity (PID, start time, owner, executable) and stops only that
        correlated process, after the grace period.
      </p>
      <div className="ae-dialog-acts">
        <button type="button" className="ae-button ae-button-quiet" onClick={onCancel}>
          Cancel
        </button>
        <button type="button" className="ae-button" onClick={onConfirm}>
          Confirm stop
        </button>
      </div>
    </dialog>
  );
}

// Identity checks: the hue rides each glyph; the words stay ink.
function StopIdentityChecks({ session }: { session: SessionView }): ReactNode {
  return (
    <span className="stop-checks" aria-label="Stop identity checks">
      <span>Stop requires</span>
      {stopIdentityChecks(session).map((check) => (
        <span className="stop-check" key={check.label}>
          {check.ready ? <Check className="ae-icon ae-ok" /> : <X className="ae-icon ae-err" />}
          {check.label}
        </span>
      ))}
    </span>
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
