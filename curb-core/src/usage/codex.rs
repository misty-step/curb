use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{
    CODEX_LIVE_COLD_READ_LIMIT, CachedRead, Event, EventKind, Layout, Provider, ProviderRoot,
    ReaderState, SourceReport, UsageError, dedupe, event_is_since, jsonl_files_one_level,
    modified_since, parse_time, read_cached, read_usage_line, reject_symlink, sort_events,
    user_input_event, validate_full_usage_file,
};

pub(super) fn provider() -> Provider {
    Provider {
        id: "codex",
        roots,
        read_file: read_cached_file,
        tail_file: read_live_tail,
    }
}

fn roots(home: &Path) -> Vec<ProviderRoot> {
    vec![
        ProviderRoot {
            path: home.join(".codex").join("archived_sessions"),
            layout: Layout::OneLevel,
            tailable: false,
        },
        ProviderRoot {
            path: home.join(".codex").join("sessions"),
            layout: Layout::Recursive,
            tailable: true,
        },
    ]
}

pub(super) fn archived_sessions_since(
    root: &Path,
    since: Option<DateTime<Utc>>,
) -> Result<(Vec<Event>, SourceReport), UsageError> {
    let mut paths = jsonl_files_one_level(root)?;
    paths.retain(|path| modified_since(path, since).unwrap_or(true));
    let mut events = Vec::new();
    for path in &paths {
        validate_full_usage_file(path)?;
        events.extend(parse_file(path, 0, None, None)?.events);
    }
    let mut events = dedupe(events);
    events.retain(|event| event_is_since(event, since));
    sort_events(&mut events);
    let report = SourceReport {
        provider: "codex".to_string(),
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
        let seed = cached.and_then(CachedSeed::from_file);
        let parsed = parse_file(
            path,
            start,
            seed.as_ref().and_then(|state| state.session_id.clone()),
            seed.and_then(|state| state.cwd),
        )?;
        let mut combined = cached.map(|file| file.events.clone()).unwrap_or_default();
        combined.extend(parsed.events);
        let combined = dedupe(combined);
        Ok(CachedRead {
            events: combined,
            provider_state: serde_json::to_value(CachedSeed {
                session_id: parsed.session_id,
                cwd: parsed.cwd,
            })
            .ok(),
        })
    })
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct CachedSeed {
    session_id: Option<String>,
    cwd: Option<PathBuf>,
}

impl CachedSeed {
    fn from_file(file: &super::CachedFile) -> Option<Self> {
        file.provider_state
            .as_ref()
            .and_then(|state| serde_json::from_value(state.clone()).ok())
    }
}

struct Parse {
    events: Vec<Event>,
    session_id: Option<String>,
    cwd: Option<PathBuf>,
}

fn read_live_tail(path: &Path) -> Result<Vec<Event>, UsageError> {
    reject_symlink(path)?;
    let metadata = fs::metadata(path).map_err(|source| UsageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() <= CODEX_LIVE_COLD_READ_LIMIT {
        return Ok(parse_file(path, 0, None, None)?.events);
    }
    let (session_id, cwd) = parse_metadata(path)?;
    let offset = aligned_line_offset(path, metadata.len() - CODEX_LIVE_COLD_READ_LIMIT)?;
    Ok(parse_file(path, offset, session_id, cwd)?.events)
}

fn parse_metadata(path: &Path) -> Result<(Option<String>, Option<PathBuf>), UsageError> {
    let file = File::open(path).map_err(|source| UsageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut buffer = Vec::new();
    file.take(64 * 1024)
        .read_to_end(&mut buffer)
        .map_err(|source| UsageError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let line = String::from_utf8_lossy(&buffer);
    if line.contains(r#""type":"session_meta""#) || line.contains(r#""type": "session_meta""#) {
        Ok((
            json_string_field(&line, "id"),
            json_string_field(&line, "cwd").map(PathBuf::from),
        ))
    } else {
        Ok((None, None))
    }
}

fn json_string_field(raw: &str, field: &str) -> Option<String> {
    let needle = format!(r#""{field}":"#);
    let start = raw.find(&needle)? + needle.len();
    let mut chars = raw[start..].chars();
    if chars.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for ch in chars {
        if escaped {
            out.push(match ch {
                '"' => '"',
                '\\' => '\\',
                '/' => '/',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            });
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

fn aligned_line_offset(path: &Path, offset: u64) -> Result<u64, UsageError> {
    if offset == 0 {
        return Ok(0);
    }
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
    let mut discarded = String::new();
    let read = reader
        .read_line(&mut discarded)
        .map_err(|source| UsageError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(offset + read as u64)
}

fn parse_file(
    path: &Path,
    offset: u64,
    mut session_id: Option<String>,
    mut cwd: Option<PathBuf>,
) -> Result<Parse, UsageError> {
    if session_id.is_none() {
        session_id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(ToString::to_string);
    }
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
        if row.row_type == "session_meta" {
            if let Some(id) = row.payload.id {
                session_id = Some(id);
            }
            if let Some(value) = row.payload.cwd {
                cwd = Some(PathBuf::from(value));
            }
            continue;
        }
        let is_user_input = (row.row_type == "event_msg"
            && row.payload.payload_type.as_deref() == Some("user_message"))
            || (row.row_type == "response_item"
                && row.payload.payload_type.as_deref() == Some("message")
                && row.payload.role.as_deref() == Some("user"));
        if is_user_input {
            out.push(user_input_event(
                "codex",
                "codex.sessions",
                path,
                session_id.clone(),
                cwd.clone(),
                parse_time(row.timestamp.as_deref()),
            ));
            continue;
        }
        if row.row_type != "event_msg" || row.payload.payload_type.as_deref() != Some("token_count")
        {
            continue;
        }
        let info = row.payload.info.unwrap_or_default();
        let last = info.last_token_usage;
        let total = if last.total_tokens != 0 {
            last.total_tokens
        } else {
            last.input_tokens + last.output_tokens + last.reasoning_output_tokens
        };
        let uncached_input = (last.input_tokens - last.cached_input_tokens).max(0);
        out.push(Event {
            kind: EventKind::TokenCheckpoint,
            provider: "codex".to_string(),
            source: "codex.archived_sessions".to_string(),
            source_path: path.to_path_buf(),
            session_id: session_id.clone(),
            turn_id: None,
            request_id: None,
            model: None,
            cwd: cwd.clone(),
            timestamp: parse_time(row.timestamp.as_deref()),
            input_tokens: last.input_tokens,
            cached_input_tokens: last.cached_input_tokens,
            cache_creation_input_tokens: 0,
            output_tokens: last.output_tokens,
            reasoning_output_tokens: last.reasoning_output_tokens,
            total_tokens: total,
            spent_tokens: uncached_input + last.output_tokens + last.reasoning_output_tokens,
            cumulative_tokens: info.total_token_usage.total_tokens,
            model_context_window: info.model_context_window,
        });
    }
    Ok(Parse {
        events: out,
        session_id,
        cwd,
    })
}

#[derive(Debug, Default, Deserialize)]
struct Row {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default, rename = "type")]
    row_type: String,
    #[serde(default)]
    payload: Payload,
}

#[derive(Debug, Default, Deserialize)]
struct Payload {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default, rename = "type")]
    payload_type: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    info: Option<Info>,
}

#[derive(Debug, Default, Deserialize)]
struct Info {
    #[serde(default)]
    last_token_usage: TokenUsage,
    #[serde(default)]
    total_token_usage: TokenUsage,
    #[serde(default)]
    model_context_window: i64,
}

#[derive(Debug, Default, Deserialize)]
struct TokenUsage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    cached_input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
    #[serde(default)]
    reasoning_output_tokens: i64,
    #[serde(default)]
    total_tokens: i64,
}
