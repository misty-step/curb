use std::collections::HashMap;

use serde_json::Value;

use super::{AlertView, EventView, SessionView, Snapshot};
use crate::ledger;

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
