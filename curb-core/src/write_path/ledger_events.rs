use serde_json::{Map, Value, json};

use crate::config::Mode;
use crate::ledger::{self, LedgerEvent};
use crate::platform;
use crate::service::{Correlation, Session, SessionAck};

pub(super) fn session_ack_event(ack: &SessionAck, extend: std::time::Duration) -> ledger::Event {
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
    ledger::Event::new(LedgerEvent::SessionAckReceived)
        .with_data(data)
        .with_message(ack.reason.clone())
}

pub(super) fn manual_stop_event(
    event_type: LedgerEvent,
    session: &Session,
    correlation: &Correlation,
    target: &platform::TerminationTarget,
    result: Option<&str>,
    reason: &str,
    mode: Mode,
) -> ledger::Event {
    let mut event = ledger::Event::new(event_type).with_data(manual_stop_event_data(
        session,
        correlation,
        target,
        result,
    ));
    event.agent_id = correlation.agent.as_ref().map(|agent| agent.id.clone());
    event.mode = Some(mode.to_string());
    if !reason.is_empty() {
        event.message = Some(reason.to_string());
    }
    event
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
    data.insert(
        "latest_spent_tokens".to_string(),
        json!(session.latest_spent_tokens),
    );
    data.insert(
        "window_spent_tokens".to_string(),
        json!(session.window_spent_tokens),
    );
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
