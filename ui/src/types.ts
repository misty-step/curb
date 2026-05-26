export type Status = "OK" | "ACTIVE" | "WATCH" | "ACTION";

export interface SourceHealth {
  provider: string;
  files: number;
  events: number;
  error?: string;
}

export interface Overview {
  mode: string;
  action: string;
  status: Status;
  message: string;
  active_agents: number;
  active_sessions: number;
  warning_sessions: number;
  stop_sessions: number;
  idle_high_sessions: number;
  window_tokens: number;
  lookback_tokens: number;
  last_scan: string;
  sources: SourceHealth[];
  changes: OverviewDelta;
  capabilities: PlatformCapabilities;
}

export interface PlatformCapabilities {
  platform: string;
  notifications: CapabilityView;
  process_capture: CapabilityView;
  process_identity: CapabilityView;
  enforcement: CapabilityView;
}

export interface CapabilityView {
  available: boolean;
  status: string;
  message: string;
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

export interface AgentView {
  id: string;
  provider: string;
  label: string;
  state: string;
  process_state: string;
  usage_state?: string;
  action_state: string;
  actionable: boolean;
  pid: number;
  process_started_at?: string;
  running_for_seconds?: number;
  project?: string;
  cwd?: string;
  matched_by?: string[];
  confidence: number;
  cpu_percent?: number;
  latest_session_id?: string;
  latest_turn_tokens?: number;
  window_tokens?: number;
  explanation: string;
}

export interface SessionView {
  key: string;
  id?: string;
  provider: string;
  state: string;
  agent_state?: string;
  process_state: string;
  usage_state?: string;
  action_state: string;
  actionable: boolean;
  can_acknowledge: boolean;
  project?: string;
  cwd?: string;
  models?: string[];
  last_seen_at: string;
  last_usage_at?: string;
  calls: number;
  latest_turn_tokens?: number;
  window_tokens?: number;
  total_tokens: number;
  correlated_agent_id?: string;
  correlated_pid?: number;
  correlated_process_started_at?: string;
  correlated_owner?: string;
  correlated_executable?: string;
  correlated_bundle_id?: string;
  correlated_team_id?: string;
  correlation_reason?: string;
  correlation_score?: number;
  confidence?: number;
  matched_by?: string[];
  risk_rank: number;
  acknowledged: boolean;
  acknowledged_until?: string;
  explanation: string;
}

export interface TurnView {
  id?: string;
  request_id?: string;
  session_key?: string;
  session_id?: string;
  provider: string;
  at: string;
  model?: string;
  input_tokens?: number;
  cached_input_tokens?: number;
  cache_creation_input_tokens?: number;
  output_tokens?: number;
  reasoning_output_tokens?: number;
  total_tokens?: number;
  cumulative_tokens?: number;
  source?: string;
}

export interface Snapshot {
  overview: Overview;
  agents: AgentView[];
  sessions: SessionView[];
  turns: TurnView[];
}

export interface ConfigAgentView {
  id: string;
  label: string;
  family: string;
  kind: string;
  terminates: boolean;
  description: string;
}

export interface ConfigView {
  path?: string;
  mode: string;
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

export type ConfigUpdate = Partial<Omit<ConfigView, "path" | "agents">>;

export interface NotificationView {
  enabled: boolean;
  available: boolean;
  status: string;
  message: string;
  last_test_at?: string;
  last_error?: string;
}

export interface OnboardingView {
  required: boolean;
  config_path?: string;
  mode: string;
  action: string;
  mode_can_terminate: boolean;
  detected_providers: string[];
  detected_workers: string[];
  enforceable_agent_types: number;
  watch_only_agent_types: number;
  notifications: NotificationView;
  capabilities: PlatformCapabilities;
  sources: SourceHealth[];
  final_sentence: string;
  steps: OnboardingStepView[];
}

export interface OnboardingStepView {
  id: string;
  label: string;
  status: string;
  message: string;
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
  agent_id: string;
  pid: number;
  started_at: string;
  owner?: string;
  executable?: string;
  bundle_id?: string;
  team_id?: string;
  scope: string;
  scope_pids: number[];
  result: {
    soft_signaled?: number[];
    hard_signaled?: number[];
    gone?: number[];
    errors?: string[];
  };
}

export interface AlertView {
  severity: string;
  label: string;
  category: string;
  message: string;
  at: string;
  seq: number;
  run_id?: string;
  agent_id?: string;
  provider?: string;
  mode?: string;
  cwd?: string;
  session_key?: string;
  session_id?: string;
  actionable: boolean;
  can_acknowledge: boolean;
  explanation: string;
}
