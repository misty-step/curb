use serde_json::{Map, Value, json};

use super::PolicySession;

pub(super) fn event_data(session: &PolicySession, result: Option<Value>) -> Map<String, Value> {
    let target = &session.target;
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
    data.insert("calls".to_string(), json!(session.calls));
    data.insert("total_tokens".to_string(), json!(session.total_tokens));
    data.insert("turn_tokens".to_string(), json!(session.latest_turn_tokens));
    data.insert(
        "latest_spent_tokens".to_string(),
        json!(session.latest_spent_tokens),
    );
    data.insert(
        "window_spent_tokens".to_string(),
        json!(session.window_spent_tokens),
    );
    if let Some(last) = session.last {
        data.insert("last".to_string(), Value::String(last.to_rfc3339()));
    }
    if let Some(last_usage) = session.last_usage {
        data.insert(
            "last_usage".to_string(),
            Value::String(last_usage.to_rfc3339()),
        );
    }
    if !session.models.is_empty() {
        data.insert(
            "models".to_string(),
            Value::Array(session.models.iter().cloned().map(Value::String).collect()),
        );
    }
    if target.matched {
        if let Some(pid) = target.pid {
            data.insert("pid".to_string(), json!(pid));
        }
        if let Some(agent_id) = &target.agent_id {
            data.insert("agent_id".to_string(), Value::String(agent_id.clone()));
        }
        data.insert(
            "correlation".to_string(),
            Value::String(target.reason.clone()),
        );
        data.insert("correlation_score".to_string(), json!(target.score));
    }
    if let Some(result) = result {
        data.insert("result".to_string(), result);
    }
    data
}

pub(super) fn usage_message(session: &PolicySession) -> String {
    format!(
        "{} session {} latest checkpoint spent {} tokens ({} in window, {} calls)",
        session.provider,
        short_id(&session.id),
        format_tokens(session.latest_spent_tokens),
        format_tokens(session.window_spent_tokens),
        session.calls
    )
}

fn short_id(id: &str) -> String {
    if id.len() <= 12 {
        id.to_string()
    } else {
        format!("{}...{}", &id[..8], &id[id.len() - 4..])
    }
}

fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}
