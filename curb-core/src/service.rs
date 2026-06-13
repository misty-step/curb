use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ledger;
use crate::onboarding::PlatformCapabilities;
use crate::platform;
use crate::usage::SourceReport;

mod ack_state;
mod config_model;
mod correlation;
mod delta;
mod events_model;
mod recovery;
mod snapshot_model;

pub(crate) use ack_state::session_ack_path;
pub use ack_state::{SessionAck, active_session_ack, read_session_ack};
pub use config_model::{
    ConfigAgentView, ConfigUpdate, ConfigView, apply_config_update, config_view,
};
pub(crate) use correlation::{Correlation, best_session_for_match, correlate, process_matches};
pub use delta::annotate_overview_delta;
pub use events_model::{alert_views, event_views};
pub(crate) use recovery::{sanitize_source_reports, source_health_recovery};
pub(crate) use snapshot_model::{
    Session, build_session_view, build_sessions, find_session, usage_activity_start,
};
pub use snapshot_model::{
    build_snapshot, build_snapshot_filtered, build_snapshot_with_processes, canonical_session_key,
    session_turns,
};
#[cfg(test)]
pub(crate) use snapshot_model::{project_name, running_for_seconds};

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
pub struct ReadinessView {
    pub status: String,
    pub app: String,
    pub api_version: u8,
    pub checks: Vec<ReadinessCheckView>,
    pub recovery: Vec<RecoveryItemView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReadinessCheckView {
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecoveryItemView {
    pub id: String,
    pub label: String,
    pub status: String,
    pub message: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runbook: Option<String>,
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
    pub recovery: Vec<RecoveryItemView>,
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

#[cfg(test)]
mod tests;
