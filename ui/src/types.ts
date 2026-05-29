// The Curb read model, mirrored from the Rust service. Every view is built by
// the daemon; the UI only renders it. Three facts describe each agent:
// turn_tokens (spend since your last input), status, and alert.

export type Status = "OK" | "WATCH" | "ACTION";
export type AlertLevel = "ok" | "warn" | "kill";
export type AgentStatus = "working" | "idle";

export interface SourceHealth {
  provider: string;
  files: number;
  events: number;
  error?: string;
}

export interface CapabilityView {
  available: boolean;
  status: string;
  message: string;
}

export interface PlatformCapabilities {
  platform: string;
  notifications: CapabilityView;
  process_capture: CapabilityView;
  process_identity: CapabilityView;
  enforcement: CapabilityView;
}

export interface OverviewDelta {
  new_sessions: number;
  sessions_with_new_turns: number;
  tokens_added: number;
  new_alerts: number;
  agents_started: number;
  agents_ended: number;
  source_errors: number;
}

export interface Overview {
  mode: string; // "watch" | "enforce"
  status: Status;
  message: string;
  working: number;
  warn: number;
  kill: number;
  busiest_turn_tokens: number;
  last_scan: string;
  sources: SourceHealth[];
  changes: OverviewDelta;
  capabilities: PlatformCapabilities;
}

export interface SessionView {
  key: string;
  id: string;
  provider: string;
  status: AgentStatus;
  alert: AlertLevel;
  can_stop: boolean;
  can_acknowledge: boolean;
  acknowledged_until?: string;
  project?: string;
  cwd?: string;
  models: string[];
  turn_tokens: number;
  turn_context_tokens: number;
  total_tokens: number;
  calls: number;
  last_activity_at?: string;
  pid?: number;
  process_started_at?: string;
  owner?: string;
  executable?: string;
  bundle_id?: string;
  team_id?: string;
  explanation: string;
}

export interface AgentView {
  id: string;
  provider: string;
  label: string;
  status: AgentStatus;
  pid: number;
  process_started_at?: string;
  running_for_seconds?: number;
  project?: string;
  cwd?: string;
  session_key?: string;
  turn_tokens: number;
  explanation: string;
}

export interface TurnView {
  session_key?: string;
  session_id?: string;
  provider: string;
  at?: string;
  model?: string;
  total_tokens: number;
  spent_tokens: number;
}

export interface Snapshot {
  overview: Overview;
  agents: AgentView[];
  sessions: SessionView[];
  turns: TurnView[];
}

export interface ConfigView {
  path?: string;
  mode: string; // raw config mode: "visibility" | "alert" | "enforcement"
  usage_enabled: boolean;
  warn_turn_tokens: number;
  kill_turn_tokens: number;
  usage_window_seconds: number;
  usage_scan_seconds: number;
  lookback_seconds: number;
  process_warn_seconds: number;
  process_kill_seconds: number;
  ack_extension_seconds: number;
  local_notifications: boolean;
  agents: ConfigAgentView[];
}

export interface ConfigAgentView {
  id: string;
  label: string;
  family: string;
  kind: string;
  terminates: boolean;
  description: string;
}

export type ConfigUpdate = Partial<
  Pick<
    ConfigView,
    "mode" | "warn_turn_tokens" | "kill_turn_tokens" | "local_notifications"
  >
>;

export interface NotificationView {
  enabled: boolean;
  available: boolean;
  status: string;
  message: string;
  last_test_at?: string;
  last_error?: string;
}

export interface AckView {
  session_key: string;
  extend_seconds: number;
  until: string;
  reason?: string;
}

export interface StopExpectedIdentity {
  pid: number;
  started_at: string;
  owner?: string;
  executable?: string;
  bundle_id?: string;
  team_id?: string;
}

export interface StopView {
  session_key: string;
  pid: number;
  scope_pids: number[];
  result: {
    soft_signaled?: number[];
    hard_signaled?: number[];
    gone?: number[];
    errors?: string[];
  };
}

/** Watch = warn only. Enforce = warn, then stop runaways. */
export type Mode = "watch" | "enforce";

export function modeFromConfig(raw: string): Mode {
  return raw === "enforcement" ? "enforce" : "watch";
}

export function modeToConfig(mode: Mode): string {
  return mode === "enforce" ? "enforcement" : "alert";
}
