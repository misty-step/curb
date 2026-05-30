use std::collections::HashSet;
use std::io::{self, Write};
use std::path::Path;

use chrono::{DateTime, Duration, Local, Utc};
use thiserror::Error;

use crate::usage::{Event, Reader, UsageError};

#[derive(Debug, Error)]
pub enum TailError {
    #[error(transparent)]
    Usage(#[from] UsageError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TailScan {
    pub rendered: usize,
    pub source_error: Option<String>,
}

#[derive(Default)]
pub struct TailState {
    seen: HashSet<String>,
}

impl TailState {
    pub fn render_new_events(
        &mut self,
        writer: impl Write,
        events: &[Event],
        now: DateTime<Utc>,
    ) -> io::Result<usize> {
        render_new_events(writer, events, now, &mut self.seen)
    }
}

pub fn scan_once(
    reader: &Reader,
    state: &mut TailState,
    writer: impl Write,
    since: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<TailScan, TailError> {
    let scan = reader.scan_since(Some(since))?;
    let rendered = state.render_new_events(writer, &scan.events, now)?;
    Ok(TailScan {
        rendered,
        source_error: scan.error,
    })
}

pub fn render_new_events(
    mut writer: impl Write,
    events: &[Event],
    now: DateTime<Utc>,
    seen: &mut HashSet<String>,
) -> io::Result<usize> {
    let mut events = events.to_vec();
    events.sort_by_key(|left| left.timestamp);
    let mut rendered = 0;
    for event in events {
        let Some(timestamp) = event.timestamp else {
            continue;
        };
        if !seen.insert(tail_key(&event)) {
            continue;
        }
        if now.signed_duration_since(timestamp) > Duration::hours(24) {
            continue;
        }
        writeln!(writer, "{}", render_event(&event, timestamp))?;
        rendered += 1;
    }
    Ok(rendered)
}

fn render_event(event: &Event, timestamp: DateTime<Utc>) -> String {
    format!(
        "{} {:<7} {:<12} total={:<8} output={:<7} model={} cwd={}",
        timestamp.with_timezone(&Local).format("%H:%M:%S"),
        event.provider,
        short_session_id(event.session_id.as_deref().unwrap_or_default()),
        token_count(event.total_tokens),
        token_count(event.output_tokens),
        event.model.as_deref().unwrap_or("-"),
        event
            .cwd
            .as_deref()
            .map(compact_home)
            .unwrap_or_else(|| "-".to_string()),
    )
}

fn tail_key(event: &Event) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}",
        event.provider,
        event.session_id.as_deref().unwrap_or_default(),
        event.request_id.as_deref().unwrap_or_default(),
        event
            .timestamp
            .map(|timestamp| timestamp.to_rfc3339())
            .unwrap_or_default(),
        event.total_tokens,
        event.cumulative_tokens
    )
}

fn token_count(value: i64) -> String {
    if value >= 1_000_000 {
        let millions = value as f64 / 1_000_000.0;
        let rendered = format!("{millions:.1}");
        format!("{}M", rendered.trim_end_matches(".0"))
    } else if value >= 10_000 {
        format!("{}k", value / 1_000)
    } else {
        value.to_string()
    }
}

fn short_session_id(id: &str) -> String {
    if id.is_empty() {
        "-".to_string()
    } else if id.len() <= 18 {
        id.to_string()
    } else {
        format!("{}...{}", &id[..8], &id[id.len() - 6..])
    }
}

fn compact_home(path: &Path) -> String {
    let rendered = path.display().to_string();
    if let Some(home) = crate::config::default_home_dir() {
        let home = home.display().to_string();
        if let Some(rest) = rendered.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    rendered
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn render_new_events_orders_dedupes_and_skips_stale_rows() {
        let now = DateTime::parse_from_rfc3339("2026-05-28T19:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let recent = event("codex", "session_abcdefghijklmnopqrstuvwxyz", 107, now);
        let duplicate = recent.clone();
        let older = event("claude", "claude-session", 33, now - Duration::minutes(1));
        let stale = event("codex", "old", 999, now - Duration::hours(25));
        let mut seen = HashSet::new();
        let mut out = Vec::new();

        let count = render_new_events(&mut out, &[recent, duplicate, older, stale], now, &mut seen)
            .unwrap();

        let text = String::from_utf8(out).unwrap();
        assert_eq!(count, 2);
        assert!(text.contains("claude  claude-session total=33"));
        assert!(text.contains("codex   session_...uvwxyz total=107"));
        assert!(!text.contains("old"));
    }

    #[test]
    fn render_new_events_keeps_seen_state_across_scans() {
        let now = Utc::now();
        let mut state = TailState::default();
        let events = vec![event("codex", "session", 107, now)];
        let mut first = Vec::new();
        let mut second = Vec::new();

        assert_eq!(
            state.render_new_events(&mut first, &events, now).unwrap(),
            1
        );
        assert_eq!(
            state.render_new_events(&mut second, &events, now).unwrap(),
            0
        );
        assert!(!first.is_empty());
        assert!(second.is_empty());
    }

    #[test]
    fn scan_once_renders_appended_events_only() {
        let home = tempfile::tempdir().unwrap();
        let state_dir = tempfile::tempdir().unwrap();
        let codex_dir = home.path().join(".codex").join("archived_sessions");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let log = codex_dir.join("rollout.jsonl");
        std::fs::write(&log, codex_fixture("session", "/repo", 107, 107)).unwrap();
        let reader = Reader::with_state(home.path(), state_dir.path());
        let mut state = TailState::default();
        let now = Utc::now();
        let since = now - Duration::hours(1);
        let mut first = Vec::new();
        let mut second = Vec::new();

        let first_scan = scan_once(&reader, &mut state, &mut first, since, now).unwrap();
        std::fs::write(
            &log,
            format!(
                "{}{}",
                codex_fixture("session", "/repo", 107, 107),
                codex_token_row(208, 315)
            ),
        )
        .unwrap();
        let second_scan = scan_once(&reader, &mut state, &mut second, since, now).unwrap();

        assert_eq!(first_scan.rendered, 1);
        assert_eq!(second_scan.rendered, 1);
        assert!(String::from_utf8(first).unwrap().contains("total=107"));
        let second_text = String::from_utf8(second).unwrap();
        assert!(second_text.contains("total=208"));
        assert!(!second_text.contains("total=107"));
    }

    #[test]
    fn scan_once_reports_source_errors_and_renders_valid_events() {
        let home = tempfile::tempdir().unwrap();
        let codex_dir = home.path().join(".codex").join("archived_sessions");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(codex_dir.join("bad.jsonl"), "{not json}\n").unwrap();
        let claude_dir = home.path().join(".claude").join("projects").join("-repo");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("session.jsonl"),
            r#"{"timestamp":"2026-05-19T20:00:00Z","requestId":"req_1","sessionId":"session_claude","uuid":"turn_1","cwd":"/repo","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_creation_input_tokens":30,"cache_read_input_tokens":40,"output_tokens":5}}}
"#,
        )
        .unwrap();
        let reader = Reader::new(home.path());
        let mut state = TailState::default();
        let now = DateTime::parse_from_rfc3339("2026-05-19T20:01:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut out = Vec::new();

        let scan = scan_once(&reader, &mut state, &mut out, now - Duration::hours(1), now).unwrap();

        let text = String::from_utf8(out).unwrap();
        assert_eq!(scan.rendered, 1);
        assert!(scan.source_error.unwrap().contains("usage json"));
        assert!(text.contains("claude"));
        assert!(text.contains("session_claude"));
        assert!(text.contains("total=76"));
    }

    fn event(provider: &str, session: &str, total: i64, timestamp: DateTime<Utc>) -> Event {
        Event {
            kind: crate::usage::EventKind::TokenCheckpoint,
            provider: provider.to_string(),
            source: "test".to_string(),
            source_path: PathBuf::from("usage.jsonl"),
            session_id: Some(session.to_string()),
            turn_id: None,
            request_id: None,
            model: None,
            cwd: Some(PathBuf::from("/repo")),
            timestamp: Some(timestamp),
            input_tokens: total,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
            output_tokens: 5,
            reasoning_output_tokens: 0,
            total_tokens: total,
            spent_tokens: total,
            cumulative_tokens: total,
            model_context_window: 0,
        }
    }

    fn codex_fixture(session: &str, cwd: &str, total: i64, cumulative: i64) -> String {
        format!(
            r#"{{"timestamp":"{}","type":"session_meta","payload":{{"id":"{session}","cwd":"{cwd}"}}}}
{}"#,
            Utc::now().to_rfc3339(),
            codex_token_row(total, cumulative)
        )
    }

    fn codex_token_row(total: i64, cumulative: i64) -> String {
        format!(
            r#"{{"timestamp":"{}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":{total}}},"total_token_usage":{{"total_tokens":{cumulative}}},"model_context_window":258400}}}}}}
"#,
            Utc::now().to_rfc3339()
        )
    }
}
