use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use super::{Event, EventKind};

impl Event {
    fn dedup_key(&self) -> String {
        if matches!(self.kind, EventKind::UserInput) {
            return format!(
                "ui:{}:{}:{}",
                self.provider,
                self.session_id.as_deref().unwrap_or_default(),
                self.timestamp
                    .map(|timestamp| timestamp.to_rfc3339())
                    .unwrap_or_default(),
            );
        }
        match self.provider.as_str() {
            "codex" if self.cumulative_tokens != 0 || self.total_tokens != 0 => format!(
                "codex:{}:{}:{}",
                self.session_id.as_deref().unwrap_or_default(),
                self.cumulative_tokens,
                self.total_tokens
            ),
            "claude" if self.request_id.is_some() => {
                let request_id = self.request_id.as_deref().unwrap_or_default();
                format!(
                    "claude:{}:{}",
                    self.session_id.as_deref().unwrap_or_default(),
                    request_id
                )
            }
            _ => format!(
                "{}:{}:{}:{}:{}",
                self.provider,
                self.session_id.as_deref().unwrap_or_default(),
                self.request_id.as_deref().unwrap_or_default(),
                self.timestamp
                    .map(|timestamp| timestamp.to_rfc3339())
                    .unwrap_or_default(),
                self.source_path.display()
            ),
        }
    }
}

pub(super) fn user_input_event(
    provider: &str,
    source: &str,
    path: &Path,
    session_id: Option<String>,
    cwd: Option<PathBuf>,
    timestamp: Option<DateTime<Utc>>,
) -> Event {
    Event {
        kind: EventKind::UserInput,
        provider: provider.to_string(),
        source: source.to_string(),
        source_path: path.to_path_buf(),
        session_id,
        turn_id: None,
        request_id: None,
        model: None,
        cwd,
        timestamp,
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

pub(super) fn dedupe(events: Vec<Event>) -> Vec<Event> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for event in events {
        if !seen.insert(event.dedup_key()) {
            continue;
        }
        out.push(event);
    }
    out
}

pub(super) fn sort_events(events: &mut [Event]) {
    events.sort_by_key(|left| left.timestamp);
}

pub(super) fn event_is_since(event: &Event, since: Option<DateTime<Utc>>) -> bool {
    match (event.timestamp, since) {
        (_, None) | (None, Some(_)) => true,
        (Some(at), Some(since)) => at >= since,
    }
}

pub(super) fn parse_time(raw: Option<&str>) -> Option<DateTime<Utc>> {
    let raw = raw?;
    DateTime::parse_from_rfc3339(raw)
        .map(|time| time.with_timezone(&Utc))
        .ok()
}
