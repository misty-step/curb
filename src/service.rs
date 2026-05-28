use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::usage::{Event, SourceReport};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub overview: Overview,
    pub sessions: Vec<SessionView>,
    pub turns: Vec<TurnView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Overview {
    pub mode: String,
    pub status: String,
    pub message: String,
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
    pub risk_rank: i64,
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
    calls: usize,
    latest_turn_tokens: i64,
    window_tokens: i64,
    total_tokens: i64,
    turns: Vec<TurnView>,
}

pub fn build_snapshot(
    cfg: &Config,
    events: &[Event],
    sources: Vec<SourceReport>,
    now: DateTime<Utc>,
) -> Snapshot {
    let window_start = now - chrono::Duration::from_std(cfg.usage.window.as_std()).unwrap();
    let mut sessions = build_sessions(events, window_start);
    sessions.sort_by(|left, right| {
        right
            .latest_turn_tokens
            .cmp(&left.latest_turn_tokens)
            .then_with(|| right.last.cmp(&left.last))
    });

    let session_views = sessions
        .iter()
        .map(|session| build_session_view(cfg, session, window_start))
        .collect::<Vec<_>>();
    let turns = sessions
        .iter()
        .flat_map(|session| session.turns.clone())
        .collect::<Vec<_>>();
    let overview = build_overview(cfg, &session_views, sources, now);
    Snapshot {
        overview,
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

fn build_session_view(cfg: &Config, session: &Session, window_start: DateTime<Utc>) -> SessionView {
    let active = session.last.is_some_and(|last| last >= window_start);
    let over_stop = session.latest_turn_tokens >= cfg.usage.kill_turn_tokens;
    let over_warn = session.latest_turn_tokens >= cfg.usage.warn_turn_tokens;
    let (state, usage_state, action_state, risk_rank, explanation) = if active && over_stop {
        (
            "stop",
            "stop",
            if cfg.mode.to_string() == "enforcement" {
                "stop-pending"
            } else {
                "would-stop"
            },
            0,
            "latest turn crossed the stop threshold",
        )
    } else if active && over_warn {
        (
            "warn",
            "warn",
            "notify",
            1,
            "latest turn crossed the warning threshold",
        )
    } else if active {
        (
            "spending",
            "spending",
            "none",
            2,
            "recent token usage is within policy",
        )
    } else if over_stop || over_warn {
        (
            "idle-high",
            "quiet-high",
            "none",
            4,
            "historically high usage is quiet in the policy window",
        )
    } else {
        ("quiet", "quiet", "none", 5, "no recent token usage")
    };
    SessionView {
        key: session.key.clone(),
        id: session.id.clone(),
        provider: session.provider.clone(),
        state: state.to_string(),
        process_state: "unknown".to_string(),
        usage_state: usage_state.to_string(),
        action_state: action_state.to_string(),
        actionable: action_state == "stop-pending",
        can_acknowledge: matches!(usage_state, "warn" | "stop"),
        cwd: session.cwd.clone(),
        models: session.models.iter().cloned().collect(),
        last_seen_at: session.last.unwrap_or(window_start),
        last_usage_at: session.last,
        calls: session.calls,
        latest_turn_tokens: session.latest_turn_tokens,
        window_tokens: session.window_tokens,
        total_tokens: session.total_tokens,
        risk_rank,
        explanation: explanation.to_string(),
    }
}

fn build_overview(
    cfg: &Config,
    sessions: &[SessionView],
    sources: Vec<SourceReport>,
    now: DateTime<Utc>,
) -> Overview {
    let active_sessions = sessions
        .iter()
        .filter(|session| matches!(session.usage_state.as_str(), "spending" | "warn" | "stop"))
        .count();
    let warning_sessions = sessions
        .iter()
        .filter(|session| session.usage_state == "warn")
        .count();
    let stop_sessions = sessions
        .iter()
        .filter(|session| session.usage_state == "stop")
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
        active_sessions,
        warning_sessions,
        stop_sessions,
        idle_high_sessions,
        window_tokens: sessions.iter().map(|session| session.window_tokens).sum(),
        lookback_tokens: sessions.iter().map(|session| session.total_tokens).sum(),
        last_scan: now,
        sources,
    }
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
        let snapshot = build_snapshot(&cfg, &[event("codex", "s1", now, 250)], Vec::new(), now);

        assert_eq!(snapshot.overview.status, "ACTION");
        assert_eq!(snapshot.sessions[0].usage_state, "stop");
        assert_eq!(snapshot.sessions[0].action_state, "stop-pending");
        assert!(snapshot.sessions[0].actionable);
    }

    #[test]
    fn alert_mode_reports_would_stop_without_actionability() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = crate::config::Mode::Alert;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let snapshot = build_snapshot(&cfg, &[event("codex", "s1", now, 250)], Vec::new(), now);

        assert_eq!(snapshot.sessions[0].action_state, "would-stop");
        assert!(!snapshot.sessions[0].actionable);
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
}
