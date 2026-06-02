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
        id: "pi",
        roots,
        read_file: read_cached_file,
        tail_file: |path| parse_file(path, 0),
    }
}

fn roots(home: &Path) -> Vec<ProviderRoot> {
    vec![ProviderRoot {
        path: home.join(".pi").join("agent").join("sessions"),
        layout: Layout::Recursive,
        tailable: false,
    }]
}

pub(super) fn sessions_since(
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
        provider: "pi".to_string(),
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
    let mut session_id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(ToString::to_string);
    let mut cwd = None;
    while let Some(line) = read_usage_line(&mut reader, path)? {
        let row: Row = serde_json::from_slice(&line).map_err(|source| UsageError::Json {
            path: path.to_path_buf(),
            source,
        })?;
        if row.row_type == "session" {
            if let Some(id) = row.id {
                session_id = Some(id);
            }
            if let Some(value) = row.cwd {
                cwd = Some(PathBuf::from(value));
            }
            continue;
        }
        if row.row_type != "message" {
            continue;
        }
        if row.message.role.as_deref() == Some("user") {
            out.push(user_input_event(
                "pi",
                "pi.sessions",
                path,
                session_id.clone(),
                cwd.clone(),
                parse_time(row.timestamp.as_deref()),
            ));
            continue;
        }
        if row.message.role.as_deref() != Some("assistant") {
            continue;
        }
        let Some(usage) = row.message.usage else {
            continue;
        };
        let total = if usage.total_tokens != 0 {
            usage.total_tokens
        } else {
            usage.input + usage.cache_read + usage.cache_write + usage.output
        };
        out.push(Event {
            kind: EventKind::TokenCheckpoint,
            provider: "pi".to_string(),
            source: "pi.sessions".to_string(),
            source_path: path.to_path_buf(),
            session_id: session_id.clone(),
            turn_id: row.id,
            request_id: None,
            model: row.message.model,
            cwd: cwd.clone(),
            timestamp: parse_time(row.timestamp.as_deref()),
            input_tokens: usage.input,
            cached_input_tokens: usage.cache_read,
            cache_creation_input_tokens: usage.cache_write,
            output_tokens: usage.output,
            reasoning_output_tokens: 0,
            total_tokens: total,
            spent_tokens: usage.input + usage.cache_write + usage.output,
            cumulative_tokens: 0,
            model_context_window: 0,
        });
    }
    Ok(out)
}

#[derive(Debug, Default, Deserialize)]
struct Row {
    #[serde(default, rename = "type")]
    row_type: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    message: Message,
}

#[derive(Debug, Default, Deserialize)]
struct Message {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Default, Deserialize)]
struct Usage {
    #[serde(default)]
    input: i64,
    #[serde(default)]
    output: i64,
    #[serde(default, rename = "cacheRead")]
    cache_read: i64,
    #[serde(default, rename = "cacheWrite")]
    cache_write: i64,
    #[serde(default, rename = "totalTokens")]
    total_tokens: i64,
}
