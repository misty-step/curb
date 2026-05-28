use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::{Agent, Config, Mode};
use crate::platform::{self, Platform};
use crate::usage::{Event, SourceReport};

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("session not found")]
    SessionNotFound,
    #[error("invalid acknowledgement: {0}")]
    InvalidAck(String),
    #[error("invalid stop request: {0}")]
    InvalidStop(String),
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
pub struct AckRequest {
    pub extend_seconds: i64,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AckView {
    pub session_key: String,
    pub extend_seconds: i64,
    pub until: DateTime<Utc>,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionAck {
    pub session_key: String,
    pub reason: String,
    pub until: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
    pub owner: String,
    pub executable: Option<PathBuf>,
    pub bundle_id: Option<String>,
    pub team_id: Option<String>,
    pub scope: String,
    pub scope_pids: Vec<i32>,
    pub result: String,
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
        let ack = write_session_ack(
            &self.cfg.service.state_dir,
            &session.key,
            extend,
            &request.reason,
            now,
        )?;
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
        let snapshot = self.platform.capture()?;
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
        self.platform.terminate(&target)?;
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
            result: "terminated".to_string(),
        })
    }
}

#[derive(Clone, Debug)]
struct Session {
    key: String,
    id: String,
    provider: String,
    cwd: Option<PathBuf>,
    models: BTreeSet<String>,
    last: Option<DateTime<Utc>>,
    last_usage: Option<DateTime<Utc>>,
    calls: usize,
    latest_turn_tokens: i64,
    window_tokens: i64,
    total_tokens: i64,
    turns: Vec<TurnView>,
}

#[derive(Clone, Debug)]
struct ProcessMatch {
    agent: Agent,
    process: platform::Process,
    confidence: i64,
    evidence: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct Correlation {
    matched: bool,
    agent: Option<Agent>,
    process: Option<platform::Process>,
    score: i64,
    reason: String,
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

fn build_sessions(events: &[Event], window_start: DateTime<Utc>) -> Vec<Session> {
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
    }
}

fn process_matches(cfg: &Config, snapshot: &platform::Snapshot) -> Vec<ProcessMatch> {
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

fn correlate(session: &Session, matches: &[ProcessMatch]) -> Correlation {
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
        } else if path_contains(&process_cwd, &session_cwd)
            || path_contains(&session_cwd, &process_cwd)
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
    _now: DateTime<Utc>,
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
        cwd: matched.process.cwd.clone(),
        matched_by: matched.evidence.clone(),
        confidence: matched.confidence,
        latest_session_id,
        latest_turn_tokens,
        window_tokens,
        explanation: explanation.to_string(),
    }
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
    fn acknowledge_session_persists_and_suppresses_actionability() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
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
    fn stop_session_rejects_stale_identity_without_terminating() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = crate::config::Mode::Enforcement;
        cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
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
        snapshot: platform::Snapshot,
        terminated: Mutex<Vec<Vec<i32>>>,
    }

    impl FakePlatform {
        fn new(snapshot: platform::Snapshot) -> Self {
            Self {
                snapshot,
                terminated: Mutex::new(Vec::new()),
            }
        }
    }

    impl Platform for FakePlatform {
        fn capture(&self) -> Result<platform::Snapshot, PlatformError> {
            Ok(self.snapshot.clone())
        }

        fn notify(&self, _title: &str, _body: &str) -> Result<(), PlatformError> {
            Ok(())
        }

        fn terminate(&self, target: &TerminationTarget) -> Result<(), PlatformError> {
            self.terminated
                .lock()
                .unwrap()
                .push(target.scope().iter().map(|pid| pid.get()).collect());
            Ok(())
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
