use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::config::{Agent, Config, Mode};
use crate::platform;
use crate::usage::{Event, SourceReport};

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
            build_session_view(cfg, session, &correlation, window_start)
        })
        .collect::<Vec<_>>();
    sort_session_views(&mut session_views);

    let mut agent_views = matches
        .iter()
        .map(|matched| {
            let best = best_session_for_match(matched, &sessions);
            let session_view = best.as_ref().map(|session| {
                let correlation = correlate(session, std::slice::from_ref(matched));
                build_session_view(cfg, session, &correlation, window_start)
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

fn build_session_view(
    cfg: &Config,
    session: &Session,
    correlation: &Correlation,
    window_start: DateTime<Utc>,
) -> SessionView {
    let active = session.last_usage.is_some_and(|last| last >= window_start);
    let over_stop = session.latest_turn_tokens >= cfg.usage.kill_turn_tokens;
    let over_warn = session.latest_turn_tokens >= cfg.usage.warn_turn_tokens;
    let matched_agent = correlation.agent.as_ref();
    let termination_allowed = matched_agent.is_some_and(Agent::termination_allowed);
    let (state, usage_state, action_state, risk_rank, explanation) = if active && over_stop {
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
        can_acknowledge: matches!(usage_state, "warn" | "stop"),
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

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::config::Config;
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
