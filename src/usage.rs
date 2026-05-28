use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UsageError {
    #[error("usage io {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("usage json {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub provider: String,
    pub source: String,
    pub source_path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    pub timestamp: Option<DateTime<Utc>>,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
    pub cumulative_tokens: i64,
    pub model_context_window: i64,
}

impl Event {
    fn dedup_key(&self) -> Option<String> {
        match self.provider.as_str() {
            "codex" if self.cumulative_tokens != 0 || self.total_tokens != 0 => Some(format!(
                "codex:{}:{}:{}",
                self.session_id.as_deref().unwrap_or_default(),
                self.cumulative_tokens,
                self.total_tokens
            )),
            "claude" => self.request_id.as_ref().map(|request_id| {
                format!(
                    "claude:{}:{}",
                    self.session_id.as_deref().unwrap_or_default(),
                    request_id
                )
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub provider: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<DateTime<Utc>>,
    pub events: usize,
    pub models: Vec<String>,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceReport {
    pub provider: String,
    pub files: usize,
    pub events: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub generated_at: DateTime<Utc>,
    pub sources: Vec<SourceReport>,
    pub sessions: Vec<SessionSummary>,
}

impl Report {
    pub fn source_line(&self) -> String {
        self.sources
            .iter()
            .map(|source| match &source.error {
                Some(_) => format!("{} unavailable", source.provider),
                None => format!("{} {} events", source.provider, source.events),
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[derive(Clone, Debug)]
pub struct Reader {
    home: PathBuf,
}

impl Reader {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    pub fn report_since(&self, since: Option<DateTime<Utc>>) -> Result<Report, UsageError> {
        let (events, sources) = self.events_since(since)?;
        Ok(Report {
            generated_at: Utc::now(),
            sources,
            sessions: summarize(&events),
        })
    }

    pub fn events_since(
        &self,
        since: Option<DateTime<Utc>>,
    ) -> Result<(Vec<Event>, Vec<SourceReport>), UsageError> {
        let codex_root = self.home.join(".codex").join("archived_sessions");
        let claude_root = self.home.join(".claude").join("projects");
        let (mut codex, codex_report) = codex_archived_sessions_since(&codex_root, since)?;
        let (claude, claude_report) = claude_projects_since(&claude_root, since)?;
        codex.extend(claude);
        let mut events = dedupe(codex);
        events.retain(|event| event_is_since(event, since));
        sort_events(&mut events);
        Ok((events, vec![codex_report, claude_report]))
    }
}

pub fn codex_archived_sessions_since(
    root: impl AsRef<Path>,
    since: Option<DateTime<Utc>>,
) -> Result<(Vec<Event>, SourceReport), UsageError> {
    let root = root.as_ref();
    let mut paths = jsonl_files_one_level(root)?;
    paths.retain(|path| modified_since(path, since).unwrap_or(true));
    let mut events = Vec::new();
    for path in &paths {
        events.extend(parse_codex_file(path, 0, None, None)?);
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

pub fn claude_projects_since(
    root: impl AsRef<Path>,
    since: Option<DateTime<Utc>>,
) -> Result<(Vec<Event>, SourceReport), UsageError> {
    let root = root.as_ref();
    let mut paths = jsonl_files_recursive(root)?;
    paths.retain(|path| modified_since(path, since).unwrap_or(true));
    let mut events = Vec::new();
    for path in &paths {
        events.extend(parse_claude_file(path, 0)?);
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

pub fn summarize(events: &[Event]) -> Vec<SessionSummary> {
    let mut by_key: HashMap<String, SessionSummary> = HashMap::new();
    let mut models: HashMap<String, BTreeSet<String>> = HashMap::new();
    for event in dedupe(events.to_vec()) {
        let session_id = event.session_id.clone().unwrap_or_default();
        let key = if session_id.is_empty() {
            format!("{}:{}", event.provider, event.source_path.display())
        } else {
            format!("{}:{session_id}", event.provider)
        };
        let summary = by_key.entry(key.clone()).or_insert_with(|| SessionSummary {
            provider: event.provider.clone(),
            session_id: session_id.clone(),
            cwd: event.cwd.clone(),
            last: None,
            events: 0,
            models: Vec::new(),
            input_tokens: 0,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            total_tokens: 0,
        });
        if event.timestamp > summary.last {
            summary.last = event.timestamp;
        }
        if summary.cwd.is_none() {
            summary.cwd = event.cwd.clone();
        }
        summary.events += 1;
        summary.input_tokens += event.input_tokens;
        summary.cached_input_tokens += event.cached_input_tokens;
        summary.cache_creation_input_tokens += event.cache_creation_input_tokens;
        summary.output_tokens += event.output_tokens;
        summary.reasoning_output_tokens += event.reasoning_output_tokens;
        summary.total_tokens += event.total_tokens;
        if let Some(model) = event.model {
            models.entry(key).or_default().insert(model);
        }
    }
    for (key, summary) in &mut by_key {
        summary.models = models.remove(key).unwrap_or_default().into_iter().collect();
    }
    let mut out = by_key.into_values().collect::<Vec<_>>();
    out.sort_by(|left, right| right.last.cmp(&left.last));
    out
}

fn parse_codex_file(
    path: &Path,
    offset: u64,
    mut session_id: Option<String>,
    mut cwd: Option<PathBuf>,
) -> Result<Vec<Event>, UsageError> {
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
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|source| UsageError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let row: CodexRow = serde_json::from_str(&line).map_err(|source| UsageError::Json {
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
        if row.row_type != "event_msg" || row.payload.payload_type.as_deref() != Some("token_count")
        {
            continue;
        }
        let info = row.payload.info.unwrap_or_default();
        let last = info.last_token_usage;
        let total = if last.total_tokens != 0 {
            last.total_tokens
        } else {
            last.input_tokens
                + last.cached_input_tokens
                + last.output_tokens
                + last.reasoning_output_tokens
        };
        out.push(Event {
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
            cumulative_tokens: info.total_token_usage.total_tokens,
            model_context_window: info.model_context_window,
        });
    }
    Ok(out)
}

fn parse_claude_file(path: &Path, offset: u64) -> Result<Vec<Event>, UsageError> {
    let mut file = File::open(path).map_err(|source| UsageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|source| UsageError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|source| UsageError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let row: ClaudeRow = serde_json::from_str(&line).map_err(|source| UsageError::Json {
            path: path.to_path_buf(),
            source,
        })?;
        let Some(usage) = row.message.usage else {
            continue;
        };
        let request_id = row.request_id.or_else(|| row.message.id.clone());
        out.push(Event {
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
            cumulative_tokens: 0,
            model_context_window: 0,
        });
    }
    Ok(out)
}

fn dedupe(events: Vec<Event>) -> Vec<Event> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for event in events {
        if let Some(key) = event.dedup_key()
            && !seen.insert(key)
        {
            continue;
        }
        out.push(event);
    }
    out
}

fn sort_events(events: &mut [Event]) {
    events.sort_by(|left, right| left.timestamp.cmp(&right.timestamp));
}

fn event_is_since(event: &Event, since: Option<DateTime<Utc>>) -> bool {
    match (event.timestamp, since) {
        (_, None) | (None, Some(_)) => true,
        (Some(at), Some(since)) => at >= since,
    }
}

fn modified_since(path: &Path, since: Option<DateTime<Utc>>) -> Result<bool, UsageError> {
    let Some(since) = since else {
        return Ok(true);
    };
    let metadata = fs::metadata(path).map_err(|source| UsageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let modified = metadata.modified().map_err(|source| UsageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(DateTime::<Utc>::from(modified) >= since)
}

fn jsonl_files_one_level(root: &Path) -> Result<Vec<PathBuf>, UsageError> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(out);
    };
    for entry in entries {
        let entry = entry.map_err(|source| UsageError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn jsonl_files_recursive(root: &Path) -> Result<Vec<PathBuf>, UsageError> {
    let mut out = Vec::new();
    collect_jsonl(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_jsonl(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), UsageError> {
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry.map_err(|source| UsageError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
    Ok(())
}

fn parse_time(raw: Option<&str>) -> Option<DateTime<Utc>> {
    let raw = raw?;
    DateTime::parse_from_rfc3339(raw)
        .map(|time| time.with_timezone(&Utc))
        .ok()
}

#[derive(Debug, Default, Deserialize)]
struct CodexRow {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default, rename = "type")]
    row_type: String,
    #[serde(default)]
    payload: CodexPayload,
}

#[derive(Debug, Default, Deserialize)]
struct CodexPayload {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default, rename = "type")]
    payload_type: Option<String>,
    #[serde(default)]
    info: Option<CodexInfo>,
}

#[derive(Debug, Default, Deserialize)]
struct CodexInfo {
    #[serde(default)]
    last_token_usage: CodexTokenUsage,
    #[serde(default)]
    total_token_usage: CodexTokenUsage,
    #[serde(default)]
    model_context_window: i64,
}

#[derive(Debug, Default, Deserialize)]
struct CodexTokenUsage {
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

#[derive(Debug, Default, Deserialize)]
struct ClaudeRow {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default, rename = "requestId")]
    request_id: Option<String>,
    #[serde(default, rename = "sessionId")]
    session_id: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    message: ClaudeMessage,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeMessage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<ClaudeUsage>,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeUsage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    cache_creation_input_tokens: i64,
    #[serde(default)]
    cache_read_input_tokens: i64,
    #[serde(default)]
    output_tokens: i64,
}

#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    use chrono::TimeZone;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn codex_archived_sessions_extracts_token_counts_without_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        fs::write(
            &path,
            r#"{"timestamp":"2026-05-19T16:00:00Z","type":"session_meta","payload":{"id":"session_codex","cwd":"/repo"}}
{"timestamp":"2026-05-19T16:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":107},"total_token_usage":{"total_tokens":107},"model_context_window":258400}}}
{"timestamp":"2026-05-19T16:02:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":107},"total_token_usage":{"total_tokens":107},"model_context_window":258400}}}
"#,
        )
        .unwrap();

        let (events, report) = codex_archived_sessions_since(dir.path(), None).unwrap();

        assert_eq!(report.files, 1);
        assert_eq!(report.events, 1);
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.provider, "codex");
        assert_eq!(event.session_id.as_deref(), Some("session_codex"));
        assert_eq!(event.input_tokens, 100);
        assert_eq!(event.cached_input_tokens, 20);
        assert_eq!(event.output_tokens, 5);
        assert_eq!(event.reasoning_output_tokens, 2);
        assert_eq!(event.total_tokens, 107);
        assert_eq!(event.cwd.as_deref(), Some(Path::new("/repo")));
        assert_eq!(event.model_context_window, 258400);
    }

    #[test]
    fn claude_projects_dedupes_requests_and_summarizes_model_usage() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("projects").join("-repo");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("session.jsonl");
        fs::write(
            &path,
            r#"{"timestamp":"2026-05-19T20:00:00Z","requestId":"req_1","sessionId":"session_claude","uuid":"turn_1","cwd":"/repo","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_creation_input_tokens":30,"cache_read_input_tokens":40,"output_tokens":5}}}
{"timestamp":"2026-05-19T20:00:01Z","requestId":"req_1","sessionId":"session_claude","uuid":"turn_1_dup","cwd":"/repo","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_creation_input_tokens":30,"cache_read_input_tokens":40,"output_tokens":5}}}
{"timestamp":"2026-05-19T20:01:00Z","requestId":"req_2","sessionId":"session_claude","uuid":"turn_2","cwd":"/repo","message":{"id":"msg_2","model":"claude-sonnet-4-5","usage":{"input_tokens":2,"cache_creation_input_tokens":3,"cache_read_input_tokens":4,"output_tokens":6}}}
"#,
        )
        .unwrap();

        let (events, report) = claude_projects_since(dir.path().join("projects"), None).unwrap();

        assert_eq!(report.files, 1);
        assert_eq!(report.events, 2);
        assert_eq!(events.len(), 2);
        let summaries = summarize(&events);
        assert_eq!(summaries.len(), 1);
        let summary = &summaries[0];
        assert_eq!(summary.provider, "claude");
        assert_eq!(summary.session_id, "session_claude");
        assert_eq!(summary.input_tokens, 3);
        assert_eq!(summary.cache_creation_input_tokens, 33);
        assert_eq!(summary.cached_input_tokens, 44);
        assert_eq!(summary.output_tokens, 11);
        assert_eq!(summary.total_tokens, 91);
        assert_eq!(
            summary.models,
            vec![
                "claude-opus-4-7".to_string(),
                "claude-sonnet-4-5".to_string()
            ]
        );
    }

    #[test]
    fn reader_scans_codex_and_claude_roots_under_home() {
        let home = tempdir().unwrap();
        let codex = home.path().join(".codex").join("archived_sessions");
        fs::create_dir_all(&codex).unwrap();
        fs::write(
            codex.join("rollout.jsonl"),
            codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107),
        )
        .unwrap();
        let claude = home.path().join(".claude").join("projects").join("-repo");
        fs::create_dir_all(&claude).unwrap();
        fs::write(
            claude.join("session.jsonl"),
            r#"{"timestamp":"2026-05-19T20:00:00Z","requestId":"req_1","sessionId":"session_claude","uuid":"turn_1","cwd":"/repo","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_creation_input_tokens":30,"cache_read_input_tokens":40,"output_tokens":5}}}
"#,
        )
        .unwrap();

        let report = Reader::new(home.path()).report_since(None).unwrap();

        assert_eq!(report.sources.len(), 2);
        assert_eq!(report.sessions.len(), 2);
        assert_eq!(report.sources[0].provider, "codex");
        assert_eq!(report.sources[0].events, 1);
        assert_eq!(report.sources[1].provider, "claude");
        assert_eq!(report.sources[1].events, 1);
    }

    #[test]
    fn since_filters_event_timestamps() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        fs::write(
            &path,
            codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107)
                + &codex_token_row("2026-05-19T17:00:00Z", 211, 318),
        )
        .unwrap();

        let since = Utc.with_ymd_and_hms(2026, 5, 19, 16, 30, 0).unwrap();
        let (events, report) = codex_archived_sessions_since(dir.path(), Some(since)).unwrap();

        assert_eq!(report.events, 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].total_tokens, 211);
    }

    #[test]
    fn invalid_json_returns_source_path_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        fs::write(&path, r#"{"bad""#).unwrap();

        let err = codex_archived_sessions_since(dir.path(), None).unwrap_err();

        assert!(err.to_string().contains("rollout.jsonl"));
    }

    #[test]
    fn appended_older_timestamp_is_included_when_since_allows_it() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        fs::write(
            &path,
            codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107),
        )
        .unwrap();
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(codex_token_row("2026-05-19T15:30:00Z", 211, 318).as_bytes())
            .unwrap();

        let since = Utc.with_ymd_and_hms(2026, 5, 19, 15, 0, 0).unwrap();
        let (events, _) = codex_archived_sessions_since(dir.path(), Some(since)).unwrap();

        assert!(events.iter().any(|event| event.total_tokens == 211));
    }

    fn codex_fixture(session_id: &str, cwd: &str, at: &str, total: i64, cumulative: i64) -> String {
        format!(
            r#"{{"timestamp":"{at}","type":"session_meta","payload":{{"id":"{session_id}","cwd":"{cwd}"}}}}
{}"#,
            codex_token_row(at, total, cumulative)
        )
    }

    fn codex_token_row(at: &str, total: i64, cumulative: i64) -> String {
        format!(
            r#"{{"timestamp":"{at}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":{total}}},"total_token_usage":{{"total_tokens":{cumulative}}},"model_context_window":258400}}}}}}
"#
        )
    }
}
