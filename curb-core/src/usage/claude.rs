use std::fs::File;
use std::io::{BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{
    CachedRead, Event, EventKind, Layout, Provider, ProviderRoot, ReaderState, SourceReport,
    UsageError, dedupe, event_is_since, jsonl_files_recursive, modified_since, parse_time,
    read_cached, read_usage_line, sort_events, user_input_event, validate_full_usage_file,
};

pub(super) fn provider() -> Provider {
    Provider {
        id: "claude",
        roots,
        read_file: read_cached_file,
        tail_file: |path| parse_file(path, 0),
    }
}

fn roots(home: &Path) -> Vec<ProviderRoot> {
    vec![ProviderRoot {
        path: home.join(".claude").join("projects"),
        layout: Layout::Recursive,
        tailable: false,
    }]
}

pub(super) fn projects_since(
    root: &Path,
    since: Option<DateTime<Utc>>,
) -> Result<(Vec<Event>, SourceReport), UsageError> {
    let mut paths = jsonl_files_recursive(root)?;
    paths.retain(|path| modified_since(path, since).unwrap_or(true));
    let mut events = Vec::new();
    for path in &paths {
        validate_full_usage_file(path)?;
        events.extend(parse_file(path, 0)?);
    }
    let mut events = dedupe(events);
    events.retain(|event| event_is_since(event, since));
    sort_events(&mut events);
    let report = SourceReport {
        provider: "claude".to_string(),
        files: paths.len(),
        events: events.len(),
        error: None,
    };
    Ok((events, report))
}

fn read_cached_file(
    state: &mut ReaderState,
    state_dir: Option<&Path>,
    path: &Path,
) -> Result<Vec<Event>, UsageError> {
    read_cached(state, state_dir, path, |start, cached| {
        let mut combined = cached.map(|file| file.events.clone()).unwrap_or_default();
        combined.extend(parse_file(path, start)?);
        Ok(CachedRead {
            events: dedupe(combined),
            provider_state: None,
        })
    })
}

fn parse_file(path: &Path, offset: u64) -> Result<Vec<Event>, UsageError> {
    let mut file = File::open(path).map_err(|source| UsageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|source| UsageError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let mut reader = BufReader::new(file);
    let mut out = Vec::new();
    while let Some(line) = read_usage_line(&mut reader, path)? {
        let row: Row = serde_json::from_slice(&line).map_err(|source| UsageError::Json {
            path: path.to_path_buf(),
            source,
        })?;
        if row.row_type.as_deref() == Some("user") {
            if row.tool_use_result.is_none()
                && !row.is_sidechain
                && is_human_content(row.message.content.as_ref())
            {
                out.push(user_input_event(
                    "claude",
                    "claude.projects",
                    path,
                    row.session_id.clone(),
                    row.cwd.clone().map(PathBuf::from),
                    parse_time(row.timestamp.as_deref()),
                ));
            }
            continue;
        }
        let Some(usage) = row.message.usage else {
            continue;
        };
        let request_id = row.request_id.or_else(|| row.message.id.clone());
        out.push(Event {
            kind: EventKind::TokenCheckpoint,
            provider: "claude".to_string(),
            source: "claude.projects".to_string(),
            source_path: path.to_path_buf(),
            session_id: row.session_id,
            turn_id: row.uuid,
            request_id: request_id.clone(),
            model: row.message.model,
            cwd: row.cwd.map(PathBuf::from),
            timestamp: parse_time(row.timestamp.as_deref()),
            input_tokens: usage.input_tokens,
            cached_input_tokens: usage.cache_read_input_tokens,
            cache_creation_input_tokens: usage.cache_creation_input_tokens,
            output_tokens: usage.output_tokens,
            reasoning_output_tokens: 0,
            total_tokens: usage.input_tokens
                + usage.cache_read_input_tokens
                + usage.cache_creation_input_tokens
                + usage.output_tokens,
            spent_tokens: usage.input_tokens
                + usage.cache_creation_input_tokens
                + usage.output_tokens,
            cumulative_tokens: 0,
            model_context_window: 0,
        });
    }
    Ok(out)
}

/// A Claude `user` row is a real human turn only when its content is typed text
/// (a string, or content blocks of type `text`). Tool results and injected
/// system strings are mid-turn, not boundaries.
fn is_human_content(content: Option<&serde_json::Value>) -> bool {
    match content {
        Some(serde_json::Value::String(_)) => true,
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .any(|item| item.get("type").and_then(|kind| kind.as_str()) == Some("text")),
        _ => false,
    }
}

#[derive(Debug, Default, Deserialize)]
struct Row {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default, rename = "type")]
    row_type: Option<String>,
    #[serde(default, rename = "requestId")]
    request_id: Option<String>,
    #[serde(default, rename = "sessionId")]
    session_id: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default, rename = "isSidechain")]
    is_sidechain: bool,
    #[serde(default, rename = "toolUseResult")]
    tool_use_result: Option<serde_json::Value>,
    #[serde(default)]
    message: Message,
}

#[derive(Debug, Default, Deserialize)]
struct Message {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    content: Option<serde_json::Value>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Default, Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    cache_creation_input_tokens: i64,
    #[serde(default)]
    cache_read_input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
}
