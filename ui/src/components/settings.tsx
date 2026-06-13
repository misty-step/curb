import { Check, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { commas, numberValue } from "../format";
import {
  type ConfigUpdate,
  type ConfigView,
  type Mode,
  type NotificationView,
  modeFromConfig,
  modeToConfig,
} from "../types";

interface SettingsProps {
  config: ConfigView;
  notifications: NotificationView;
  message: string;
  onSave: (update: ConfigUpdate) => void;
  onTestNotification: () => void;
}

// The settings drawer body: a form of lines, not boxes. Choice marks are the
// system's squares; limits are underline inputs; the save confirmation is a
// status line whose hue rides the glyph.
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
        <span className="ae-label">When an agent crosses the kill line</span>
        <ModeChoice mode={mode} onChange={(next) => onSave({ mode: modeToConfig(next) })} />
        {mode === "enforce" ? (
          <label className="ae-choice setting-check">
            <input
              type="checkbox"
              className="ae-check"
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
      <div className="setting">
        <div className="notify-row">
          <label className="ae-choice">
            <input
              type="checkbox"
              className="ae-switch"
              checked={config.local_notifications}
              onChange={(event) => onSave({ local_notifications: event.target.checked })}
            />
            Notify me
          </label>
          <button type="button" className="ae-button ae-button-quiet ae-button-compact" onClick={onTestNotification}>
            Test
          </button>
          <span className="note">
            {notifications.available ? null : <TriangleAlert className="ae-icon ae-warn" />}{" "}
            {notifications.message}
          </span>
        </div>
      </div>
      {message ? (
        <p className="settings-msg" role="status">
          <Check className="ae-icon ae-ok" /> {message}
        </p>
      ) : null}
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
      <label className="ae-label" htmlFor={id}>
        {label}
      </label>
      <div className="token-input">
        <input
          id={id}
          className="ae-input ae-num"
          type="text"
          inputMode="numeric"
          defaultValue={commas(value)}
          onBlur={(event) => {
            const parsed = numberValue(event.target.value.replace(/[^0-9]/g, ""));
            event.target.value = commas(parsed);
            onSave(parsed);
          }}
        />
        <span className="note">tokens / turn</span>
      </div>
    </div>
  );
}

function ModeChoice({ mode, onChange }: { mode: Mode; onChange: (mode: Mode) => void }): ReactNode {
  return (
    <div role="radiogroup" aria-label="Mode">
      <label className="ae-choice">
        <input
          type="radio"
          className="ae-radio"
          name="curb-mode"
          checked={mode === "watch"}
          onChange={() => onChange("watch")}
        />
        Warn only
      </label>
      <label className="ae-choice">
        <input
          type="radio"
          className="ae-radio"
          name="curb-mode"
          checked={mode === "enforce"}
          onChange={() => onChange("enforce")}
        />
        Stop runaways
      </label>
    </div>
  );
}
