use std::collections::{BTreeSet, HashMap};

use super::{AgentView, OverviewDelta, SessionView, Snapshot, TurnView};

pub fn annotate_overview_delta(previous: Option<&Snapshot>, mut next: Snapshot) -> Snapshot {
    next.overview.changes = previous
        .map(|previous| build_overview_delta(previous, &next))
        .unwrap_or_default();
    next
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
