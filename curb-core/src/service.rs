use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::{Agent, Config, HumanDuration, Mode};
use crate::ledger;
use crate::onboarding::PlatformCapabilities;
use crate::platform;
use crate::usage::{Event, EventKind, SourceReport};

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

/// Machine-wide rollup. `status` is the single colour of the dashboard and is
/// exactly the worst `SessionView.alert` raised to upper case: no session over a
/// line → `OK`, any `warn` → `WATCH`, any `kill` → `ACTION`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Overview {
    pub mode: String,
    pub status: String,
    pub message: String,
    pub working: usize,
    pub warn: usize,
    pub kill: usize,
    pub busiest_turn_tokens: i64,
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

/// One agent working in one directory — the unit the dashboard shows.
///
/// Three facts answer everything: `turn_tokens` (what it has spent since you
/// last steered it), `status` (`working` or `idle`), and `alert` (`ok`, `warn`,
/// or `kill`). The process-identity fields are present only when Curb has
/// matched a live worker, and exist solely so a stop can be revalidated.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionView {
    pub key: String,
    pub id: String,
    pub provider: String,
    pub status: String,
    pub alert: String,
    pub can_stop: bool,
    pub can_acknowledge: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledged_until: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    pub models: Vec<String>,
    pub turn_tokens: i64,
    pub turn_context_tokens: i64,
    pub total_tokens: i64,
    pub calls: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    pub explanation: String,
}

/// A live worker process Curb matched. Most are correlated to a session and
/// shown through it; this view backs the "N agents running" count and the
/// terminal dashboard.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentView {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub status: String,
    pub pid: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running_for_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_key: Option<String>,
    pub turn_tokens: i64,
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
    pub spent_tokens: i64,
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
    pub escalate_supervised: bool,
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
    pub escalate_supervised: Option<bool>,
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
        escalate_supervised: cfg.usage.escalate_supervised,
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
    if let Some(enabled) = update.escalate_supervised {
        cfg.usage.escalate_supervised = enabled;
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
    let view = ledger::LedgerEvent::parse(&event.event_type)
        .map_or(DEFAULT_VIEW_CLASS, ledger::LedgerEvent::view_class);
    EventView {
        seq: event.seq,
        at: event.ts,
        category: view.category.to_string(),
        kind: view.kind.to_string(),
        message: event
            .message
            .clone()
            .unwrap_or_else(|| default_event_message(view.category, view.kind)),
        run_id: event.run_id.clone(),
        agent_id: event.agent_id.clone(),
        mode: event.mode.clone(),
    }
}

const DEFAULT_VIEW_CLASS: ledger::ViewClass = ledger::ViewClass {
    category: "other",
    kind: "recorded",
};

fn new_alert_view(event: &ledger::Event) -> Option<AlertView> {
    let parsed = ledger::LedgerEvent::parse(&event.event_type)?;
    if !parsed.is_alert() {
        return None;
    }
    let alert = parsed.alert_class();
    Some(AlertView {
        severity: alert.severity.to_string(),
        label: alert.label.to_string(),
        category: alert.category.to_string(),
        message: event
            .message
            .clone()
            .unwrap_or_else(|| default_alert_message(alert.category).to_string()),
        at: event.ts,
        seq: event.seq,
        run_id: event.run_id.clone(),
        agent_id: event.agent_id.clone(),
        provider: string_data(event, "provider"),
        mode: event.mode.clone(),
        cwd: string_data(event, "cwd"),
        session_key: None,
        session_id: string_data(event, "session_id"),
        actionable: alert.actionable,
        can_acknowledge: false,
        explanation: alert.explanation.to_string(),
    })
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

fn default_alert_message(category: &str) -> &'static str {
    match category {
        "stopped" => "Curb stopped a correlated worker.",
        "stopping" => "Curb started stopping a correlated worker.",
        "grace" => "Curb started an enforcement grace period.",
        "would_stop" => "Curb would stop a correlated worker in enforcement mode.",
        "blocked" => "Curb blocked termination for an uncorrelated or protected process.",
        "failed" => "Curb could not complete a policy action.",
        _ => "Usage or runtime crossed policy.",
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
        delta.tokens_added += turn.spent_tokens;
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
    session.alert != "ok"
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
    pub(crate) latest_spent_tokens: i64,
    pub(crate) window_spent_tokens: i64,
    pub(crate) total_tokens: i64,
    turns: Vec<TurnView>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProcessMatch {
    agent: Agent,
    process: platform::Process,
    confidence: i64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Correlation {
    pub(crate) matched: bool,
    pub(crate) agent: Option<Agent>,
    pub(crate) process: Option<platform::Process>,
    pub(crate) score: i64,
    pub(crate) reason: String,
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
    build_snapshot_filtered(cfg, processes, events, sources, now, &BTreeSet::new())
}

/// Like [`build_snapshot_with_processes`], but drops sessions Curb has already
/// terminated (`terminated`) so a killed agent leaves the dashboard at once
/// instead of lingering on log recency until the window expires.
pub fn build_snapshot_filtered(
    cfg: &Config,
    processes: Option<&platform::Snapshot>,
    events: &[Event],
    sources: Vec<SourceReport>,
    now: DateTime<Utc>,
    terminated: &BTreeSet<String>,
) -> Snapshot {
    let window_start = now - chrono::Duration::from_std(cfg.usage.window.as_std()).unwrap();
    let fresh_start = usage_activity_start(cfg, now);
    let sessions = build_sessions(events, window_start)
        .into_iter()
        .filter(|session| !terminated.contains(&session.key))
        .collect::<Vec<_>>();
    let matches = processes
        .map(|snapshot| process_matches(cfg, snapshot))
        .unwrap_or_default();

    let mut session_views = sessions
        .iter()
        .map(|session| {
            let correlation = correlate(session, &matches);
            build_session_view(cfg, session, &correlation, window_start, fresh_start, now)
        })
        .collect::<Vec<_>>();
    sort_session_views(&mut session_views);

    let mut agent_views = matches
        .iter()
        .map(|matched| {
            let best = best_session_for_match(matched, &sessions);
            let session_view = best.as_ref().map(|session| {
                let correlation = correlate(session, std::slice::from_ref(matched));
                build_session_view(cfg, session, &correlation, window_start, fresh_start, now)
            });
            build_agent_view(matched, session_view.as_ref(), now)
        })
        .collect::<Vec<_>>();
    sort_agent_views(&mut agent_views);

    let turns = sessions
        .iter()
        .flat_map(|session| session.turns.clone())
        .collect::<Vec<_>>();
    let overview = build_overview(cfg, &session_views, sources, now);
    Snapshot {
        overview,
        agents: agent_views,
        sessions: session_views,
        turns,
    }
}

pub(crate) fn build_sessions(events: &[Event], window_start: DateTime<Utc>) -> Vec<Session> {
    let mut by_key: HashMap<String, Session> = HashMap::new();
    let mut ordered = events.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|event| {
        (
            event.provider.as_str(),
            event.session_id.as_deref().unwrap_or_default(),
            event.timestamp,
            event.cumulative_tokens,
            event.total_tokens,
        )
    });
    for event in ordered {
        let id = event.session_id.clone().unwrap_or_default();
        let key = if id.is_empty() {
            format!("{}:{}", event.provider, event.source_path.display())
        } else {
            format!("{}:{id}", event.provider)
        };
        let spent_tokens = event_spent_tokens(event);
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
            latest_spent_tokens: 0,
            window_spent_tokens: 0,
            total_tokens: 0,
            turns: Vec::new(),
        });
        if session.cwd.is_none() {
            session.cwd = event.cwd.clone();
        }
        if event.timestamp > session.last {
            session.last = event.timestamp;
        }
        // A user-input boundary ends the previous turn. The next checkpoints
        // accumulate into a fresh turn, so `latest_*` always reflects spend
        // since the human last steered the agent.
        if matches!(event.kind, EventKind::UserInput) {
            session.latest_turn_tokens = 0;
            session.latest_spent_tokens = 0;
            continue;
        }
        if let Some(model) = &event.model {
            session.models.insert(model.clone());
        }
        if event.total_tokens > 0 && event.timestamp >= session.last_usage {
            session.last_usage = event.timestamp;
        }
        session.latest_turn_tokens += event.total_tokens;
        session.latest_spent_tokens += spent_tokens;
        if event.timestamp.is_some_and(|at| at >= window_start) {
            session.window_spent_tokens += spent_tokens;
        }
        session.calls += 1;
        session.total_tokens += spent_tokens;
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
            spent_tokens,
            cumulative_tokens: event.cumulative_tokens,
            source: event.source.clone(),
        });
    }
    by_key.into_values().collect()
}

fn event_spent_tokens(event: &Event) -> i64 {
    event.spent_tokens.max(0)
}

pub(crate) fn find_session(events: &[Event], key: &str) -> Option<Session> {
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
    turns.sort_by_key(|right| std::cmp::Reverse(right.at));
    if limit > 0 && turns.len() > limit {
        turns.truncate(limit);
    }
    Ok(turns)
}

// This is a READ for snapshot derivation: `build_session_view` needs the
// `acknowledged_until` to compute the alert/can_stop flags. All ack *mutation*
// (write/delete/rollback) lives in `write_path`; only this read stays beside the
// read model it serves.
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

pub(crate) fn session_ack_path(state_dir: &Path, session_key: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(session_key.as_bytes());
    state_dir
        .join("usage-acks")
        .join(format!("{}.json", hex::encode(hasher.finalize())))
}

pub(crate) fn build_session_view(
    cfg: &Config,
    session: &Session,
    correlation: &Correlation,
    window_start: DateTime<Utc>,
    fresh_start: DateTime<Utc>,
    now: DateTime<Utc>,
) -> SessionView {
    let recent = session.last_usage.is_some_and(|last| last >= window_start);
    let working = session.last_usage.is_some_and(|last| last >= fresh_start);
    let over_kill = session.latest_spent_tokens >= cfg.usage.kill_turn_tokens;
    let over_warn = session.latest_spent_tokens >= cfg.usage.warn_turn_tokens;
    let enforceable = correlation
        .agent
        .as_ref()
        .is_some_and(|agent| agent.can_terminate(cfg.usage.escalate_supervised));
    let supervised = correlation.agent.as_ref().is_some_and(Agent::is_supervised);
    let acknowledged_until = active_session_ack(&cfg.service.state_dir, &session.key, now)
        .ok()
        .flatten()
        .map(|ack| ack.until);
    let acknowledged = acknowledged_until.is_some();

    let alert = if acknowledged {
        "ok"
    } else if recent && over_kill {
        "kill"
    } else if recent && over_warn {
        "warn"
    } else {
        "ok"
    };
    let status = if working { "working" } else { "idle" };
    let can_stop =
        alert == "kill" && cfg.mode == Mode::Enforcement && correlation.matched && enforceable;
    let can_acknowledge = !acknowledged && recent && (over_kill || over_warn);
    let explanation = session_explanation(
        alert,
        status,
        &Explanation {
            matched: correlation.matched,
            enforceable,
            supervised,
            mode: cfg.mode,
            acknowledged,
            over_limit: recent && (over_kill || over_warn),
        },
    );
    let process = correlation.process.as_ref();

    SessionView {
        key: session.key.clone(),
        id: session.id.clone(),
        provider: session.provider.clone(),
        status: status.to_string(),
        alert: alert.to_string(),
        can_stop,
        can_acknowledge,
        acknowledged_until,
        project: session.cwd.as_ref().and_then(|cwd| project_name(cwd)),
        cwd: session.cwd.clone(),
        models: session.models.iter().cloned().collect(),
        turn_tokens: session.latest_spent_tokens,
        turn_context_tokens: session.latest_turn_tokens,
        total_tokens: session.total_tokens,
        calls: session.calls,
        last_activity_at: session.last_usage.or(session.last),
        pid: process.map(|process| process.pid.get()),
        process_started_at: process.and_then(|process| process.started_at),
        owner: process.and_then(|process| process.username.clone()),
        executable: process.and_then(|process| process.executable.clone()),
        bundle_id: process.and_then(|process| process.bundle_id.clone()),
        team_id: process.and_then(|process| process.team_id.clone()),
        explanation,
    }
}

/// Inputs for the one-line row explanation, grouped to keep the signature small.
struct Explanation {
    matched: bool,
    enforceable: bool,
    supervised: bool,
    mode: Mode,
    acknowledged: bool,
    over_limit: bool,
}

/// One plain sentence explaining the row's `alert`/`status`. This is the only
/// place state is turned into prose, so the language stays consistent.
fn session_explanation(alert: &str, status: &str, ctx: &Explanation) -> String {
    if ctx.acknowledged && ctx.over_limit {
        return "Over your limit, but acknowledged.".to_string();
    }
    match alert {
        "kill" if !ctx.matched => "Over your kill line, but no live process matched to stop.",
        "kill" if ctx.supervised && !ctx.enforceable => {
            "Over your kill line, but a desktop app supervises this task and would respawn it — Curb can warn but not stop it."
        }
        "kill" if !ctx.enforceable => "Over your kill line, but this is a watch-only desktop app.",
        "kill" if ctx.mode != Mode::Enforcement => "Over your kill line. Watch mode only warns.",
        "kill" => "Over your kill line — stopping after the grace period.",
        "warn" => "Over your warn line this turn.",
        _ => match status {
            "working" => "Working.",
            _ => "Idle between turns.",
        },
    }
    .to_string()
}

fn build_overview(
    cfg: &Config,
    sessions: &[SessionView],
    sources: Vec<SourceReport>,
    now: DateTime<Utc>,
) -> Overview {
    let working = sessions
        .iter()
        .filter(|session| session.status == "working")
        .count();
    let warn = sessions
        .iter()
        .filter(|session| session.alert == "warn")
        .count();
    let kill = sessions
        .iter()
        .filter(|session| session.alert == "kill")
        .count();
    let busiest_turn_tokens = sessions
        .iter()
        .filter(|session| session.status == "working")
        .map(|session| session.turn_tokens)
        .max()
        .unwrap_or(0);
    let status = if kill > 0 {
        "ACTION"
    } else if warn > 0 {
        "WATCH"
    } else {
        "OK"
    };
    Overview {
        mode: mode_label(cfg.mode),
        status: status.to_string(),
        message: overview_message(status, working, warn, kill),
        working,
        warn,
        kill,
        busiest_turn_tokens,
        last_scan: now,
        sources,
        changes: OverviewDelta::default(),
        capabilities: PlatformCapabilities::default(),
    }
}

/// The two operating modes the product exposes: watch (warn only) and enforce
/// (warn, then stop runaways).
pub(crate) fn mode_label(mode: Mode) -> String {
    match mode {
        Mode::Enforcement => "enforce",
        _ => "watch",
    }
    .to_string()
}

fn overview_message(status: &str, working: usize, warn: usize, kill: usize) -> String {
    let plural = |count: usize| if count == 1 { "agent" } else { "agents" };
    match status {
        "ACTION" => format!("{kill} over the kill line"),
        "WATCH" => format!("{warn} over the warn line"),
        _ if working > 0 => format!("{working} {} working", plural(working)),
        _ => "Nothing spending".to_string(),
    }
}

/// An agent is "working" while it is in a turn — chewing tokens since your last
/// input — not just at the instant of a model call. A turn is one continuous
/// spending episode with gaps between model calls (tool runs, thinking), so the
/// freshness window spans those gaps: a worker stays "working" until it has been
/// quiet for the whole window, rather than flickering idle between calls. The
/// trade-off is that a finished agent reads "working" until the window elapses.
const FRESH_WINDOW_SECS: u64 = 120;

pub(crate) fn usage_activity_start(cfg: &Config, now: DateTime<Utc>) -> DateTime<Utc> {
    let scan = cfg.usage.scan_interval.as_std();
    let freshness =
        std::time::Duration::from_secs(scan.as_secs().saturating_mul(3).max(FRESH_WINDOW_SECS));
    now - chrono::Duration::from_std(freshness).unwrap()
}

pub(crate) fn process_matches(cfg: &Config, snapshot: &platform::Snapshot) -> Vec<ProcessMatch> {
    let mut matches = Vec::new();
    for process in snapshot.processes() {
        for agent in &cfg.agents {
            let confidence = match_agent(agent, process, snapshot);
            if confidence >= cfg.service.min_confidence {
                matches.push(ProcessMatch {
                    agent: agent.clone(),
                    process: process.clone(),
                    confidence,
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

/// Score how strongly a process looks like a configured agent. Exclusion and
/// require filters veto a match (score 0); positive signals add confidence.
fn match_agent(agent: &Agent, process: &platform::Process, snapshot: &platform::Snapshot) -> i64 {
    let matcher = &agent.matcher;
    let excluded = matcher
        .exclude_names
        .iter()
        .any(|name| name.eq_ignore_ascii_case(&process.name))
        || any_regex_matches(&matcher.exclude_command_regex, &process.command)
        || process
            .ppid
            .and_then(|pid| snapshot.process(pid))
            .is_some_and(|parent| {
                any_regex_matches(&matcher.exclude_parent_regex, &parent.command)
            });
    let missing_required = !matcher.require_command_regex.is_empty()
        && !matcher
            .require_command_regex
            .iter()
            .all(|pattern| regex_matches(pattern, &process.command));
    if excluded || missing_required {
        return 0;
    }

    let mut confidence = 0;
    if matcher
        .process_names
        .iter()
        .any(|name| name.eq_ignore_ascii_case(&process.name))
    {
        confidence += 50;
    }
    if any_regex_matches(&matcher.command_regex, &process.command) {
        confidence += 40;
    }
    if process.executable.as_ref().is_some_and(|executable| {
        matcher
            .executable_paths
            .iter()
            .any(|path| std::path::Path::new(path) == executable.as_path())
    }) {
        confidence += 80;
    }
    if process.bundle_id.as_ref().is_some_and(|bundle_id| {
        matcher
            .bundle_ids
            .iter()
            .any(|expected| expected == bundle_id)
    }) {
        confidence += 80;
    }
    confidence
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
            correlation.matched.then_some((
                correlation.score,
                session.last_usage,
                session.last,
                session,
            ))
        })
        .max_by_key(|(score, last_usage, last, _)| (*score, *last_usage, *last))
        .map(|(_, _, _, session)| session)
}

fn build_agent_view(
    matched: &ProcessMatch,
    session: Option<&SessionView>,
    now: DateTime<Utc>,
) -> AgentView {
    let working = session.is_some_and(|session| session.status == "working");
    let explanation = match session {
        Some(session) if working => session.explanation.clone(),
        Some(_) => "Running; its session is not spending right now.".to_string(),
        None if matched.agent.termination_allowed() => "Running, no usage matched yet.".to_string(),
        None => "Watch-only desktop app.".to_string(),
    };
    AgentView {
        id: matched.agent.id.clone(),
        provider: matched.agent.family.clone(),
        label: matched.agent.label.clone(),
        status: if working { "working" } else { "idle" }.to_string(),
        pid: matched.process.pid.get(),
        process_started_at: matched.process.started_at,
        running_for_seconds: running_for_seconds(matched.process.started_at, now),
        project: matched
            .process
            .cwd
            .as_ref()
            .and_then(|cwd| project_name(cwd)),
        cwd: matched.process.cwd.clone(),
        session_key: session.map(|session| session.key.clone()),
        turn_tokens: session.map_or(0, |session| session.turn_tokens),
        explanation,
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

/// Most urgent first: anything past kill, then warn, then working, then idle;
/// ties broken by current turn spend. Spend always outranks runtime.
fn sort_session_views(sessions: &mut [SessionView]) {
    let alert_rank = |alert: &str| match alert {
        "kill" => 0,
        "warn" => 1,
        _ => 2,
    };
    let status_rank = |status: &str| usize::from(status != "working");
    sessions.sort_by(|left, right| {
        alert_rank(&left.alert)
            .cmp(&alert_rank(&right.alert))
            .then_with(|| status_rank(&left.status).cmp(&status_rank(&right.status)))
            .then_with(|| right.turn_tokens.cmp(&left.turn_tokens))
            .then_with(|| right.last_activity_at.cmp(&left.last_activity_at))
    });
}

fn sort_agent_views(agents: &mut [AgentView]) {
    let rank = |status: &str| usize::from(status != "working");
    agents.sort_by(|left, right| {
        rank(&left.status)
            .cmp(&rank(&right.status))
            .then_with(|| left.project.cmp(&right.project))
            .then_with(|| left.id.cmp(&right.id))
    });
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

/// Correlate by working directory only when both paths are specific enough that
/// one containing the other is meaningful — never `/` or `/Users`.
fn safe_cwd_prefix_match(parent: &std::path::Path, child: &std::path::Path) -> bool {
    path_specificity(parent) >= 2 && path_specificity(child) >= 2 && child.starts_with(parent)
}

fn path_specificity(path: &std::path::Path) -> usize {
    path.components()
        .filter(|component| matches!(component, std::path::Component::Normal(_)))
        .count()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::Map;

    use super::*;
    use crate::config::Config;
    use crate::usage::Event;

    #[test]
    fn active_stop_session_is_actionable_only_in_enforcement_mode() {
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
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
        assert_eq!(snapshot.sessions[0].alert, "kill");
        assert!(snapshot.sessions[0].can_stop);
        assert_eq!(snapshot.sessions[0].pid, Some(100));
        assert_eq!(snapshot.sessions[0].project.as_deref(), Some("repo"));
        assert_eq!(snapshot.agents[0].project.as_deref(), Some("repo"));
        assert_eq!(snapshot.agents[0].running_for_seconds, Some(600));
    }

    #[test]
    fn alert_mode_reports_would_stop_without_actionability() {
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
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

        assert_eq!(snapshot.sessions[0].alert, "kill");
        assert!(!snapshot.sessions[0].can_stop);
    }

    #[test]
    fn uncorrelated_stop_usage_is_blocked_not_actionable() {
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
        cfg.mode = crate::config::Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let snapshot = build_snapshot(&cfg, &[event("codex", "s1", now, 250)], Vec::new(), now);

        assert_eq!(snapshot.overview.status, "ACTION");
        assert_eq!(snapshot.sessions[0].alert, "kill");
        assert_eq!(snapshot.sessions[0].pid, None);
        assert!(!snapshot.sessions[0].can_stop);
    }

    #[test]
    fn watch_only_app_match_blocks_termination() {
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
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

        assert_eq!(snapshot.sessions[0].alert, "kill");
        assert!(!snapshot.sessions[0].can_stop);
        assert_eq!(snapshot.sessions[0].pid, Some(100));
        assert!(snapshot.sessions[0].explanation.contains("watch-only"));
    }

    #[test]
    fn multiple_sessions_can_correlate_to_one_worker() {
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
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
                .all(|session| session.pid == Some(100))
        );
    }

    #[test]
    fn agent_view_uses_newest_matching_session_when_scores_tie() {
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
        cfg.usage.scan_interval = HumanDuration::seconds(5);
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let processes = process_snapshot(now, "codex", "/repo");
        let snapshot = build_snapshot_with_processes(
            &cfg,
            Some(&processes),
            &[
                event("codex", "stale", now - chrono::Duration::minutes(10), 150),
                event("codex", "fresh", now, 125),
            ],
            Vec::new(),
            now,
        );

        assert_eq!(
            snapshot.agents[0].session_key.as_deref(),
            Some("codex:fresh")
        );
        assert_eq!(snapshot.agents[0].status, "working");
    }

    #[test]
    fn overview_delta_reports_new_usage_alerts_agents_and_source_errors() {
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
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
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
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

        assert_eq!(snapshot.sessions[0].pid, Some(200));
    }

    #[test]
    fn cwd_prefix_correlation_rejects_root_or_top_level_paths() {
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
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

        assert_eq!(snapshot.sessions[0].pid, None);
        assert_eq!(snapshot.sessions[1].pid, None);
    }

    #[test]
    fn old_high_usage_is_idle_high_not_active_stop() {
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let old = now - chrono::Duration::hours(2);
        let snapshot = build_snapshot(&cfg, &[event("codex", "s1", old, 250)], Vec::new(), now);

        assert_eq!(snapshot.overview.status, "OK");
        assert_eq!(snapshot.sessions[0].alert, "ok");
        assert_eq!(snapshot.sessions[0].status, "idle");
    }

    #[test]
    fn terminated_sessions_are_dropped_from_the_snapshot() {
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let events = [
            event("codex", "s1", now, 250),
            event("codex", "s2", now, 150),
        ];

        let full = build_snapshot_with_processes(&cfg, None, &events, Vec::new(), now);
        assert_eq!(full.sessions.len(), 2);

        let terminated: std::collections::BTreeSet<String> =
            ["codex:s1".to_string()].into_iter().collect();
        let filtered = build_snapshot_filtered(&cfg, None, &events, Vec::new(), now, &terminated);
        assert_eq!(filtered.sessions.len(), 1);
        assert_eq!(filtered.sessions[0].key, "codex:s2");
    }

    #[test]
    fn stale_policy_warning_is_not_fresh_activity() {
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
        cfg.usage.scan_interval = HumanDuration::seconds(5);
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let stale_but_in_window = now - chrono::Duration::minutes(3);
        let processes = process_snapshot(now, "codex", "/repo");
        let snapshot = build_snapshot_with_processes(
            &cfg,
            Some(&processes),
            &[event("codex", "s1", stale_but_in_window, 150)],
            Vec::new(),
            now,
        );

        assert_eq!(snapshot.overview.status, "WATCH");
        assert_eq!(snapshot.overview.working, 0);
        assert_eq!(snapshot.sessions[0].alert, "warn");
        assert_eq!(snapshot.sessions[0].status, "idle");
        assert_eq!(snapshot.agents[0].status, "idle");
    }

    #[test]
    fn turn_spend_resets_after_a_user_input_boundary() {
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
        cfg.usage.scan_interval = HumanDuration::seconds(5);
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let processes = process_snapshot(now, "codex", "/repo");
        // An expensive past turn (250 > kill), then the human steered again,
        // then a cheap fresh turn (40 < warn). Only the fresh turn counts.
        let events = vec![
            event("codex", "s1", now - chrono::Duration::seconds(20), 250),
            user_input("codex", "s1", now - chrono::Duration::seconds(10)),
            event("codex", "s1", now - chrono::Duration::seconds(2), 40),
        ];
        let snapshot =
            build_snapshot_with_processes(&cfg, Some(&processes), &events, Vec::new(), now);

        assert_eq!(snapshot.sessions[0].turn_tokens, 40);
        assert_eq!(snapshot.sessions[0].alert, "ok");
    }

    #[test]
    fn process_matching_applies_parent_command_exclusions_to_parent() {
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
        cfg.service.min_confidence = 1;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let mut parent = process(now, 100, "codex", "/repo");
        parent.command =
            "/Applications/Codex.app/Contents/MacOS/codex app-server --listen stdio://".to_string();
        let mut child = process(now, 101, "claude", "/repo");
        child.ppid = Some(parent.pid);
        child.command = "/usr/local/bin/claude --print worker".to_string();
        let snapshot = platform::Snapshot::new([parent, child]);

        let matches = process_matches(&cfg, &snapshot);

        assert!(!matches.iter().any(|matched| {
            matched.agent.id == "claude-code" && matched.process.pid == platform::Pid::new(101)
        }));
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
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
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
        let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
        cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
        cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        crate::write_path::write_session_ack(
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
            kind: crate::usage::EventKind::TokenCheckpoint,
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
            spent_tokens: total,
            cumulative_tokens: total,
            model_context_window: 0,
        }
    }

    fn user_input(provider: &str, session: &str, at: DateTime<Utc>) -> Event {
        Event {
            kind: crate::usage::EventKind::UserInput,
            provider: provider.to_string(),
            source: "test".to_string(),
            source_path: "fixture.jsonl".into(),
            session_id: Some(session.to_string()),
            turn_id: None,
            request_id: None,
            model: None,
            cwd: Some("/repo".into()),
            timestamp: Some(at),
            input_tokens: 0,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            total_tokens: 0,
            spent_tokens: 0,
            cumulative_tokens: 0,
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
