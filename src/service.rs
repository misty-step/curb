use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::{Agent, Config, HumanDuration, Mode};
use crate::ledger::{self, Ledger};
use crate::platform::{self, NotificationCapability, Platform, TerminationCapability};
use crate::usage::{Event, SourceReport};

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("session not found")]
    SessionNotFound,
    #[error("invalid acknowledgement: {0}")]
    InvalidAck(String),
    #[error("invalid stop request: {0}")]
    InvalidStop(String),
    #[error("invalid config update: {0}")]
    InvalidConfig(String),
    #[error("session cannot be stopped safely: {0}")]
    StopConflict(String),
    #[error("service io {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("service json {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error(transparent)]
    Ledger(#[from] ledger::LedgerError),
    #[error(transparent)]
    Platform(#[from] platform::PlatformError),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub overview: Overview,
    pub agents: Vec<AgentView>,
    pub sessions: Vec<SessionView>,
    pub turns: Vec<TurnView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Overview {
    pub mode: String,
    pub action: String,
    pub status: String,
    pub message: String,
    pub active_agents: usize,
    pub active_sessions: usize,
    pub warning_sessions: usize,
    pub stop_sessions: usize,
    pub idle_high_sessions: usize,
    pub window_tokens: i64,
    pub lookback_tokens: i64,
    pub last_scan: DateTime<Utc>,
    pub sources: Vec<SourceReport>,
    pub changes: OverviewDelta,
    pub capabilities: PlatformCapabilities,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct OverviewDelta {
    pub new_sessions: usize,
    pub sessions_with_new_turns: usize,
    pub tokens_added: i64,
    pub new_alerts: usize,
    pub agents_started: usize,
    pub agents_ended: usize,
    pub source_errors: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionView {
    pub key: String,
    pub id: String,
    pub provider: String,
    pub state: String,
    pub process_state: String,
    pub usage_state: String,
    pub action_state: String,
    pub actionable: bool,
    pub can_acknowledge: bool,
    pub acknowledged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledged_until: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub cwd: Option<PathBuf>,
    pub models: Vec<String>,
    pub last_seen_at: DateTime<Utc>,
    pub last_usage_at: Option<DateTime<Utc>>,
    pub calls: usize,
    pub latest_turn_tokens: i64,
    pub window_tokens: i64,
    pub total_tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlated_agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlated_pid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlated_process_started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlated_owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlated_executable: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlated_bundle_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlated_team_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_reason: Option<String>,
    pub correlation_score: i64,
    pub confidence: i64,
    pub matched_by: Vec<String>,
    pub risk_rank: i64,
    pub explanation: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentView {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub state: String,
    pub process_state: String,
    pub usage_state: String,
    pub action_state: String,
    pub actionable: bool,
    pub pid: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running_for_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    pub matched_by: Vec<String>,
    pub confidence: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_session_id: Option<String>,
    pub latest_turn_tokens: i64,
    pub window_tokens: i64,
    pub explanation: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TurnView {
    pub id: Option<String>,
    pub request_id: Option<String>,
    pub session_key: String,
    pub session_id: Option<String>,
    pub provider: String,
    pub at: Option<DateTime<Utc>>,
    pub model: Option<String>,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
    pub cumulative_tokens: i64,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventView {
    pub seq: i64,
    pub at: DateTime<Utc>,
    pub category: String,
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AlertView {
    pub severity: String,
    pub label: String,
    pub category: String,
    pub message: String,
    pub at: DateTime<Utc>,
    pub seq: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub actionable: bool,
    pub can_acknowledge: bool,
    pub explanation: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AckRequest {
    pub extend_seconds: i64,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AckView {
    pub session_key: String,
    pub extend_seconds: i64,
    pub until: DateTime<Utc>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionAck {
    pub session_key: String,
    pub reason: String,
    pub until: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StopRequest {
    pub confirm: bool,
    pub scope: String,
    pub reason: String,
    pub expected: StopExpectedIdentity,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct StopExpectedIdentity {
    pub pid: i32,
    pub started_at: Option<DateTime<Utc>>,
    pub owner: String,
    pub executable: Option<PathBuf>,
    pub bundle_id: Option<String>,
    pub team_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StopView {
    pub session_key: String,
    pub agent_id: String,
    pub pid: i32,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub owner: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    pub scope: String,
    pub scope_pids: Vec<i32>,
    pub result: platform::TerminationResult,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NotificationView {
    pub enabled: bool,
    pub available: bool,
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_test_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilityView {
    pub available: bool,
    pub status: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlatformCapabilities {
    pub platform: String,
    pub notifications: CapabilityView,
    pub process_capture: CapabilityView,
    pub process_identity: CapabilityView,
    pub enforcement: CapabilityView,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OnboardingView {
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    pub mode: String,
    pub action: String,
    pub mode_can_terminate: bool,
    pub detected_providers: Vec<String>,
    pub detected_workers: Vec<String>,
    pub enforceable_agent_types: usize,
    pub watch_only_agent_types: usize,
    pub notifications: NotificationView,
    pub capabilities: PlatformCapabilities,
    pub sources: Vec<SourceReport>,
    pub final_sentence: String,
    pub steps: Vec<OnboardingStepView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OnboardingStepView {
    pub id: String,
    pub label: String,
    pub status: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub mode: String,
    pub usage_enabled: bool,
    pub warn_turn_tokens: i64,
    pub kill_turn_tokens: i64,
    pub usage_window_seconds: i64,
    pub usage_scan_seconds: i64,
    pub lookback_seconds: i64,
    pub process_warn_seconds: i64,
    pub process_kill_seconds: i64,
    pub ack_extension_seconds: i64,
    pub local_notifications: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ledger_forward_url: Option<String>,
    pub agents: Vec<ConfigAgentView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigAgentView {
    pub id: String,
    pub label: String,
    pub family: String,
    pub kind: String,
    pub terminates: bool,
    pub description: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(default)]
pub struct ConfigUpdate {
    pub mode: Option<String>,
    pub usage_enabled: Option<bool>,
    pub warn_turn_tokens: Option<i64>,
    pub kill_turn_tokens: Option<i64>,
    pub usage_window_seconds: Option<i64>,
    pub usage_scan_seconds: Option<i64>,
    pub lookback_seconds: Option<i64>,
    pub process_warn_seconds: Option<i64>,
    pub process_kill_seconds: Option<i64>,
    pub local_notifications: Option<bool>,
}

pub fn config_view(path: Option<&Path>, cfg: &Config) -> ConfigView {
    ConfigView {
        path: path.map(|path| path.display().to_string()),
        mode: cfg.mode.to_string(),
        usage_enabled: cfg.usage.enabled(),
        warn_turn_tokens: cfg.usage.warn_turn_tokens,
        kill_turn_tokens: cfg.usage.kill_turn_tokens,
        usage_window_seconds: seconds(cfg.usage.window),
        usage_scan_seconds: seconds(cfg.usage.scan_interval),
        lookback_seconds: seconds(cfg.usage.lookback),
        process_warn_seconds: seconds(cfg.defaults.warn_after),
        process_kill_seconds: seconds(cfg.defaults.kill_after),
        ack_extension_seconds: seconds(cfg.defaults.ack_extension),
        local_notifications: cfg.alerts.local_notifications,
        ledger_forward_url: (!cfg.ledger.forward_url.is_empty())
            .then(|| cfg.ledger.forward_url.clone()),
        agents: cfg
            .agents
            .iter()
            .map(|agent| {
                let terminates = agent.termination_allowed();
                ConfigAgentView {
                    id: agent.id.clone(),
                    label: agent.label.clone(),
                    family: agent.family.clone(),
                    kind: agent_kind(agent, terminates).to_string(),
                    terminates,
                    description: if terminates {
                        "worker process; eligible for enforcement".to_string()
                    } else {
                        "app or shell process; visibility only".to_string()
                    },
                }
            })
            .collect(),
    }
}

pub fn apply_config_update(cfg: &mut Config, update: ConfigUpdate) -> Result<(), ServiceError> {
    if let Some(mode) = update.mode {
        cfg.mode = parse_mode(&mode)?;
    }
    if let Some(enabled) = update.usage_enabled {
        cfg.usage.enabled = Some(enabled);
    }
    if let Some(value) = update.warn_turn_tokens {
        cfg.usage.warn_turn_tokens = value;
    }
    if let Some(value) = update.kill_turn_tokens {
        cfg.usage.kill_turn_tokens = value;
    }
    if let Some(value) = update.usage_window_seconds {
        cfg.usage.window = positive_seconds("usage_window_seconds", value)?;
    }
    if let Some(value) = update.usage_scan_seconds {
        cfg.usage.scan_interval = positive_seconds("usage_scan_seconds", value)?;
    }
    if let Some(value) = update.lookback_seconds {
        cfg.usage.lookback = positive_seconds("lookback_seconds", value)?;
    }
    if let Some(value) = update.process_warn_seconds {
        cfg.defaults.warn_after = positive_seconds("process_warn_seconds", value)?;
    }
    if let Some(value) = update.process_kill_seconds {
        cfg.defaults.kill_after = positive_seconds("process_kill_seconds", value)?;
    }
    if let Some(enabled) = update.local_notifications {
        cfg.alerts.local_notifications = enabled;
    }
    cfg.validate()
        .map_err(|error| ServiceError::InvalidConfig(error.to_string()))
}

fn seconds(duration: HumanDuration) -> i64 {
    i64::try_from(duration.as_std().as_secs()).unwrap_or(i64::MAX)
}

fn positive_seconds(field: &'static str, value: i64) -> Result<HumanDuration, ServiceError> {
    if value <= 0 {
        return Err(ServiceError::InvalidConfig(format!(
            "{field} must be positive"
        )));
    }
    Ok(HumanDuration::seconds(value as u64))
}

fn parse_mode(mode: &str) -> Result<Mode, ServiceError> {
    match mode {
        "visibility" => Ok(Mode::Visibility),
        "alert" => Ok(Mode::Alert),
        "enforcement" => Ok(Mode::Enforcement),
        other => Err(ServiceError::InvalidConfig(format!(
            "invalid mode {other:?}"
        ))),
    }
}

fn agent_kind(agent: &Agent, terminates: bool) -> &'static str {
    match agent.kind {
        crate::config::AgentKind::Process => "process",
        crate::config::AgentKind::App => "app",
        crate::config::AgentKind::Unspecified if terminates => "process",
        crate::config::AgentKind::Unspecified => "app",
    }
}

pub fn notification_view(
    enabled: bool,
    capability: NotificationCapability,
    last: Option<NotificationView>,
) -> NotificationView {
    let mut view = new_notification_view(enabled, capability);
    if let Some(last) = last {
        view.last_test_at = last.last_test_at;
        view.last_error = last.last_error;
        if view.enabled && view.available && matches!(last.status.as_str(), "delivered" | "error") {
            view.status = last.status;
            view.message = last.message;
            view.available = last.available;
        }
    }
    view
}

fn new_notification_view(enabled: bool, capability: NotificationCapability) -> NotificationView {
    let mut status = capability.status;
    if status == "available" {
        status = "ready".to_string();
    }
    let mut view = NotificationView {
        enabled,
        available: enabled && capability.supported,
        status,
        message: capability.message,
        last_test_at: None,
        last_error: None,
    };
    if !enabled {
        view.status = "disabled".to_string();
        view.message = "local notifications are disabled in Curb policy".to_string();
        view.available = false;
    } else if !capability.supported {
        view.status = "unavailable".to_string();
    }
    view
}

pub fn onboarding_view(
    config: ConfigView,
    required: bool,
    notifications: NotificationView,
    termination: TerminationCapability,
    snapshot: Snapshot,
) -> OnboardingView {
    let enforceable_agent_types = config
        .agents
        .iter()
        .filter(|agent| agent.terminates)
        .count();
    let watch_only_agent_types = config.agents.len().saturating_sub(enforceable_agent_types);
    let capabilities = onboarding_capabilities(
        &config,
        &notifications,
        &termination,
        &snapshot,
        enforceable_agent_types,
    );
    let mode_can_terminate = config.mode == "enforcement"
        && enforceable_agent_types > 0
        && capabilities.enforcement.available;
    let steps = vec![
        config_step(&config),
        agent_step(&config),
        source_step(&snapshot.overview.sources, &capabilities.process_capture),
        notification_step(&config.mode, &notifications),
        safety_step(&config),
    ];
    OnboardingView {
        required,
        config_path: config.path.clone(),
        mode: config.mode.clone(),
        action: action_label(&config.mode),
        mode_can_terminate,
        detected_providers: detected_providers(&snapshot),
        detected_workers: detected_workers(&snapshot),
        enforceable_agent_types,
        watch_only_agent_types,
        notifications,
        capabilities,
        sources: snapshot.overview.sources,
        final_sentence: onboarding_final_sentence(&config.mode),
        steps,
    }
}

pub fn platform_capabilities(
    cfg: &Config,
    processes: Option<&platform::Snapshot>,
    capture_error: Option<&platform::PlatformError>,
    notifications: NotificationView,
    termination: TerminationCapability,
    agents: &[AgentView],
) -> PlatformCapabilities {
    PlatformCapabilities {
        platform: std::env::consts::OS.to_string(),
        notifications: notification_capability_view(&notifications),
        process_capture: process_capture_capability_from_platform(processes, capture_error),
        process_identity: process_identity_capability_from_platform(processes, capture_error),
        enforcement: platform_enforcement_capability(
            cfg,
            processes,
            capture_error,
            &termination,
            agents,
        ),
    }
}

pub fn annotate_overview_delta(previous: Option<&Snapshot>, mut next: Snapshot) -> Snapshot {
    next.overview.changes = previous
        .map(|previous| build_overview_delta(previous, &next))
        .unwrap_or_default();
    next
}

pub fn event_views(events: &[ledger::Event], limit: usize) -> Vec<EventView> {
    recent_events(events, limit)
        .into_iter()
        .map(new_event_view)
        .collect()
}

pub fn alert_views(
    events: &[ledger::Event],
    snapshot: Option<&Snapshot>,
    limit: usize,
) -> Vec<AlertView> {
    let sessions = snapshot.map(session_index).unwrap_or_default();
    let mut alerts = Vec::new();
    for event in events.iter().rev() {
        if alerts.len() == effective_limit(limit, events.len()) {
            break;
        }
        let Some(mut alert) = new_alert_view(event) else {
            continue;
        };
        project_alert_action(&mut alert, &sessions);
        alerts.push(alert);
    }
    alerts.reverse();
    alerts
}

fn recent_events(events: &[ledger::Event], limit: usize) -> Vec<&ledger::Event> {
    let limit = effective_limit(limit, events.len());
    if limit >= events.len() {
        events.iter().collect()
    } else {
        events[events.len() - limit..].iter().collect()
    }
}

fn effective_limit(limit: usize, len: usize) -> usize {
    if limit == 0 { len } else { limit.min(len) }
}

fn new_event_view(event: &ledger::Event) -> EventView {
    let (category, kind) = event_class(&event.event_type);
    EventView {
        seq: event.seq,
        at: event.ts,
        category: category.to_string(),
        kind: kind.to_string(),
        message: event
            .message
            .clone()
            .unwrap_or_else(|| default_event_message(category, kind)),
        run_id: event.run_id.clone(),
        agent_id: event.agent_id.clone(),
        mode: event.mode.clone(),
    }
}

fn new_alert_view(event: &ledger::Event) -> Option<AlertView> {
    if !alert_event(&event.event_type) {
        return None;
    }
    let category = alert_category(&event.event_type);
    Some(AlertView {
        severity: alert_severity(event).to_string(),
        label: alert_label(&event.event_type).to_string(),
        category: category.to_string(),
        message: event
            .message
            .clone()
            .unwrap_or_else(|| default_alert_message(category).to_string()),
        at: event.ts,
        seq: event.seq,
        run_id: event.run_id.clone(),
        agent_id: event.agent_id.clone(),
        provider: string_data(event, "provider"),
        mode: event.mode.clone(),
        cwd: string_data(event, "cwd"),
        session_key: None,
        session_id: string_data(event, "session_id"),
        actionable: actionable_event(event),
        can_acknowledge: false,
        explanation: alert_explanation(&event.event_type).to_string(),
    })
}

fn event_class(event_type: &str) -> (&'static str, &'static str) {
    match event_type {
        "service_started" => ("service", "started"),
        "service_stopped" => ("service", "stopped"),
        "run_started" => ("run", "started"),
        "run_stopped" => ("run", "stopped"),
        "ack_received" | "session_ack_received" => ("ack", "received"),
        "ack_rejected" => ("ack", "rejected"),
        "policy_warning" | "usage_warning" => ("alert", "warning"),
        "usage_would_terminate" => ("alert", "would_stop"),
        "usage_kill_blocked" => ("alert", "blocked"),
        "usage_grace_started" => ("alert", "grace"),
        "usage_termination_started" | "termination_started" => ("termination", "started"),
        "usage_termination_completed" | "termination_completed" => ("termination", "completed"),
        "usage_termination_failed" | "termination_failed" => ("termination", "failed"),
        "scan_failed" | "usage_scan_failed" => ("error", "scan_failed"),
        "notification_failed" => ("error", "notification_failed"),
        _ => ("other", "recorded"),
    }
}

fn default_event_message(category: &str, kind: &str) -> String {
    match category {
        "service" => format!("Curb service {kind}."),
        "run" => format!("Agent run {kind}."),
        "ack" => format!("Acknowledgement {kind}."),
        "alert" => "Policy alert recorded.".to_string(),
        "termination" => format!("Termination {kind}."),
        "error" => "Curb recorded an error.".to_string(),
        _ => "Curb recorded an event.".to_string(),
    }
}

fn alert_event(event_type: &str) -> bool {
    event_type.contains("warning")
        || event_type.contains("terminate")
        || event_type.contains("termination")
        || event_type.contains("kill")
        || event_type.contains("grace")
}

fn alert_category(event_type: &str) -> &'static str {
    if event_type.contains("completed") {
        "stopped"
    } else if event_type.contains("started") || event_type.contains("grace") {
        "grace"
    } else if event_type.contains("would") {
        "would_stop"
    } else if event_type.contains("blocked") {
        "blocked"
    } else if event_type.contains("failed") {
        "failed"
    } else {
        "warning"
    }
}

fn alert_severity(event: &ledger::Event) -> &'static str {
    if event.event_type == "usage_termination_completed" {
        "stop"
    } else if event.event_type.contains("failed") {
        "error"
    } else if event.event_type.contains("blocked") {
        "blocked"
    } else if event.event_type.contains("would") || event.event_type.contains("grace") {
        "watch"
    } else {
        "warn"
    }
}

fn alert_label(event_type: &str) -> &'static str {
    if event_type.contains("completed") {
        "stopped"
    } else if event_type.contains("started") || event_type.contains("grace") {
        "grace"
    } else if event_type.contains("would") {
        "would stop"
    } else if event_type.contains("blocked") {
        "blocked"
    } else if event_type.contains("failed") {
        "failed"
    } else {
        "warning"
    }
}

fn default_alert_message(category: &str) -> &'static str {
    match category {
        "stopped" => "Curb stopped a correlated worker.",
        "grace" => "Curb started an enforcement grace period.",
        "would_stop" => "Curb would stop a correlated worker in enforcement mode.",
        "blocked" => "Curb blocked termination for an uncorrelated or protected process.",
        "failed" => "Curb could not complete a policy action.",
        _ => "Usage or runtime crossed policy.",
    }
}

fn actionable_event(event: &ledger::Event) -> bool {
    matches!(
        event.event_type.as_str(),
        "usage_termination_started" | "usage_termination_completed"
    )
}

fn alert_explanation(event_type: &str) -> &'static str {
    match event_type {
        "usage_would_terminate" => {
            "Alert mode: Curb would stop this correlated worker in enforcement mode."
        }
        "usage_kill_blocked" => {
            "Curb did not stop anything because the session was uncorrelated or watch-only."
        }
        "usage_grace_started" => "Enforcement grace period started for a correlated worker.",
        "usage_termination_started" => "Curb started terminating a correlated worker.",
        "usage_termination_completed" => "Curb completed termination for a correlated worker.",
        "policy_warning" | "usage_warning" => "Usage or runtime crossed the warning policy.",
        _ => "",
    }
}

fn session_index(snapshot: &Snapshot) -> HashMap<String, &SessionView> {
    snapshot
        .sessions
        .iter()
        .filter(|session| !session.provider.is_empty() && !session.id.is_empty())
        .map(|session| {
            (
                provider_session_key(&session.provider, &session.id),
                session,
            )
        })
        .collect()
}

fn project_alert_action(alert: &mut AlertView, sessions: &HashMap<String, &SessionView>) {
    let (Some(provider), Some(session_id)) = (&alert.provider, &alert.session_id) else {
        return;
    };
    let Some(session) = sessions.get(&provider_session_key(provider, session_id)) else {
        return;
    };
    alert.session_key = Some(session.key.clone());
    if matches!(
        alert.category.as_str(),
        "warning" | "would_stop" | "blocked" | "grace"
    ) {
        alert.can_acknowledge = session.can_acknowledge;
    }
}

fn provider_session_key(provider: &str, session_id: &str) -> String {
    format!("{provider}\0{session_id}")
}

fn string_data(event: &ledger::Event, key: &str) -> Option<String> {
    event
        .data
        .as_ref()
        .and_then(|data| data.get(key))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn build_overview_delta(previous: &Snapshot, next: &Snapshot) -> OverviewDelta {
    let previous_sessions = previous
        .sessions
        .iter()
        .map(|session| (session.key.as_str(), session))
        .collect::<HashMap<_, _>>();
    let previous_turns = previous.turns.iter().map(turn_key).collect::<BTreeSet<_>>();
    let mut sessions_with_turns = BTreeSet::new();
    let mut delta = OverviewDelta::default();
    for session in &next.sessions {
        let previous = previous_sessions.get(session.key.as_str()).copied();
        if previous.is_none() {
            delta.new_sessions += 1;
        }
        if is_alerting_session(session) && !previous.is_some_and(is_alerting_session) {
            delta.new_alerts += 1;
        }
    }
    for turn in &next.turns {
        if previous_turns.contains(&turn_key(turn)) {
            continue;
        }
        delta.tokens_added += turn.total_tokens;
        if !turn.session_key.is_empty() {
            sessions_with_turns.insert(turn.session_key.clone());
        }
    }
    delta.sessions_with_new_turns = sessions_with_turns.len();

    let previous_agents = previous
        .agents
        .iter()
        .map(agent_key)
        .collect::<BTreeSet<_>>();
    let next_agents = next.agents.iter().map(agent_key).collect::<BTreeSet<_>>();
    delta.agents_started = next_agents.difference(&previous_agents).count();
    delta.agents_ended = previous_agents.difference(&next_agents).count();

    let previous_source_errors = previous
        .overview
        .sources
        .iter()
        .filter_map(|source| {
            source
                .error
                .as_ref()
                .map(|error| (source.provider.as_str(), error.as_str()))
        })
        .collect::<HashMap<_, _>>();
    delta.source_errors = next
        .overview
        .sources
        .iter()
        .filter(|source| {
            source.error.as_ref().is_some_and(|error| {
                previous_source_errors
                    .get(source.provider.as_str())
                    .is_none_or(|previous| previous != error)
            })
        })
        .count();
    delta
}

fn is_alerting_session(session: &SessionView) -> bool {
    matches!(session.usage_state.as_str(), "warn" | "stop")
        || matches!(session.state.as_str(), "warn" | "stop")
}

fn turn_key(turn: &TurnView) -> String {
    if let Some(id) = &turn.id
        && !id.is_empty()
    {
        return format!("{}:{}:{}", turn.provider, turn.session_key, id);
    }
    if let Some(request_id) = &turn.request_id
        && !request_id.is_empty()
    {
        return format!("{}:{}:{}", turn.provider, turn.session_key, request_id);
    }
    format!(
        "{}:{}:{}:{}:{}:{}",
        turn.provider,
        turn.session_key,
        turn.model.as_deref().unwrap_or_default(),
        turn.at.map(|at| at.to_rfc3339()).unwrap_or_default(),
        turn.total_tokens,
        turn.cumulative_tokens
    )
}

fn agent_key(agent: &AgentView) -> String {
    format!(
        "{}:{}:{}",
        agent.id,
        agent.pid,
        agent
            .process_started_at
            .map(|started_at| started_at.to_rfc3339())
            .unwrap_or_default()
    )
}

fn onboarding_capabilities(
    config: &ConfigView,
    notifications: &NotificationView,
    termination: &TerminationCapability,
    snapshot: &Snapshot,
    enforceable_agent_types: usize,
) -> PlatformCapabilities {
    PlatformCapabilities {
        platform: std::env::consts::OS.to_string(),
        notifications: notification_capability_view(notifications),
        process_capture: process_capture_capability(&snapshot.overview.sources),
        process_identity: process_identity_capability(snapshot),
        enforcement: enforcement_capability(config, termination, enforceable_agent_types),
    }
}

fn notification_capability_view(notifications: &NotificationView) -> CapabilityView {
    CapabilityView {
        available: notifications.available,
        status: notifications.status.clone(),
        message: notifications.message.clone(),
    }
}

fn process_capture_capability(sources: &[SourceReport]) -> CapabilityView {
    if let Some(error) = sources
        .iter()
        .find(|source| source.provider == "processes")
        .and_then(|source| source.error.clone())
    {
        return CapabilityView {
            available: false,
            status: "error".to_string(),
            message: error,
        };
    }
    CapabilityView {
        available: true,
        status: "ready".to_string(),
        message: "local process scan is available".to_string(),
    }
}

fn process_capture_capability_from_platform(
    processes: Option<&platform::Snapshot>,
    capture_error: Option<&platform::PlatformError>,
) -> CapabilityView {
    if let Some(error) = capture_error {
        return CapabilityView {
            available: false,
            status: "error".to_string(),
            message: format!("process capture failed: {error}"),
        };
    }
    let Some(processes) = processes else {
        return CapabilityView {
            available: false,
            status: "waiting".to_string(),
            message: "process capture has not run yet".to_string(),
        };
    };
    CapabilityView {
        available: true,
        status: "ready".to_string(),
        message: format!(
            "{} captured",
            format_count(processes.processes().count(), "process")
        ),
    }
}

fn process_identity_capability_from_platform(
    processes: Option<&platform::Snapshot>,
    capture_error: Option<&platform::PlatformError>,
) -> CapabilityView {
    if capture_error.is_some() {
        return CapabilityView {
            available: false,
            status: "error".to_string(),
            message: "process identity unavailable until capture succeeds".to_string(),
        };
    }
    let Some(processes) = processes else {
        return CapabilityView {
            available: false,
            status: "waiting".to_string(),
            message: "process identity has not been sampled yet".to_string(),
        };
    };
    let total = processes.processes().count();
    if total == 0 {
        return CapabilityView {
            available: false,
            status: "waiting".to_string(),
            message: "no processes captured yet".to_string(),
        };
    }
    let with_identity = processes
        .processes()
        .filter(|process| process.has_termination_identity())
        .count();
    if with_identity == 0 {
        return CapabilityView {
            available: false,
            status: "degraded".to_string(),
            message: "captured processes lack start-time or executable identity evidence"
                .to_string(),
        };
    }
    CapabilityView {
        available: true,
        status: "ready".to_string(),
        message: format!(
            "{} with identity evidence",
            format_count(with_identity, "process")
        ),
    }
}

fn process_identity_capability(snapshot: &Snapshot) -> CapabilityView {
    let matched = snapshot.agents.iter().filter(|agent| agent.pid > 0).count();
    let revalidatable = snapshot
        .agents
        .iter()
        .filter(|agent| agent.process_started_at.is_some() && agent.pid > 0)
        .count();
    if revalidatable > 0 {
        CapabilityView {
            available: true,
            status: "ready".to_string(),
            message: format!(
                "{} include PID and start time",
                format_count(revalidatable, "matched worker")
            ),
        }
    } else if matched > 0 {
        CapabilityView {
            available: false,
            status: "action".to_string(),
            message: "matched workers are missing process start times; Curb will not stop them"
                .to_string(),
        }
    } else {
        CapabilityView {
            available: false,
            status: "waiting".to_string(),
            message: "no live worker identity evidence yet".to_string(),
        }
    }
}

fn platform_enforcement_capability(
    cfg: &Config,
    processes: Option<&platform::Snapshot>,
    capture_error: Option<&platform::PlatformError>,
    termination: &TerminationCapability,
    agents: &[AgentView],
) -> CapabilityView {
    if cfg.mode != Mode::Enforcement {
        return CapabilityView {
            available: false,
            status: "disabled".to_string(),
            message: "current mode will not terminate processes".to_string(),
        };
    }
    if !termination.supported {
        return CapabilityView {
            available: false,
            status: termination.status.clone(),
            message: termination.message.clone(),
        };
    }
    if !cfg.agents.iter().any(Agent::termination_allowed) {
        return CapabilityView {
            available: false,
            status: "blocked".to_string(),
            message: "no enforceable agent types are configured".to_string(),
        };
    }
    if !process_identity_capability_from_platform(processes, capture_error).available {
        return CapabilityView {
            available: false,
            status: "blocked".to_string(),
            message: "process identity is not strong enough for enforcement".to_string(),
        };
    }
    let enforceable = cfg
        .agents
        .iter()
        .filter(|agent| agent.termination_allowed())
        .map(|agent| agent.id.as_str())
        .collect::<BTreeSet<_>>();
    if !agents.iter().any(|agent| {
        enforceable.contains(agent.id.as_str())
            && agent.pid > 0
            && agent.process_started_at.is_some()
    }) {
        return CapabilityView {
            available: false,
            status: "blocked".to_string(),
            message: "no live enforceable worker is currently matched".to_string(),
        };
    }
    CapabilityView {
        available: true,
        status: "ready".to_string(),
        message: "enforcement can target revalidated worker processes only".to_string(),
    }
}

fn enforcement_capability(
    config: &ConfigView,
    termination: &TerminationCapability,
    enforceable_agent_types: usize,
) -> CapabilityView {
    if config.mode != "enforcement" {
        return CapabilityView {
            available: false,
            status: "disabled".to_string(),
            message: "current mode never terminates processes".to_string(),
        };
    }
    if enforceable_agent_types == 0 {
        return CapabilityView {
            available: false,
            status: "action".to_string(),
            message: "no enforceable worker matchers are configured".to_string(),
        };
    }
    CapabilityView {
        available: termination.supported,
        status: if termination.supported {
            "ready".to_string()
        } else {
            termination.status.clone()
        },
        message: if termination.supported {
            "enforcement can stop only revalidated worker process trees".to_string()
        } else {
            termination.message.clone()
        },
    }
}

fn config_step(config: &ConfigView) -> OnboardingStepView {
    match &config.path {
        Some(path) if !path.is_empty() => step("config", "Config", "done", format!("using {path}")),
        _ => step(
            "config",
            "Config",
            "action",
            "config path is not available".to_string(),
        ),
    }
}

fn agent_step(config: &ConfigView) -> OnboardingStepView {
    if config.agents.is_empty() {
        step(
            "agents",
            "Agents",
            "action",
            "no agent matchers are configured".to_string(),
        )
    } else {
        step(
            "agents",
            "Agents",
            "done",
            agent_count_message(&config.agents),
        )
    }
}

fn source_step(sources: &[SourceReport], capture: &CapabilityView) -> OnboardingStepView {
    if capture.status == "error" {
        return step("sources", "Sources", "action", capture.message.clone());
    }
    if sources.is_empty() {
        return step(
            "sources",
            "Sources",
            "waiting",
            "usage sources have not been scanned yet".to_string(),
        );
    }
    if let Some(source) = sources.iter().find(|source| source.error.is_some()) {
        return step(
            "sources",
            "Sources",
            "action",
            format!(
                "{}: {}",
                source.provider,
                source.error.as_deref().unwrap_or_default()
            ),
        );
    }
    let events = sources.iter().map(|source| source.events).sum::<usize>();
    let files = sources.iter().map(|source| source.files).sum::<usize>();
    if events == 0 {
        return step(
            "sources",
            "Sources",
            "waiting",
            "scanned usage sources; no local usage events found yet".to_string(),
        );
    }
    step(
        "sources",
        "Sources",
        "done",
        format!(
            "{} from {}",
            format_count(events, "usage event"),
            format_count(files, "file")
        ),
    )
}

fn notification_step(mode: &str, notifications: &NotificationView) -> OnboardingStepView {
    if mode == "visibility" {
        return step(
            "notifications",
            "Notifications",
            "waiting",
            "visibility mode records activity without requiring notifications".to_string(),
        );
    }
    if !notifications.enabled {
        return step(
            "notifications",
            "Notifications",
            "action",
            "local notifications are disabled".to_string(),
        );
    }
    if !notifications.available {
        return step(
            "notifications",
            "Notifications",
            "action",
            notifications.message.clone(),
        );
    }
    step(
        "notifications",
        "Notifications",
        "done",
        notifications.message.clone(),
    )
}

fn safety_step(config: &ConfigView) -> OnboardingStepView {
    if let Some(agent) = config
        .agents
        .iter()
        .find(|agent| agent.kind == "app" && agent.terminates)
    {
        return step(
            "safety",
            "Safety",
            "action",
            format!("{} is an app root but is enforceable", agent.label),
        );
    }
    step(
        "safety",
        "Safety",
        "done",
        "desktop app roots are watch-only; Curb stops only enforceable workers".to_string(),
    )
}

fn detected_providers(snapshot: &Snapshot) -> Vec<String> {
    let mut providers = Vec::new();
    for provider in snapshot
        .overview
        .sources
        .iter()
        .map(|source| source.provider.as_str())
        .chain(
            snapshot
                .sessions
                .iter()
                .map(|session| session.provider.as_str()),
        )
    {
        push_unique(&mut providers, provider);
    }
    providers
}

fn detected_workers(snapshot: &Snapshot) -> Vec<String> {
    let mut workers = Vec::new();
    for agent in &snapshot.agents {
        let label = if agent.label.is_empty() {
            agent.id.as_str()
        } else {
            agent.label.as_str()
        };
        push_unique(&mut workers, label);
    }
    workers
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !value.is_empty() && !values.iter().any(|seen| seen == value) {
        values.push(value.to_string());
    }
}

fn onboarding_final_sentence(mode: &str) -> String {
    match mode {
        "alert" => "Curb will notify on high-token turns. It will not stop any process in Alert mode. Desktop app roots are watch-only.".to_string(),
        "enforcement" => "Curb can stop only correlated enforceable workers after policy and grace checks. Desktop app roots are watch-only.".to_string(),
        _ => "Curb will record local agent activity. It will not notify or stop any process in Visibility mode. Desktop app roots are watch-only.".to_string(),
    }
}

fn action_label(mode: &str) -> String {
    match mode {
        "visibility" => "record only; no warnings or kills",
        "alert" => "notify only; never kill",
        "enforcement" => "enforcement enabled",
        other => other,
    }
    .to_string()
}

fn agent_count_message(agents: &[ConfigAgentView]) -> String {
    let enforceable = agents.iter().filter(|agent| agent.terminates).count();
    let watch_only = agents.len().saturating_sub(enforceable);
    if watch_only == 0 {
        format_count(enforceable, "enforceable agent")
    } else {
        format!(
            "{}, {}",
            format_count(enforceable, "enforceable agent"),
            format_count(watch_only, "watch-only agent")
        )
    }
}

fn format_count(count: usize, singular: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {singular}s")
    }
}

fn step(
    id: impl Into<String>,
    label: impl Into<String>,
    status: impl Into<String>,
    message: String,
) -> OnboardingStepView {
    OnboardingStepView {
        id: id.into(),
        label: label.into(),
        status: status.into(),
        message,
    }
}

pub struct Service<'a, P: Platform> {
    cfg: &'a Config,
    events: &'a [Event],
    platform: &'a P,
}

impl<'a, P: Platform> Service<'a, P> {
    pub fn new(cfg: &'a Config, events: &'a [Event], platform: &'a P) -> Self {
        Self {
            cfg,
            events,
            platform,
        }
    }

    pub fn acknowledge_session(
        &self,
        session_key: &str,
        request: AckRequest,
        now: DateTime<Utc>,
    ) -> Result<AckView, ServiceError> {
        if session_key.is_empty() {
            return Err(ServiceError::InvalidAck(
                "session key is required".to_string(),
            ));
        }
        if request.extend_seconds < 0 {
            return Err(ServiceError::InvalidAck(
                "extension must be positive".to_string(),
            ));
        }
        let session =
            find_session(self.events, session_key).ok_or(ServiceError::SessionNotFound)?;
        let default_extend = self.cfg.defaults.ack_extension.as_std();
        let mut extend = if request.extend_seconds == 0 {
            default_extend
        } else {
            std::time::Duration::from_secs(request.extend_seconds as u64)
        };
        if extend.is_zero() {
            return Err(ServiceError::InvalidAck(
                "ack extension must be configured".to_string(),
            ));
        }
        if !default_extend.is_zero() && extend > default_extend {
            extend = default_extend;
        }
        let previous_ack = read_session_ack(&self.cfg.service.state_dir, &session.key)?;
        let ack = write_session_ack(
            &self.cfg.service.state_dir,
            &session.key,
            extend,
            &request.reason,
            now,
        )?;
        if let Err(err) = self.append_session_ack_event(&ack, extend) {
            rollback_session_ack(&self.cfg.service.state_dir, &session.key, previous_ack)?;
            return Err(err);
        }
        Ok(AckView {
            session_key: ack.session_key,
            extend_seconds: extend.as_secs() as i64,
            until: ack.until,
            reason: ack.reason,
        })
    }

    pub fn stop_session(
        &self,
        session_key: &str,
        request: StopRequest,
        now: DateTime<Utc>,
    ) -> Result<StopView, ServiceError> {
        if session_key.is_empty() {
            return Err(ServiceError::InvalidStop(
                "session key is required".to_string(),
            ));
        }
        if !request.confirm {
            return Err(ServiceError::InvalidStop(
                "confirmation is required".to_string(),
            ));
        }
        let scope = if request.scope.is_empty() {
            "tree"
        } else {
            request.scope.as_str()
        };
        if scope != "tree" {
            return Err(ServiceError::InvalidStop(
                "only process tree scope is supported".to_string(),
            ));
        }
        validate_expected_stop_identity(&request.expected)?;
        if self.cfg.mode != Mode::Enforcement {
            return Err(ServiceError::StopConflict(
                "enforcement mode is required".to_string(),
            ));
        }
        let session =
            find_session(self.events, session_key).ok_or(ServiceError::SessionNotFound)?;
        if active_session_ack(&self.cfg.service.state_dir, &session.key, now)?.is_some() {
            return Err(ServiceError::StopConflict(
                "session is acknowledged".to_string(),
            ));
        }
        let snapshot = self.platform.capture().map_err(|error| {
            ServiceError::StopConflict(format!("process snapshot unavailable: {error}"))
        })?;
        let matches = process_matches(self.cfg, &snapshot);
        let correlation = correlate(&session, &matches);
        if !correlation.matched {
            return Err(ServiceError::StopConflict(
                "no live process correlation".to_string(),
            ));
        }
        let agent = correlation.agent.as_ref().expect("matched agent");
        if !agent.termination_allowed() {
            return Err(ServiceError::StopConflict(
                "matched agent is watch-only".to_string(),
            ));
        }
        let window_start =
            now - chrono::Duration::from_std(self.cfg.usage.window.as_std()).unwrap();
        let view = build_session_view(self.cfg, &session, &correlation, window_start, now);
        if view.usage_state != "stop" || !view.actionable {
            return Err(ServiceError::StopConflict(
                "session is not an actionable stop candidate".to_string(),
            ));
        }
        let process = correlation.process.as_ref().expect("matched process");
        validate_stop_expectation(&request.expected, process)?;
        let target = snapshot.termination_target(process).ok_or_else(|| {
            ServiceError::StopConflict("process identity could not be revalidated".to_string())
        })?;
        self.append_manual_stop_event(
            "manual_stop_started",
            &session,
            &correlation,
            &target,
            None,
            &request.reason,
        )?;
        let result = self
            .platform
            .terminate(&target, self.cfg.usage.grace_period.as_std());
        self.append_manual_stop_event(
            "manual_stop_completed",
            &session,
            &correlation,
            &target,
            Some("completed"),
            &request.reason,
        )?;
        let root = target.root();
        Ok(StopView {
            session_key: session.key,
            agent_id: agent.id.clone(),
            pid: root.pid.get(),
            started_at: root.started_at.expect("validated start time"),
            owner: root.username.clone().unwrap_or_default(),
            executable: root.executable.clone(),
            bundle_id: root.bundle_id.clone(),
            team_id: root.team_id.clone(),
            scope: scope.to_string(),
            scope_pids: target.scope().iter().map(|pid| pid.get()).collect(),
            result,
        })
    }

    fn append_session_ack_event(
        &self,
        ack: &SessionAck,
        extend: std::time::Duration,
    ) -> Result<(), ServiceError> {
        let mut data = Map::new();
        data.insert(
            "session_key".to_string(),
            Value::String(ack.session_key.clone()),
        );
        data.insert(
            "extend_seconds".to_string(),
            Value::Number(extend.as_secs().into()),
        );
        data.insert("until".to_string(), Value::String(ack.until.to_rfc3339()));
        self.append_ledger_event(
            ledger::Event::new("session_ack_received")
                .with_data(data)
                .with_message(ack.reason.clone()),
        )
    }

    fn append_manual_stop_event(
        &self,
        event_type: &str,
        session: &Session,
        correlation: &Correlation,
        target: &platform::TerminationTarget,
        result: Option<&str>,
        reason: &str,
    ) -> Result<(), ServiceError> {
        let mut event = ledger::Event::new(event_type).with_data(manual_stop_event_data(
            session,
            correlation,
            target,
            result,
        ));
        event.agent_id = correlation.agent.as_ref().map(|agent| agent.id.clone());
        event.mode = Some(self.cfg.mode.to_string());
        if !reason.is_empty() {
            event.message = Some(reason.to_string());
        }
        self.append_ledger_event(event)
    }

    fn append_ledger_event(&self, event: ledger::Event) -> Result<(), ServiceError> {
        Ledger::open(&self.cfg.ledger.path)?.append(event)?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Session {
    pub(crate) key: String,
    pub(crate) id: String,
    pub(crate) provider: String,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) models: BTreeSet<String>,
    pub(crate) last: Option<DateTime<Utc>>,
    pub(crate) last_usage: Option<DateTime<Utc>>,
    pub(crate) calls: usize,
    pub(crate) latest_turn_tokens: i64,
    pub(crate) window_tokens: i64,
    pub(crate) total_tokens: i64,
    turns: Vec<TurnView>,
}

impl Session {
    pub(crate) fn recent_usage(&self, window_start: DateTime<Utc>) -> bool {
        self.last_usage
            .is_some_and(|last_usage| last_usage >= window_start)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ProcessMatch {
    agent: Agent,
    process: platform::Process,
    confidence: i64,
    evidence: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Correlation {
    pub(crate) matched: bool,
    pub(crate) agent: Option<Agent>,
    pub(crate) process: Option<platform::Process>,
    pub(crate) score: i64,
    pub(crate) reason: String,
    confidence: i64,
    evidence: Vec<String>,
}

pub fn build_snapshot(
    cfg: &Config,
    events: &[Event],
    sources: Vec<SourceReport>,
    now: DateTime<Utc>,
) -> Snapshot {
    build_snapshot_with_processes(cfg, None, events, sources, now)
}

pub fn build_snapshot_with_processes(
    cfg: &Config,
    processes: Option<&platform::Snapshot>,
    events: &[Event],
    sources: Vec<SourceReport>,
    now: DateTime<Utc>,
) -> Snapshot {
    let window_start = now - chrono::Duration::from_std(cfg.usage.window.as_std()).unwrap();
    let sessions = build_sessions(events, window_start);
    let matches = processes
        .map(|snapshot| process_matches(cfg, snapshot))
        .unwrap_or_default();

    let mut session_views = sessions
        .iter()
        .map(|session| {
            let correlation = correlate(session, &matches);
            build_session_view(cfg, session, &correlation, window_start, now)
        })
        .collect::<Vec<_>>();
    sort_session_views(&mut session_views);

    let mut agent_views = matches
        .iter()
        .map(|matched| {
            let best = best_session_for_match(matched, &sessions);
            let session_view = best.as_ref().map(|session| {
                let correlation = correlate(session, std::slice::from_ref(matched));
                build_session_view(cfg, session, &correlation, window_start, now)
            });
            build_agent_view(matched, session_view.as_ref(), now)
        })
        .collect::<Vec<_>>();
    sort_agent_views(&mut agent_views);

    let turns = sessions
        .iter()
        .flat_map(|session| session.turns.clone())
        .collect::<Vec<_>>();
    let overview = build_overview(cfg, &agent_views, &session_views, sources, now);
    Snapshot {
        overview,
        agents: agent_views,
        sessions: session_views,
        turns,
    }
}

pub(crate) fn build_sessions(events: &[Event], window_start: DateTime<Utc>) -> Vec<Session> {
    let mut by_key: HashMap<String, Session> = HashMap::new();
    for event in events {
        let id = event.session_id.clone().unwrap_or_default();
        let key = if id.is_empty() {
            format!("{}:{}", event.provider, event.source_path.display())
        } else {
            format!("{}:{id}", event.provider)
        };
        let session = by_key.entry(key.clone()).or_insert_with(|| Session {
            key: key.clone(),
            id: id.clone(),
            provider: event.provider.clone(),
            cwd: event.cwd.clone(),
            models: BTreeSet::new(),
            last: None,
            last_usage: None,
            calls: 0,
            latest_turn_tokens: 0,
            window_tokens: 0,
            total_tokens: 0,
            turns: Vec::new(),
        });
        if session.cwd.is_none() {
            session.cwd = event.cwd.clone();
        }
        if let Some(model) = &event.model {
            session.models.insert(model.clone());
        }
        if event.timestamp > session.last {
            session.last = event.timestamp;
        }
        if event.total_tokens > 0 && event.timestamp >= session.last_usage {
            session.last_usage = event.timestamp;
            session.latest_turn_tokens = event.total_tokens;
        }
        if event.timestamp.is_some_and(|at| at >= window_start) {
            session.window_tokens += event.total_tokens;
        }
        session.calls += 1;
        session.total_tokens += event.total_tokens;
        session.turns.push(TurnView {
            id: event.turn_id.clone(),
            request_id: event.request_id.clone(),
            session_key: key,
            session_id: event.session_id.clone(),
            provider: event.provider.clone(),
            at: event.timestamp,
            model: event.model.clone(),
            input_tokens: event.input_tokens,
            cached_input_tokens: event.cached_input_tokens,
            output_tokens: event.output_tokens,
            cache_creation_input_tokens: event.cache_creation_input_tokens,
            reasoning_output_tokens: event.reasoning_output_tokens,
            total_tokens: event.total_tokens,
            cumulative_tokens: event.cumulative_tokens,
            source: event.source.clone(),
        });
    }
    by_key.into_values().collect()
}

fn find_session(events: &[Event], key: &str) -> Option<Session> {
    build_sessions(events, DateTime::<Utc>::MIN_UTC)
        .into_iter()
        .find(|session| session.key == key || session.id == key)
}

pub fn canonical_session_key(events: &[Event], key: &str) -> Option<String> {
    find_session(events, key).map(|session| session.key)
}

pub fn session_turns(
    events: &[Event],
    key: &str,
    since: Option<DateTime<Utc>>,
    limit: usize,
) -> Result<Vec<TurnView>, ServiceError> {
    let session = find_session(events, key).ok_or(ServiceError::SessionNotFound)?;
    let mut turns = session
        .turns
        .into_iter()
        .filter(|turn| since.is_none_or(|since| turn.at.is_none_or(|at| at >= since)))
        .collect::<Vec<_>>();
    turns.sort_by(|left, right| right.at.cmp(&left.at));
    if limit > 0 && turns.len() > limit {
        turns.truncate(limit);
    }
    Ok(turns)
}

pub fn write_session_ack(
    state_dir: &Path,
    session_key: &str,
    extend: std::time::Duration,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<SessionAck, ServiceError> {
    if session_key.is_empty() {
        return Err(ServiceError::InvalidAck(
            "session key is required".to_string(),
        ));
    }
    if extend.is_zero() {
        return Err(ServiceError::InvalidAck(
            "extension must be positive".to_string(),
        ));
    }
    let ack = SessionAck {
        session_key: session_key.to_string(),
        reason: reason.to_string(),
        until: now + chrono::Duration::from_std(extend).unwrap(),
        created_at: now,
    };
    let path = session_ack_path(state_dir, session_key);
    let dir = path.parent().unwrap_or(state_dir);
    fs::create_dir_all(dir).map_err(|source| ServiceError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|source| {
            ServiceError::Io {
                path: dir.to_path_buf(),
                source,
            }
        })?;
    }
    let content = serde_json::to_vec_pretty(&ack).map_err(|source| ServiceError::Json {
        path: path.clone(),
        source,
    })?;
    fs::write(&path, content).map_err(|source| ServiceError::Io {
        path: path.clone(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            ServiceError::Io {
                path: path.clone(),
                source,
            }
        })?;
    }
    Ok(ack)
}

pub fn read_session_ack(
    state_dir: &Path,
    session_key: &str,
) -> Result<Option<SessionAck>, ServiceError> {
    let path = session_ack_path(state_dir, session_key);
    let content = match fs::read(&path) {
        Ok(content) => content,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(ServiceError::Io { path, source }),
    };
    serde_json::from_slice(&content)
        .map(Some)
        .map_err(|source| ServiceError::Json { path, source })
}

pub fn delete_session_ack(state_dir: &Path, session_key: &str) -> Result<(), ServiceError> {
    let path = session_ack_path(state_dir, session_key);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ServiceError::Io { path, source }),
    }
}

fn rollback_session_ack(
    state_dir: &Path,
    session_key: &str,
    previous: Option<SessionAck>,
) -> Result<(), ServiceError> {
    match previous {
        Some(previous) => {
            let extend = previous
                .until
                .signed_duration_since(previous.created_at)
                .to_std()
                .map_err(|_| {
                    ServiceError::InvalidAck("previous ack duration is invalid".to_string())
                })?;
            write_session_ack(
                state_dir,
                session_key,
                extend,
                &previous.reason,
                previous.created_at,
            )?;
            Ok(())
        }
        None => delete_session_ack(state_dir, session_key),
    }
}

pub fn active_session_ack(
    state_dir: &Path,
    session_key: &str,
    now: DateTime<Utc>,
) -> Result<Option<SessionAck>, ServiceError> {
    let Some(ack) = read_session_ack(state_dir, session_key)? else {
        return Ok(None);
    };
    if now < ack.until {
        Ok(Some(ack))
    } else {
        Ok(None)
    }
}

fn manual_stop_event_data(
    session: &Session,
    correlation: &Correlation,
    target: &platform::TerminationTarget,
    result: Option<&str>,
) -> Map<String, Value> {
    let root = target.root();
    let mut data = Map::new();
    data.insert(
        "session_key".to_string(),
        Value::String(session.key.clone()),
    );
    data.insert("session_id".to_string(), Value::String(session.id.clone()));
    data.insert(
        "provider".to_string(),
        Value::String(session.provider.clone()),
    );
    if let Some(cwd) = &session.cwd {
        data.insert("cwd".to_string(), Value::String(cwd.display().to_string()));
    }
    data.insert("turn_tokens".to_string(), json!(session.latest_turn_tokens));
    if let Some(agent) = &correlation.agent {
        data.insert("agent_id".to_string(), Value::String(agent.id.clone()));
    }
    data.insert("pid".to_string(), json!(root.pid.get()));
    if let Some(started_at) = root.started_at {
        data.insert(
            "started_at".to_string(),
            Value::String(started_at.to_rfc3339()),
        );
    }
    if let Some(owner) = &root.username {
        data.insert("owner".to_string(), Value::String(owner.clone()));
    }
    if let Some(executable) = &root.executable {
        data.insert(
            "executable".to_string(),
            Value::String(executable.display().to_string()),
        );
    }
    if let Some(bundle_id) = &root.bundle_id {
        data.insert("bundle_id".to_string(), Value::String(bundle_id.clone()));
    }
    if let Some(team_id) = &root.team_id {
        data.insert("team_id".to_string(), Value::String(team_id.clone()));
    }
    data.insert("scope".to_string(), Value::String("tree".to_string()));
    data.insert(
        "scope_pids".to_string(),
        Value::Array(target.scope().iter().map(|pid| json!(pid.get())).collect()),
    );
    data.insert(
        "correlation".to_string(),
        Value::String(correlation.reason.clone()),
    );
    data.insert("correlation_score".to_string(), json!(correlation.score));
    if let Some(result) = result {
        data.insert("result".to_string(), Value::String(result.to_string()));
    }
    data
}

fn session_ack_path(state_dir: &Path, session_key: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(session_key.as_bytes());
    state_dir
        .join("usage-acks")
        .join(format!("{}.json", hex::encode(hasher.finalize())))
}

fn build_session_view(
    cfg: &Config,
    session: &Session,
    correlation: &Correlation,
    window_start: DateTime<Utc>,
    now: DateTime<Utc>,
) -> SessionView {
    let active = session.last_usage.is_some_and(|last| last >= window_start);
    let over_stop = session.latest_turn_tokens >= cfg.usage.kill_turn_tokens;
    let over_warn = session.latest_turn_tokens >= cfg.usage.warn_turn_tokens;
    let matched_agent = correlation.agent.as_ref();
    let termination_allowed = matched_agent.is_some_and(Agent::termination_allowed);
    let ack_until = active_session_ack(&cfg.service.state_dir, &session.key, now)
        .ok()
        .flatten()
        .map(|ack| ack.until);
    let (mut state, usage_state, mut action_state, mut risk_rank, mut explanation) = if active
        && over_stop
    {
        let state = match (correlation.matched, termination_allowed) {
            (false, _) => "uncorrelated",
            (true, false) => "watch-only",
            (true, true) => "stop",
        };
        (
            state,
            "stop",
            if state == "stop" && cfg.mode == Mode::Enforcement {
                "stop-pending"
            } else if state == "stop" {
                "would-stop"
            } else {
                "blocked"
            },
            if state == "stop" && cfg.mode == Mode::Enforcement {
                0
            } else {
                1
            },
            match state {
                "uncorrelated" => {
                    "usage crossed threshold, but no live process matched; Curb will not stop anything"
                }
                "watch-only" => {
                    "usage crossed threshold, but matched agent is watch-only; Curb will not stop desktop apps"
                }
                _ => "latest turn crossed the stop threshold",
            },
        )
    } else if active && over_warn {
        let state = if !correlation.matched {
            "uncorrelated"
        } else if !termination_allowed {
            "watch-only"
        } else {
            "warn"
        };
        (
            state,
            "warn",
            "acknowledge",
            1,
            match state {
                "uncorrelated" => {
                    "usage crossed threshold, but no live process matched; Curb will not stop anything"
                }
                "watch-only" => {
                    "usage crossed threshold, but matched agent is watch-only; Curb will not stop desktop apps"
                }
                _ => "latest turn crossed the warning threshold",
            },
        )
    } else if active {
        (
            "active",
            "spending",
            "none",
            1,
            "recent token usage is within policy",
        )
    } else if over_stop || over_warn {
        (
            "idle-high",
            "quiet-high",
            "none",
            3,
            "historically high usage is quiet in the policy window",
        )
    } else {
        ("quiet", "quiet", "none", 5, "no recent token usage")
    };
    if ack_until.is_some() && matches!(usage_state, "warn" | "stop") {
        state = "acknowledged";
        action_state = "acknowledged";
        risk_rank = 2;
        explanation = "usage crossed threshold, but this session is acknowledged";
    }
    let process_state = session_process_state(state, usage_state, correlation);
    let agent_state = session_agent_state(state);
    SessionView {
        key: session.key.clone(),
        id: session.id.clone(),
        provider: session.provider.clone(),
        state: state.to_string(),
        process_state: process_state.to_string(),
        usage_state: usage_state.to_string(),
        action_state: action_state.to_string(),
        actionable: action_state == "stop-pending",
        can_acknowledge: ack_until.is_none() && matches!(usage_state, "warn" | "stop"),
        acknowledged: ack_until.is_some(),
        acknowledged_until: ack_until,
        agent_state: Some(agent_state.to_string()),
        project: session.cwd.as_ref().and_then(|cwd| project_name(cwd)),
        cwd: session.cwd.clone(),
        models: session.models.iter().cloned().collect(),
        last_seen_at: session.last.unwrap_or(window_start),
        last_usage_at: session.last_usage,
        calls: session.calls,
        latest_turn_tokens: session.latest_turn_tokens,
        window_tokens: session.window_tokens,
        total_tokens: session.total_tokens,
        correlated_agent_id: matched_agent.map(|agent| agent.id.clone()),
        correlated_pid: correlation
            .process
            .as_ref()
            .map(|process| process.pid.get()),
        correlated_process_started_at: correlation
            .process
            .as_ref()
            .and_then(|process| process.started_at),
        correlated_owner: correlation
            .process
            .as_ref()
            .and_then(|process| process.username.clone()),
        correlated_executable: correlation
            .process
            .as_ref()
            .and_then(|process| process.executable.clone()),
        correlated_bundle_id: correlation
            .process
            .as_ref()
            .and_then(|process| process.bundle_id.clone()),
        correlated_team_id: correlation
            .process
            .as_ref()
            .and_then(|process| process.team_id.clone()),
        correlation_reason: correlation.matched.then(|| correlation.reason.clone()),
        correlation_score: correlation.score,
        confidence: correlation.confidence,
        matched_by: correlation.evidence.clone(),
        risk_rank,
        explanation: explanation.to_string(),
    }
}

fn build_overview(
    cfg: &Config,
    agents: &[AgentView],
    sessions: &[SessionView],
    sources: Vec<SourceReport>,
    now: DateTime<Utc>,
) -> Overview {
    let active_sessions = sessions
        .iter()
        .filter(|session| {
            session.process_state == "running"
                && matches!(session.usage_state.as_str(), "spending" | "warn" | "stop")
        })
        .count();
    let warning_sessions = sessions
        .iter()
        .filter(|session| {
            matches!(session.usage_state.as_str(), "warn" | "stop") && !session.actionable
        })
        .count();
    let stop_sessions = sessions
        .iter()
        .filter(|session| session.usage_state == "stop" && session.actionable)
        .count();
    let idle_high_sessions = sessions
        .iter()
        .filter(|session| session.usage_state == "quiet-high")
        .count();
    let status = if stop_sessions > 0 {
        "ACTION"
    } else if warning_sessions > 0 {
        "WATCH"
    } else if active_sessions > 0 {
        "ACTIVE"
    } else {
        "OK"
    };
    let message = match status {
        "ACTION" => "active usage is over a stop threshold",
        "WATCH" => "active usage is over a warning threshold",
        "ACTIVE" => "agents are spending tokens within policy",
        _ => "no active over-budget usage",
    };
    Overview {
        mode: cfg.mode.to_string(),
        action: action_label(&cfg.mode.to_string()),
        status: status.to_string(),
        message: message.to_string(),
        active_agents: agents.len(),
        active_sessions,
        warning_sessions,
        stop_sessions,
        idle_high_sessions,
        window_tokens: sessions
            .iter()
            .filter(|session| session.process_state == "running")
            .map(|session| session.window_tokens)
            .sum(),
        lookback_tokens: sessions.iter().map(|session| session.total_tokens).sum(),
        last_scan: now,
        sources,
        changes: OverviewDelta::default(),
        capabilities: PlatformCapabilities::default(),
    }
}

pub(crate) fn process_matches(cfg: &Config, snapshot: &platform::Snapshot) -> Vec<ProcessMatch> {
    let mut matches = Vec::new();
    for process in snapshot.processes() {
        for agent in &cfg.agents {
            let (confidence, evidence) = match_agent(agent, process);
            if confidence >= cfg.service.min_confidence {
                matches.push(ProcessMatch {
                    agent: agent.clone(),
                    process: process.clone(),
                    confidence,
                    evidence,
                });
            }
        }
    }
    matches.sort_by(|left, right| {
        right
            .confidence
            .cmp(&left.confidence)
            .then_with(|| left.process.pid.get().cmp(&right.process.pid.get()))
    });
    matches
}

fn match_agent(agent: &Agent, process: &platform::Process) -> (i64, Vec<String>) {
    let matcher = &agent.matcher;
    if matcher
        .exclude_names
        .iter()
        .any(|name| name.eq_ignore_ascii_case(&process.name))
    {
        return (0, Vec::new());
    }
    if any_regex_matches(&matcher.exclude_command_regex, &process.command)
        || any_regex_matches(&matcher.exclude_parent_regex, &process.command)
    {
        return (0, Vec::new());
    }
    if !matcher.require_command_regex.is_empty()
        && !matcher
            .require_command_regex
            .iter()
            .all(|pattern| regex_matches(pattern, &process.command))
    {
        return (0, Vec::new());
    }

    let mut confidence = 0;
    let mut evidence = Vec::new();
    if matcher
        .process_names
        .iter()
        .any(|name| name.eq_ignore_ascii_case(&process.name))
    {
        confidence += 50;
        evidence.push("process_name".to_string());
    }
    if any_regex_matches(&matcher.command_regex, &process.command) {
        confidence += 40;
        evidence.push("command_regex".to_string());
    }
    if process.executable.as_ref().is_some_and(|executable| {
        matcher
            .executable_paths
            .iter()
            .any(|path| std::path::Path::new(path) == executable.as_path())
    }) {
        confidence += 80;
        evidence.push("executable_path".to_string());
    }
    if process.bundle_id.as_ref().is_some_and(|bundle_id| {
        matcher
            .bundle_ids
            .iter()
            .any(|expected| expected == bundle_id)
    }) {
        confidence += 80;
        evidence.push("bundle_id".to_string());
    }
    (confidence, evidence)
}

fn any_regex_matches(patterns: &[String], value: &str) -> bool {
    patterns.iter().any(|pattern| regex_matches(pattern, value))
}

fn regex_matches(pattern: &str, value: &str) -> bool {
    Regex::new(pattern)
        .map(|regex| regex.is_match(value))
        .unwrap_or(false)
}

pub(crate) fn correlate(session: &Session, matches: &[ProcessMatch]) -> Correlation {
    let Some(session_cwd) = clean_path(session.cwd.as_ref()) else {
        return Correlation::default();
    };
    let mut best = Correlation::default();
    for matched in matches {
        if !same_provider(&session.provider, &matched.agent.family) {
            continue;
        }
        let Some(process_cwd) = clean_path(matched.process.cwd.as_ref()) else {
            continue;
        };
        let (score, reason) = if process_cwd == session_cwd {
            (125, "provider+cwd")
        } else if safe_cwd_prefix_match(&process_cwd, &session_cwd)
            || safe_cwd_prefix_match(&session_cwd, &process_cwd)
        {
            (75, "provider+cwd-prefix")
        } else {
            continue;
        };
        if score > best.score {
            best = Correlation {
                matched: true,
                agent: Some(matched.agent.clone()),
                process: Some(matched.process.clone()),
                score,
                reason: reason.to_string(),
                confidence: matched.confidence,
                evidence: matched.evidence.clone(),
            };
        }
    }
    best
}

fn best_session_for_match<'a>(
    matched: &ProcessMatch,
    sessions: &'a [Session],
) -> Option<&'a Session> {
    sessions
        .iter()
        .filter_map(|session| {
            let correlation = correlate(session, std::slice::from_ref(matched));
            correlation.matched.then_some((correlation.score, session))
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, session)| session)
}

fn build_agent_view(
    matched: &ProcessMatch,
    session: Option<&SessionView>,
    now: DateTime<Utc>,
) -> AgentView {
    let mut state = if matched.agent.termination_allowed() {
        "running"
    } else {
        "watch-only"
    };
    let mut explanation = if matched.agent.termination_allowed() {
        "process is running with no correlated usage"
    } else {
        "matched agent is watch-only"
    };
    let mut usage_state = "quiet";
    let mut action_state = "none";
    let mut actionable = false;
    let mut latest_session_id = None;
    let mut latest_turn_tokens = 0;
    let mut window_tokens = 0;
    if let Some(session) = session {
        state = if matched.agent.termination_allowed() {
            match session.state.as_str() {
                "active" => "spending",
                "idle-high" | "quiet" => "idle",
                other => other,
            }
        } else {
            "watch-only"
        };
        usage_state = &session.usage_state;
        action_state = &session.action_state;
        actionable = session.actionable;
        latest_session_id = Some(session.id.clone());
        latest_turn_tokens = session.latest_turn_tokens;
        window_tokens = session.window_tokens;
        explanation = if state == "idle" {
            "process is running; correlated session is not currently spending"
        } else {
            &session.explanation
        };
    }
    AgentView {
        id: matched.agent.id.clone(),
        provider: matched.agent.family.clone(),
        label: matched.agent.label.clone(),
        state: state.to_string(),
        process_state: if matched.agent.termination_allowed() {
            "running"
        } else {
            "watch-only"
        }
        .to_string(),
        usage_state: usage_state.to_string(),
        action_state: action_state.to_string(),
        actionable,
        pid: matched.process.pid.get(),
        process_started_at: matched.process.started_at,
        running_for_seconds: running_for_seconds(matched.process.started_at, now),
        project: matched
            .process
            .cwd
            .as_ref()
            .and_then(|cwd| project_name(cwd)),
        cwd: matched.process.cwd.clone(),
        matched_by: matched.evidence.clone(),
        confidence: matched.confidence,
        latest_session_id,
        latest_turn_tokens,
        window_tokens,
        explanation: explanation.to_string(),
    }
}

fn session_agent_state(state: &str) -> &str {
    match state {
        "active" => "spending",
        "idle-high" | "quiet" => "idle",
        other => other,
    }
}

fn running_for_seconds(started_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Option<i64> {
    let started_at = started_at?;
    Some(now.signed_duration_since(started_at).num_seconds().max(0))
}

fn project_name(path: &Path) -> Option<String> {
    let raw = path.display().to_string();
    let trimmed = raw.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .map(ToString::to_string)
}

fn sort_session_views(sessions: &mut [SessionView]) {
    sessions.sort_by(|left, right| {
        left.risk_rank
            .cmp(&right.risk_rank)
            .then_with(|| right.latest_turn_tokens.cmp(&left.latest_turn_tokens))
            .then_with(|| right.window_tokens.cmp(&left.window_tokens))
            .then_with(|| right.last_seen_at.cmp(&left.last_seen_at))
    });
}

fn sort_agent_views(agents: &mut [AgentView]) {
    let priority = |state: &str| match state {
        "stop" => 0,
        "warn" => 1,
        "spending" => 2,
        "running" => 3,
        "watch-only" => 4,
        "idle" => 5,
        _ => 6,
    };
    agents.sort_by(|left, right| {
        priority(&left.state)
            .cmp(&priority(&right.state))
            .then_with(|| left.project.cmp(&right.project))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn session_process_state(
    state: &str,
    usage_state: &str,
    correlation: &Correlation,
) -> &'static str {
    if state == "watch-only" {
        "watch-only"
    } else if correlation.matched {
        "running"
    } else if state == "uncorrelated" || matches!(usage_state, "warn" | "stop") {
        "unknown"
    } else {
        "no-process"
    }
}

fn same_provider(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn clean_path(path: Option<&PathBuf>) -> Option<PathBuf> {
    let path = path?;
    if path.as_os_str().is_empty() {
        None
    } else {
        Some(path.components().collect())
    }
}

fn path_contains(parent: &std::path::Path, child: &std::path::Path) -> bool {
    child.starts_with(parent)
}

fn safe_cwd_prefix_match(parent: &std::path::Path, child: &std::path::Path) -> bool {
    path_specificity(parent) >= 2 && path_specificity(child) >= 2 && path_contains(parent, child)
}

fn path_specificity(path: &std::path::Path) -> usize {
    path.components()
        .filter(|component| matches!(component, std::path::Component::Normal(_)))
        .count()
}

fn validate_expected_stop_identity(expected: &StopExpectedIdentity) -> Result<(), ServiceError> {
    if expected.pid == 0 {
        return Err(ServiceError::InvalidStop(
            "expected pid is required".to_string(),
        ));
    }
    if expected.started_at.is_none() {
        return Err(ServiceError::InvalidStop(
            "expected process start time is required".to_string(),
        ));
    }
    if expected.owner.is_empty() {
        return Err(ServiceError::InvalidStop(
            "expected owner is required".to_string(),
        ));
    }
    if expected.executable.is_none() && expected.bundle_id.is_none() && expected.team_id.is_none() {
        return Err(ServiceError::InvalidStop(
            "expected executable or app identity is required".to_string(),
        ));
    }
    Ok(())
}

fn validate_stop_expectation(
    expected: &StopExpectedIdentity,
    actual: &platform::Process,
) -> Result<(), ServiceError> {
    if actual.pid.get() != expected.pid {
        return Err(ServiceError::StopConflict("pid changed".to_string()));
    }
    if actual.started_at != expected.started_at {
        return Err(ServiceError::StopConflict(
            "process start time changed".to_string(),
        ));
    }
    if actual.username.as_deref() != Some(expected.owner.as_str()) {
        return Err(ServiceError::StopConflict(
            "process owner changed".to_string(),
        ));
    }
    if let Some(executable) = &expected.executable
        && actual.executable.as_ref() != Some(executable)
    {
        return Err(ServiceError::StopConflict("executable changed".to_string()));
    }
    if let Some(bundle_id) = &expected.bundle_id
        && actual.bundle_id.as_ref() != Some(bundle_id)
    {
        return Err(ServiceError::StopConflict("bundle id changed".to_string()));
    }
    if let Some(team_id) = &expected.team_id
        && actual.team_id.as_ref() != Some(team_id)
    {
        return Err(ServiceError::StopConflict("team id changed".to_string()));
    }
    if !actual.has_termination_identity() {
        return Err(ServiceError::StopConflict(
            "process identity is incomplete".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chrono::TimeZone;

    use super::*;
    use crate::config::Config;
    use crate::platform::{PlatformError, TerminationTarget};
    use crate::usage::Event;

    #[test]
    fn active_stop_session_is_actionable_only_in_enforcement_mode() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = crate::config::Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let processes = process_snapshot(now, "codex", "/repo");
        let snapshot = build_snapshot_with_processes(
            &cfg,
            Some(&processes),
            &[event("codex", "s1", now, 250)],
            Vec::new(),
            now,
        );

        assert_eq!(snapshot.overview.status, "ACTION");
        assert_eq!(snapshot.sessions[0].usage_state, "stop");
        assert_eq!(snapshot.sessions[0].action_state, "stop-pending");
        assert!(snapshot.sessions[0].actionable);
        assert_eq!(snapshot.sessions[0].process_state, "running");
        assert_eq!(snapshot.sessions[0].correlated_pid, Some(100));
        assert_eq!(snapshot.sessions[0].agent_state.as_deref(), Some("stop"));
        assert_eq!(snapshot.sessions[0].project.as_deref(), Some("repo"));
        assert_eq!(snapshot.agents[0].project.as_deref(), Some("repo"));
        assert_eq!(snapshot.agents[0].running_for_seconds, Some(600));
    }

    #[test]
    fn alert_mode_reports_would_stop_without_actionability() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = crate::config::Mode::Alert;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let processes = process_snapshot(now, "codex", "/repo");
        let snapshot = build_snapshot_with_processes(
            &cfg,
            Some(&processes),
            &[event("codex", "s1", now, 250)],
            Vec::new(),
            now,
        );

        assert_eq!(snapshot.sessions[0].action_state, "would-stop");
        assert!(!snapshot.sessions[0].actionable);
    }

    #[test]
    fn uncorrelated_stop_usage_is_blocked_not_actionable() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = crate::config::Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let snapshot = build_snapshot(&cfg, &[event("codex", "s1", now, 250)], Vec::new(), now);

        assert_eq!(snapshot.overview.status, "WATCH");
        assert_eq!(snapshot.sessions[0].state, "uncorrelated");
        assert_eq!(snapshot.sessions[0].process_state, "unknown");
        assert_eq!(snapshot.sessions[0].action_state, "blocked");
        assert!(!snapshot.sessions[0].actionable);
    }

    #[test]
    fn watch_only_app_match_blocks_termination() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.agents = vec![crate::config::Agent {
            id: "codex-desktop".to_string(),
            label: "Codex Desktop".to_string(),
            family: "codex".to_string(),
            kind: crate::config::AgentKind::App,
            matcher: crate::config::Match {
                process_names: vec!["Codex".to_string()],
                ..Default::default()
            },
            policy: None,
        }];
        cfg.mode = crate::config::Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let processes = process_snapshot(now, "Codex", "/repo");
        let snapshot = build_snapshot_with_processes(
            &cfg,
            Some(&processes),
            &[event("codex", "s1", now, 250)],
            Vec::new(),
            now,
        );

        assert_eq!(snapshot.sessions[0].state, "watch-only");
        assert_eq!(snapshot.sessions[0].process_state, "watch-only");
        assert_eq!(snapshot.sessions[0].action_state, "blocked");
        assert!(!snapshot.sessions[0].actionable);
        assert_eq!(snapshot.agents[0].state, "watch-only");
    }

    #[test]
    fn multiple_sessions_can_correlate_to_one_worker() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let processes = process_snapshot(now, "codex", "/repo");
        let snapshot = build_snapshot_with_processes(
            &cfg,
            Some(&processes),
            &[
                event("codex", "s1", now, 50),
                event("codex", "s2", now - chrono::Duration::minutes(1), 40),
            ],
            Vec::new(),
            now,
        );

        assert_eq!(snapshot.agents.len(), 1);
        assert_eq!(snapshot.sessions.len(), 2);
        assert!(
            snapshot
                .sessions
                .iter()
                .all(|session| session.correlated_pid == Some(100))
        );
    }

    #[test]
    fn overview_delta_reports_new_usage_alerts_agents_and_source_errors() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let previous_processes = platform::Snapshot::new([process(now, 100, "codex", "/repo")]);
        let next_processes = platform::Snapshot::new([process(now, 101, "codex", "/repo")]);
        let previous = build_snapshot_with_processes(
            &cfg,
            Some(&previous_processes),
            &[event("codex", "old", now, 50)],
            vec![
                SourceReport {
                    provider: "codex".to_string(),
                    files: 1,
                    events: 1,
                    error: None,
                },
                SourceReport {
                    provider: "claude".to_string(),
                    files: 1,
                    events: 0,
                    error: Some("permission denied".to_string()),
                },
            ],
            now,
        );
        let next = build_snapshot_with_processes(
            &cfg,
            Some(&next_processes),
            &[
                event("codex", "old", now, 50),
                event("codex", "old", now + chrono::Duration::seconds(1), 250),
                event("codex", "new", now + chrono::Duration::seconds(2), 25),
            ],
            vec![
                SourceReport {
                    provider: "codex".to_string(),
                    files: 1,
                    events: 3,
                    error: Some("schema changed".to_string()),
                },
                SourceReport {
                    provider: "claude".to_string(),
                    files: 1,
                    events: 0,
                    error: Some("permission denied".to_string()),
                },
            ],
            now,
        );

        let annotated = annotate_overview_delta(Some(&previous), next);

        assert_eq!(annotated.overview.changes.new_sessions, 1);
        assert_eq!(annotated.overview.changes.sessions_with_new_turns, 2);
        assert_eq!(annotated.overview.changes.tokens_added, 275);
        assert_eq!(annotated.overview.changes.new_alerts, 1);
        assert_eq!(annotated.overview.changes.agents_started, 1);
        assert_eq!(annotated.overview.changes.agents_ended, 1);
        assert_eq!(annotated.overview.changes.source_errors, 1);
    }

    #[test]
    fn cwd_correlation_uses_path_components_not_string_prefixes() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let processes = platform::Snapshot::new([
            process(now, 100, "codex", "/work/project-other"),
            process(now, 200, "codex", "/work/project/src"),
        ]);
        let snapshot = build_snapshot_with_processes(
            &cfg,
            Some(&processes),
            &[event("codex", "s1", now, 50).with_cwd("/work/project")],
            Vec::new(),
            now,
        );

        assert_eq!(snapshot.sessions[0].correlated_pid, Some(200));
        assert_eq!(
            snapshot.sessions[0].correlation_reason.as_deref(),
            Some("provider+cwd-prefix")
        );
    }

    #[test]
    fn cwd_prefix_correlation_rejects_root_or_top_level_paths() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let processes = platform::Snapshot::new([
            process(now, 100, "codex", "/repo/a"),
            process(now, 200, "codex", "/Users/phaedrus/project"),
        ]);
        let snapshot = build_snapshot_with_processes(
            &cfg,
            Some(&processes),
            &[
                event("codex", "root", now, 50).with_cwd("/"),
                event("codex", "top", now, 50).with_cwd("/Users"),
            ],
            Vec::new(),
            now,
        );

        assert_eq!(snapshot.sessions[0].correlated_pid, None);
        assert_eq!(snapshot.sessions[1].correlated_pid, None);
    }

    #[test]
    fn acknowledge_session_persists_and_suppresses_actionability() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
        cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
        cfg.mode = crate::config::Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        cfg.defaults.ack_extension = crate::config::HumanDuration::seconds(60);
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let events = vec![event("codex", "s1", now, 250)];
        let platform = FakePlatform::new(process_snapshot(now, "codex", "/repo"));
        let service = Service::new(&cfg, &events, &platform);

        let ack = service
            .acknowledge_session(
                "s1",
                AckRequest {
                    extend_seconds: 300,
                    reason: "still supervising".to_string(),
                },
                now,
            )
            .unwrap();

        assert_eq!(ack.session_key, "codex:s1");
        assert_eq!(ack.extend_seconds, 60);
        let stored = active_session_ack(&cfg.service.state_dir, "codex:s1", now).unwrap();
        assert!(stored.is_some());
        let snapshot = build_snapshot_with_processes(
            &cfg,
            Some(&platform.capture().unwrap()),
            &events,
            Vec::new(),
            now,
        );
        assert_eq!(snapshot.sessions[0].state, "acknowledged");
        assert_eq!(snapshot.sessions[0].action_state, "acknowledged");
        assert!(!snapshot.sessions[0].actionable);
        assert!(!snapshot.sessions[0].can_acknowledge);
    }

    #[test]
    fn stop_session_revalidates_identity_and_terminates_tree() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = crate::config::Mode::Enforcement;
        cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
        cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let events = vec![event("codex", "s1", now, 250)];
        let root = process(now, 100, "codex", "/repo");
        let mut child = process(now, 101, "node", "/repo");
        child.ppid = Some(platform::Pid::new(100));
        let platform = FakePlatform::new(platform::Snapshot::new([root.clone(), child]));
        let service = Service::new(&cfg, &events, &platform);

        let view = service
            .stop_session("s1", stop_request_for(&root), now)
            .unwrap();

        assert_eq!(view.session_key, "codex:s1");
        assert_eq!(view.agent_id, "codex-cli");
        assert_eq!(view.scope_pids, vec![101, 100]);
        assert_eq!(*platform.terminated.lock().unwrap(), vec![vec![101, 100]]);
    }

    #[test]
    fn stop_session_records_structured_termination_result_errors() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = crate::config::Mode::Enforcement;
        cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
        cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let events = vec![event("codex", "s1", now, 250)];
        let root = process(now, 100, "codex", "/repo");
        let platform = FakePlatform::new(platform::Snapshot::new([root.clone()]))
            .with_terminate_error("unsupported in this slice");
        let service = Service::new(&cfg, &events, &platform);

        let view = service
            .stop_session("s1", stop_request_for(&root), now)
            .unwrap();

        assert_eq!(
            view.result.errors,
            vec!["unsupported in this slice".to_string()]
        );
        let events = crate::ledger::read(cfg.ledger.path.clone()).unwrap();
        assert_eq!(events[0].event_type, "manual_stop_started");
        assert_eq!(events[1].event_type, "manual_stop_completed");
        assert_eq!(
            events[1].data.as_ref().unwrap().get("result").unwrap(),
            "completed"
        );
    }

    #[test]
    fn stop_session_treats_process_capture_failure_as_stop_conflict() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = crate::config::Mode::Enforcement;
        cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
        cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let events = vec![event("codex", "s1", now, 250)];
        let root = process(now, 100, "codex", "/repo");
        let platform = FakePlatform::capture_error("ps unavailable");
        let service = Service::new(&cfg, &events, &platform);

        let err = service
            .stop_session("s1", stop_request_for(&root), now)
            .unwrap_err();

        assert!(matches!(err, ServiceError::StopConflict(_)));
        assert!(platform.terminated.lock().unwrap().is_empty());
        assert!(
            crate::ledger::read(cfg.ledger.path.clone())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn stop_session_rejects_stale_identity_without_terminating() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = crate::config::Mode::Enforcement;
        cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
        cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let events = vec![event("codex", "s1", now, 250)];
        let root = process(now, 100, "codex", "/repo");
        let platform = FakePlatform::new(platform::Snapshot::new([root.clone()]));
        let service = Service::new(&cfg, &events, &platform);
        let mut request = stop_request_for(&root);
        request.expected.started_at = root.started_at.map(|at| at - chrono::Duration::seconds(1));

        let err = service.stop_session("s1", request, now).unwrap_err();

        assert!(matches!(err, ServiceError::StopConflict(_)));
        assert!(platform.terminated.lock().unwrap().is_empty());
    }

    #[test]
    fn stop_session_rejects_watch_only_uncorrelated_acknowledged_and_alert_mode() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let events = vec![event("codex", "s1", now, 250)];
        let root = process(now, 100, "codex", "/repo");

        let mut alert_cfg = Config::load("configs/curb.example.yaml").unwrap();
        alert_cfg.mode = crate::config::Mode::Alert;
        alert_cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
        alert_cfg.ledger.path = alert_cfg.service.state_dir.join("runs.ndjson");
        alert_cfg.usage.warn_turn_tokens = 100;
        alert_cfg.usage.kill_turn_tokens = 200;
        let alert_platform = FakePlatform::new(platform::Snapshot::new([root.clone()]));
        assert!(matches!(
            Service::new(&alert_cfg, &events, &alert_platform).stop_session(
                "s1",
                stop_request_for(&root),
                now
            ),
            Err(ServiceError::StopConflict(_))
        ));

        let mut uncorrelated_cfg = alert_cfg.clone();
        uncorrelated_cfg.mode = crate::config::Mode::Enforcement;
        let other = process(now, 100, "codex", "/other");
        let uncorrelated_platform = FakePlatform::new(platform::Snapshot::new([other.clone()]));
        assert!(matches!(
            Service::new(&uncorrelated_cfg, &events, &uncorrelated_platform).stop_session(
                "s1",
                stop_request_for(&other),
                now
            ),
            Err(ServiceError::StopConflict(_))
        ));

        let mut watch_cfg = uncorrelated_cfg.clone();
        watch_cfg.agents[1].kind = crate::config::AgentKind::App;
        let watch_platform = FakePlatform::new(platform::Snapshot::new([root.clone()]));
        assert!(matches!(
            Service::new(&watch_cfg, &events, &watch_platform).stop_session(
                "s1",
                stop_request_for(&root),
                now
            ),
            Err(ServiceError::StopConflict(_))
        ));

        let ack_cfg = uncorrelated_cfg;
        write_session_ack(
            &ack_cfg.service.state_dir,
            "codex:s1",
            std::time::Duration::from_secs(60),
            "still supervising",
            now,
        )
        .unwrap();
        let ack_platform = FakePlatform::new(platform::Snapshot::new([root.clone()]));
        assert!(matches!(
            Service::new(&ack_cfg, &events, &ack_platform).stop_session(
                "s1",
                stop_request_for(&root),
                now
            ),
            Err(ServiceError::StopConflict(_))
        ));
    }

    #[test]
    fn old_high_usage_is_idle_high_not_active_stop() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let old = now - chrono::Duration::hours(2);
        let snapshot = build_snapshot(&cfg, &[event("codex", "s1", old, 250)], Vec::new(), now);

        assert_eq!(snapshot.overview.status, "OK");
        assert_eq!(snapshot.overview.idle_high_sessions, 1);
        assert_eq!(snapshot.sessions[0].usage_state, "quiet-high");
        assert_eq!(snapshot.sessions[0].action_state, "none");
    }

    #[test]
    fn project_name_handles_unix_and_windows_paths() {
        assert_eq!(
            project_name(Path::new("/Users/me/repo")).as_deref(),
            Some("repo")
        );
        assert_eq!(
            project_name(Path::new(r"C:\Users\me\repo\")).as_deref(),
            Some("repo")
        );
        assert_eq!(
            project_name(Path::new(r"C:\Users\me/repo")).as_deref(),
            Some("repo")
        );
        assert_eq!(project_name(Path::new("/")).as_deref(), None);
    }

    #[test]
    fn running_for_seconds_clamps_future_and_omits_missing_start() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        assert_eq!(
            running_for_seconds(Some(now - chrono::Duration::seconds(42)), now),
            Some(42)
        );
        assert_eq!(
            running_for_seconds(Some(now + chrono::Duration::seconds(42)), now),
            Some(0)
        );
        assert_eq!(running_for_seconds(None, now), None);
    }

    #[test]
    fn event_views_classify_ledger_events_and_apply_limits() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let events = vec![
            ledger_event("service_started", 1, now),
            ledger_event("usage_warning", 2, now + chrono::Duration::seconds(1)),
            ledger_event(
                "usage_would_terminate",
                3,
                now + chrono::Duration::seconds(2),
            ),
            ledger_event(
                "session_ack_received",
                4,
                now + chrono::Duration::seconds(3),
            ),
        ];

        let views = event_views(&events, 3);

        assert_eq!(views.len(), 3);
        assert_eq!(
            (views[0].category.as_str(), views[0].kind.as_str()),
            ("alert", "warning")
        );
        assert_eq!(
            (views[1].category.as_str(), views[1].kind.as_str()),
            ("alert", "would_stop")
        );
        assert_eq!(
            (views[2].category.as_str(), views[2].kind.as_str()),
            ("ack", "received")
        );
        assert_eq!(views[2].message, "Acknowledgement received.");
    }

    #[test]
    fn alert_views_filter_limit_order_and_project_session_actions() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let snapshot = build_snapshot_with_processes(
            &cfg,
            Some(&process_snapshot(now, "codex", "/repo")),
            &[event("codex", "s1", now, 150)],
            Vec::new(),
            now,
        );
        let events = vec![
            ledger_event("run_started", 1, now),
            ledger_event("usage_warning", 2, now + chrono::Duration::seconds(1))
                .with_data(alert_data("codex", "s1", "/repo")),
            ledger_event(
                "usage_would_terminate",
                3,
                now + chrono::Duration::seconds(2),
            )
            .with_message("would stop"),
            ledger_event(
                "usage_termination_completed",
                4,
                now + chrono::Duration::seconds(3),
            ),
        ];

        let alerts = alert_views(&events, Some(&snapshot), 2);

        assert_eq!(alerts.len(), 2);
        assert_eq!(alerts[0].category, "would_stop");
        assert_eq!(alerts[0].severity, "watch");
        assert_eq!(alerts[1].category, "stopped");
        assert_eq!(alerts[1].severity, "stop");
        assert!(alerts[1].actionable);

        let all = alert_views(&events, Some(&snapshot), 0);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].session_key.as_deref(), Some("codex:s1"));
        assert!(all[0].can_acknowledge);
        assert_eq!(all[0].cwd.as_deref(), Some("/repo"));
    }

    #[test]
    fn alert_views_do_not_ack_missing_or_already_acknowledged_sessions() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
        cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        write_session_ack(
            &cfg.service.state_dir,
            "codex:s1",
            std::time::Duration::from_secs(60),
            "handled",
            now,
        )
        .unwrap();
        let snapshot = build_snapshot_with_processes(
            &cfg,
            Some(&process_snapshot(now, "codex", "/repo")),
            &[event("codex", "s1", now, 150)],
            Vec::new(),
            now,
        );
        let events = vec![
            ledger_event("usage_warning", 1, now).with_data(alert_data("codex", "s1", "/repo")),
            ledger_event("usage_warning", 2, now)
                .with_data(alert_data("codex", "missing", "/repo")),
        ];

        let alerts = alert_views(&events, Some(&snapshot), 0);

        assert_eq!(alerts.len(), 2);
        assert_eq!(alerts[0].session_key.as_deref(), Some("codex:s1"));
        assert!(!alerts[0].can_acknowledge);
        assert_eq!(alerts[1].session_key, None);
        assert!(!alerts[1].can_acknowledge);
    }

    fn event(provider: &str, session: &str, at: DateTime<Utc>, total: i64) -> Event {
        Event {
            provider: provider.to_string(),
            source: "test".to_string(),
            source_path: "fixture.jsonl".into(),
            session_id: Some(session.to_string()),
            turn_id: None,
            request_id: None,
            model: Some("model".to_string()),
            cwd: Some("/repo".into()),
            timestamp: Some(at),
            input_tokens: total,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            total_tokens: total,
            cumulative_tokens: total,
            model_context_window: 0,
        }
    }

    fn ledger_event(event_type: &str, seq: i64, at: DateTime<Utc>) -> ledger::Event {
        ledger::Event {
            event_type: event_type.to_string(),
            seq,
            ts: at,
            run_id: None,
            agent_id: Some("codex-cli".to_string()),
            mode: Some("alert".to_string()),
            message: None,
            data: None,
            prev_hash: None,
            event_hash: None,
        }
    }

    fn alert_data(provider: &str, session_id: &str, cwd: &str) -> Map<String, Value> {
        let mut data = Map::new();
        data.insert("provider".to_string(), Value::String(provider.to_string()));
        data.insert(
            "session_id".to_string(),
            Value::String(session_id.to_string()),
        );
        data.insert("cwd".to_string(), Value::String(cwd.to_string()));
        data
    }

    fn process_snapshot(now: DateTime<Utc>, name: &str, cwd: &str) -> platform::Snapshot {
        platform::Snapshot::new([process(now, 100, name, cwd)])
    }

    fn process(now: DateTime<Utc>, pid: i32, name: &str, cwd: &str) -> platform::Process {
        platform::Process {
            pid: platform::Pid::new(pid),
            ppid: None,
            name: name.to_string(),
            executable: Some(PathBuf::from("/usr/local/bin/codex")),
            command: name.to_string(),
            cwd: Some(PathBuf::from(cwd)),
            started_at: Some(now - chrono::Duration::minutes(10)),
            username: Some("tester".to_string()),
            bundle_id: None,
            team_id: None,
        }
    }

    fn stop_request_for(process: &platform::Process) -> StopRequest {
        StopRequest {
            confirm: true,
            scope: "tree".to_string(),
            reason: "test".to_string(),
            expected: StopExpectedIdentity {
                pid: process.pid.get(),
                started_at: process.started_at,
                owner: process.username.clone().unwrap_or_default(),
                executable: process.executable.clone(),
                bundle_id: process.bundle_id.clone(),
                team_id: process.team_id.clone(),
            },
        }
    }

    struct FakePlatform {
        capture: Result<platform::Snapshot, PlatformError>,
        terminated: Mutex<Vec<Vec<i32>>>,
        terminate_error: Option<String>,
    }

    impl FakePlatform {
        fn new(snapshot: platform::Snapshot) -> Self {
            Self {
                capture: Ok(snapshot),
                terminated: Mutex::new(Vec::new()),
                terminate_error: None,
            }
        }

        fn capture_error(message: &str) -> Self {
            Self {
                capture: Err(PlatformError::Capture(message.to_string())),
                terminated: Mutex::new(Vec::new()),
                terminate_error: None,
            }
        }

        fn with_terminate_error(mut self, message: &str) -> Self {
            self.terminate_error = Some(message.to_string());
            self
        }
    }

    impl Platform for FakePlatform {
        fn capture(&self) -> Result<platform::Snapshot, PlatformError> {
            self.capture.clone()
        }

        fn notification_capability(&self) -> platform::NotificationCapability {
            platform::NotificationCapability {
                supported: true,
                status: "available".to_string(),
                message: "available".to_string(),
            }
        }

        fn termination_capability(&self) -> platform::TerminationCapability {
            platform::TerminationCapability {
                supported: true,
                status: "available".to_string(),
                message: "test platform can terminate process trees".to_string(),
            }
        }

        fn notify(&self, _title: &str, _body: &str) -> Result<(), PlatformError> {
            Ok(())
        }

        fn terminate(
            &self,
            target: &TerminationTarget,
            _grace: std::time::Duration,
        ) -> platform::TerminationResult {
            if let Some(message) = &self.terminate_error {
                return platform::TerminationResult {
                    errors: vec![message.clone()],
                    ..platform::TerminationResult::default()
                };
            }
            self.terminated
                .lock()
                .unwrap()
                .push(target.scope().iter().map(|pid| pid.get()).collect());
            platform::TerminationResult {
                soft_signaled: target.scope().iter().map(|pid| pid.get()).collect(),
                ..platform::TerminationResult::default()
            }
        }
    }

    trait EventTestExt {
        fn with_cwd(self, cwd: &str) -> Self;
    }

    impl EventTestExt for Event {
        fn with_cwd(mut self, cwd: &str) -> Self {
            self.cwd = Some(PathBuf::from(cwd));
            self
        }
    }
}
