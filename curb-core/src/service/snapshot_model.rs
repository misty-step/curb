use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use super::correlation::ProcessMatch;
use super::{
    AgentView, Correlation, Overview, OverviewDelta, ServiceError, SessionView, Snapshot, TurnView,
    active_session_ack, best_session_for_match, correlate, process_matches,
    sanitize_source_reports, source_health_recovery,
};
use crate::config::{Agent, Config, Mode};
use crate::onboarding::PlatformCapabilities;
use crate::platform;
use crate::usage::{Event, EventKind, SourceReport};

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
    let sources = sanitize_source_reports(sources);
    let recovery = source_health_recovery(&sources);
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
        recovery,
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

pub(crate) fn running_for_seconds(
    started_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<i64> {
    let started_at = started_at?;
    Some(now.signed_duration_since(started_at).num_seconds().max(0))
}

pub(crate) fn project_name(path: &Path) -> Option<String> {
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
