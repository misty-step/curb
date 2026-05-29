use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const CODEX_LIVE_COLD_READ_LIMIT: u64 = 256 * 1024;

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
    #[error("usage state {path}: {source}")]
    State {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("usage scan: {0}")]
    Scan(String),
}

/// What a normalized usage row represents. Token checkpoints carry spend; a
/// user-input boundary marks where one human turn ends and the next begins.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    #[default]
    TokenCheckpoint,
    UserInput,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Event {
    #[serde(default)]
    pub kind: EventKind,
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
    pub spent_tokens: i64,
    pub cumulative_tokens: i64,
    pub model_context_window: i64,
}

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Scan {
    pub events: Vec<Event>,
    pub sources: Vec<SourceReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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

pub struct Reader {
    home: PathBuf,
    state_dir: Option<PathBuf>,
    state: Mutex<ReaderState>,
}

impl Reader {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            state_dir: None,
            state: Mutex::new(ReaderState::default()),
        }
    }

    pub fn with_state(home: impl Into<PathBuf>, state_dir: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            state_dir: Some(state_dir.into()),
            state: Mutex::new(ReaderState::default()),
        }
    }

    pub fn set_state_dir(&mut self, state_dir: impl Into<PathBuf>) {
        self.state_dir = Some(state_dir.into());
        self.state = Mutex::new(ReaderState::default());
    }

    pub fn report_since(&self, since: Option<DateTime<Utc>>) -> Result<Report, UsageError> {
        let scan = self.scan_since(since)?;
        if let Some(error) = scan.error {
            return Err(UsageError::Scan(error));
        }
        Ok(Report {
            generated_at: Utc::now(),
            sources: scan.sources,
            sessions: summarize(&scan.events),
        })
    }

    pub fn scan_since(&self, since: Option<DateTime<Utc>>) -> Result<Scan, UsageError> {
        let mut state = self.state.lock().expect("usage reader mutex poisoned");
        state.load(self.state_dir.as_deref())?;
        let mut events = Vec::new();
        let mut sources = Vec::new();
        let mut errors = Vec::new();
        // One pass per registered provider. A provider that errors becomes a
        // source-health error and never blocks the others. Adding GrokBuild,
        // Antigravity CLI, Pi, or OpenCode means adding to `providers()` — this
        // loop does not change.
        for provider in providers() {
            match scan_provider(
                &mut state,
                self.state_dir.as_deref(),
                &self.home,
                &provider,
                since,
            ) {
                Ok((provider_events, report)) => {
                    events.extend(provider_events);
                    sources.push(report);
                }
                Err(error) => {
                    let error = error.to_string();
                    errors.push(error.clone());
                    sources.push(SourceReport {
                        provider: provider.id.to_string(),
                        files: 0,
                        events: 0,
                        error: Some(error),
                    });
                }
            }
        }
        let mut events = dedupe(events);
        events.retain(|event| event_is_since(event, since));
        sort_events(&mut events);
        Ok(Scan {
            events,
            sources,
            error: if errors.is_empty() {
                None
            } else {
                Some(errors.join("; "))
            },
        })
    }

    pub fn events_since(
        &self,
        since: Option<DateTime<Utc>>,
    ) -> Result<(Vec<Event>, Vec<SourceReport>), UsageError> {
        let scan = self.scan_since(since)?;
        if let Some(error) = scan.error {
            Err(UsageError::Scan(error))
        } else {
            Ok((scan.events, scan.sources))
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ReaderState {
    #[serde(skip)]
    loaded: bool,
    #[serde(default)]
    files: HashMap<PathBuf, CachedFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CachedFile {
    size: u64,
    modified: DateTime<Utc>,
    prefix_hash: String,
    events: Vec<Event>,
    #[serde(skip_serializing_if = "Option::is_none")]
    codex_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    codex_cwd: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedReaderState {
    version: u8,
    files: HashMap<PathBuf, CachedFile>,
}

const PERSISTED_READER_STATE_VERSION: u8 = 2;

impl ReaderState {
    fn load(&mut self, state_dir: Option<&Path>) -> Result<(), UsageError> {
        if self.loaded {
            return Ok(());
        }
        self.loaded = true;
        let Some(state_dir) = state_dir else {
            return Ok(());
        };
        let path = state_dir.join("usage-cache.json");
        let content = match fs::read(&path) {
            Ok(content) => content,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(source) => return Err(UsageError::Io { path, source }),
        };
        let persisted: PersistedReaderState =
            serde_json::from_slice(&content).map_err(|source| UsageError::State {
                path: path.clone(),
                source,
            })?;
        if persisted.version == PERSISTED_READER_STATE_VERSION {
            self.files = persisted.files;
        }
        Ok(())
    }

    fn save(&self, state_dir: Option<&Path>) -> Result<(), UsageError> {
        let Some(state_dir) = state_dir else {
            return Ok(());
        };
        fs::create_dir_all(state_dir).map_err(|source| UsageError::Io {
            path: state_dir.to_path_buf(),
            source,
        })?;
        let path = state_dir.join("usage-cache.json");
        let persisted = PersistedReaderState {
            version: PERSISTED_READER_STATE_VERSION,
            files: self.files.clone(),
        };
        let content =
            serde_json::to_vec_pretty(&persisted).map_err(|source| UsageError::State {
                path: path.clone(),
                source,
            })?;
        let tmp = state_dir.join(format!(".usage-cache-{}.tmp", std::process::id()));
        {
            let mut file = File::create(&tmp).map_err(|source| UsageError::Io {
                path: tmp.clone(),
                source,
            })?;
            file.write_all(&content).map_err(|source| UsageError::Io {
                path: tmp.clone(),
                source,
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(fs::Permissions::from_mode(0o600))
                    .map_err(|source| UsageError::Io {
                        path: tmp.clone(),
                        source,
                    })?;
            }
        }
        fs::rename(&tmp, &path).map_err(|source| UsageError::Io { path, source })?;
        Ok(())
    }
}

impl UsageError {
    fn is_not_found(&self) -> bool {
        matches!(
            self,
            UsageError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound
        )
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
        events.extend(parse_codex_file(path, 0, None, None)?.events);
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

/// How a provider's log files are laid out under a root directory.
#[derive(Clone, Copy)]
enum Layout {
    /// `*.jsonl` directly in the root (e.g. Codex archived sessions).
    OneLevel,
    /// `*.jsonl` anywhere beneath the root (e.g. Codex live sessions, Claude
    /// projects).
    Recursive,
}

/// One log directory a provider writes to.
struct ProviderRoot {
    path: PathBuf,
    layout: Layout,
    /// When true, a live scan (`since` set) reads only the tail of large files
    /// instead of the whole thing. Codex live sessions grow without bound.
    tailable: bool,
}

/// Reads one file into events, creating/refreshing its incremental cache entry.
type FileReader = fn(&mut ReaderState, Option<&Path>, &Path) -> Result<Vec<Event>, UsageError>;
/// Reads only the tail of a large, still-growing live file.
type FileTailer = fn(&Path) -> Result<Vec<Event>, UsageError>;

/// A usage provider Curb ingests. Curb ships **Codex** and **Claude Code**.
///
/// Adding another agent — GrokBuild CLI, Antigravity CLI, Pi, OpenCode — means
/// appending one `Provider` to `providers()` with its log roots and a file
/// parser that emits [`Event`]s (token checkpoints + user-input boundaries).
/// Nothing in the scan loop, cache, dedupe, or read model changes.
struct Provider {
    /// Stable id used as the source name and as the session-key prefix.
    id: &'static str,
    /// Where this provider writes logs, relative to the user's home.
    roots: fn(&Path) -> Vec<ProviderRoot>,
    /// Read one file into events, refreshing the incremental cache.
    read_file: FileReader,
    /// Read only the tail of a large live file. Used when a root is `tailable`.
    tail_file: FileTailer,
}

/// The registered providers, in display order. This is the one place to extend
/// when adding a new agent.
fn providers() -> Vec<Provider> {
    vec![
        Provider {
            id: "codex",
            roots: |home| {
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
            },
            read_file: read_codex_cached,
            tail_file: read_codex_live_tail,
        },
        Provider {
            id: "claude",
            roots: |home| {
                vec![ProviderRoot {
                    path: home.join(".claude").join("projects"),
                    layout: Layout::Recursive,
                    tailable: false,
                }]
            },
            read_file: read_claude_cached,
            tail_file: |path| parse_claude_file(path, 0),
        },
    ]
}

/// Discover, prune, and read every file across a provider's roots into one
/// deduped, time-sorted event list plus a source-health report.
fn scan_provider(
    state: &mut ReaderState,
    state_dir: Option<&Path>,
    home: &Path,
    provider: &Provider,
    since: Option<DateTime<Utc>>,
) -> Result<(Vec<Event>, SourceReport), UsageError> {
    let mut events = Vec::new();
    let mut file_count = 0;
    for root in (provider.roots)(home) {
        let mut paths = match root.layout {
            Layout::OneLevel => jsonl_files_one_level(&root.path)?,
            Layout::Recursive => jsonl_files_recursive(&root.path)?,
        };
        paths.retain(|path| modified_since(path, since).unwrap_or(true));
        prune_missing(state, state_dir, &root.path, &paths)?;
        file_count += paths.len();
        for path in &paths {
            let read = if root.tailable && since.is_some() {
                (provider.tail_file)(path)
            } else {
                (provider.read_file)(state, state_dir, path)
            };
            match read {
                Ok(file_events) => events.extend(file_events),
                Err(error) if error.is_not_found() => continue,
                Err(error) => return Err(error),
            }
        }
    }
    let mut events = dedupe(events);
    events.retain(|event| event_is_since(event, since));
    sort_events(&mut events);
    let report = SourceReport {
        provider: provider.id.to_string(),
        files: file_count,
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

fn read_codex_cached(
    state: &mut ReaderState,
    state_dir: Option<&Path>,
    path: &Path,
) -> Result<Vec<Event>, UsageError> {
    read_cached(state, state_dir, path, |start, cached| {
        let seed_session = cached.and_then(|file| file.codex_session_id.clone());
        let seed_cwd = cached.and_then(|file| file.codex_cwd.clone());
        let parsed = parse_codex_file(path, start, seed_session, seed_cwd)?;
        let mut combined = cached.map(|file| file.events.clone()).unwrap_or_default();
        combined.extend(parsed.events);
        let combined = dedupe(combined);
        Ok(CachedRead {
            events: combined,
            codex_session_id: parsed.session_id,
            codex_cwd: parsed.cwd,
        })
    })
}

fn read_claude_cached(
    state: &mut ReaderState,
    state_dir: Option<&Path>,
    path: &Path,
) -> Result<Vec<Event>, UsageError> {
    read_cached(state, state_dir, path, |start, cached| {
        let mut combined = cached.map(|file| file.events.clone()).unwrap_or_default();
        combined.extend(parse_claude_file(path, start)?);
        Ok(CachedRead {
            events: dedupe(combined),
            codex_session_id: None,
            codex_cwd: None,
        })
    })
}

struct CachedRead {
    events: Vec<Event>,
    codex_session_id: Option<String>,
    codex_cwd: Option<PathBuf>,
}

struct CodexParse {
    events: Vec<Event>,
    session_id: Option<String>,
    cwd: Option<PathBuf>,
}

fn read_codex_live_tail(path: &Path) -> Result<Vec<Event>, UsageError> {
    let metadata = fs::metadata(path).map_err(|source| UsageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() <= CODEX_LIVE_COLD_READ_LIMIT {
        return Ok(parse_codex_file(path, 0, None, None)?.events);
    }
    let (session_id, cwd) = parse_codex_metadata(path)?;
    let offset = aligned_line_offset(path, metadata.len() - CODEX_LIVE_COLD_READ_LIMIT)?;
    Ok(parse_codex_file(path, offset, session_id, cwd)?.events)
}

fn parse_codex_metadata(path: &Path) -> Result<(Option<String>, Option<PathBuf>), UsageError> {
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

fn read_cached(
    state: &mut ReaderState,
    state_dir: Option<&Path>,
    path: &Path,
    read: impl FnOnce(u64, Option<&CachedFile>) -> Result<CachedRead, UsageError>,
) -> Result<Vec<Event>, UsageError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(source) => {
            state.files.remove(path);
            let _ = state.save(state_dir);
            return Err(UsageError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let modified = system_time_to_utc(metadata.modified().map_err(|source| UsageError::Io {
        path: path.to_path_buf(),
        source,
    })?);
    let size = metadata.len();
    if let Some(cached) = state.files.get(path)
        && cached.size == size
        && cached.modified == modified
    {
        return Ok(cached.events.clone());
    }

    let cached = state.files.get(path).cloned();
    let mut start = 0;
    if let Some(cached_file) = &cached
        && cached_file.size > 0
        && size > cached_file.size
        && file_prefix_hash(path, cached_file.size)? == cached_file.prefix_hash
    {
        start = cached_file.size;
    }

    let next = match read(start, cached.as_ref()) {
        Ok(next) => next,
        Err(error) => {
            state.files.remove(path);
            let _ = state.save(state_dir);
            return Err(error);
        }
    };
    let prefix_hash = match file_prefix_hash(path, size) {
        Ok(hash) => hash,
        Err(error) => {
            state.files.remove(path);
            let _ = state.save(state_dir);
            return Err(error);
        }
    };
    let cached_file = CachedFile {
        size,
        modified,
        prefix_hash,
        events: next.events.clone(),
        codex_session_id: next.codex_session_id,
        codex_cwd: next.codex_cwd,
    };
    state.files.insert(path.to_path_buf(), cached_file);
    state.save(state_dir)?;
    Ok(next.events)
}

fn prune_missing(
    state: &mut ReaderState,
    state_dir: Option<&Path>,
    root: &Path,
    paths: &[PathBuf],
) -> Result<(), UsageError> {
    let current = paths.iter().collect::<HashSet<_>>();
    let before = state.files.len();
    state
        .files
        .retain(|path, _| !path_within(path, root) || current.contains(path));
    if state.files.len() != before {
        state.save(state_dir)?;
    }
    Ok(())
}

fn path_within(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root).is_ok()
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
) -> Result<CodexParse, UsageError> {
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
        // A user-input boundary ends the current turn. Codex records the human's
        // input two ways and does not always emit both: the `user_message` UI
        // event, and the canonical `message` conversation item with role "user"
        // (the more complete record). Treat either as a boundary so turn spend
        // resets on every prompt, not just the ones that emit a UI event.
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
        // Codex reports the full prompt as `input_tokens`, with `cached_input_tokens`
        // a subset of it. So `total` already counts cached input once (inside
        // `input_tokens`); the fallback must not add it again.
        let total = if last.total_tokens != 0 {
            last.total_tokens
        } else {
            last.input_tokens + last.output_tokens + last.reasoning_output_tokens
        };
        // Spend is fresh work only: drop the cached prefix the model re-reads each
        // call. Without this, a turn's many tool calls each re-count the whole
        // cached context and turn spend balloons. (Mirrors Claude excluding
        // cache_read.)
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
    Ok(CodexParse {
        events: out,
        session_id,
        cwd,
    })
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
        if row.row_type.as_deref() == Some("user") {
            if row.tool_use_result.is_none()
                && !row.is_sidechain
                && claude_is_human_content(row.message.content.as_ref())
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

fn user_input_event(
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

/// A Claude `user` row is a real human turn only when its content is typed text
/// (a string, or content blocks of type `text`). Tool results and injected
/// system strings are mid-turn, not boundaries.
fn claude_is_human_content(content: Option<&serde_json::Value>) -> bool {
    match content {
        Some(serde_json::Value::String(_)) => true,
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .any(|item| item.get("type").and_then(|kind| kind.as_str()) == Some("text")),
        _ => false,
    }
}

fn dedupe(events: Vec<Event>) -> Vec<Event> {
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

fn system_time_to_utc(time: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(time)
}

fn file_prefix_hash(path: &Path, bytes: u64) -> Result<String, UsageError> {
    let mut file = File::open(path).map_err(|source| UsageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut remaining = bytes;
    let mut buffer = [0u8; 8192];
    let mut hasher = Sha256::new();
    while remaining > 0 {
        let read_len = remaining.min(buffer.len() as u64) as usize;
        let read = file
            .read(&mut buffer[..read_len])
            .map_err(|source| UsageError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        remaining -= read as u64;
    }
    Ok(hex::encode(hasher.finalize()))
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
    role: Option<String>,
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
    message: ClaudeMessage,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeMessage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    content: Option<serde_json::Value>,
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
        assert_eq!(event.spent_tokens, 87); // uncached input (100-20) + output 5 + reasoning 2
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
            events.iter().map(|event| event.spent_tokens).sum::<i64>(),
            47
        );
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
    fn reader_scans_live_codex_sessions_under_home() {
        let home = tempdir().unwrap();
        let live = home
            .path()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("05")
            .join("28");
        fs::create_dir_all(&live).unwrap();
        fs::write(
            live.join("rollout.jsonl"),
            codex_fixture(
                "session_live_codex",
                "/repo",
                "2026-05-28T16:00:00Z",
                211,
                211,
            ),
        )
        .unwrap();

        let report = Reader::new(home.path()).report_since(None).unwrap();

        assert_eq!(report.sources[0].provider, "codex");
        assert_eq!(report.sources[0].files, 1);
        assert_eq!(report.sources[0].events, 1);
        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.sessions[0].provider, "codex");
        assert_eq!(report.sessions[0].session_id, "session_live_codex");
        assert_eq!(report.sessions[0].total_tokens, 211);
    }

    #[test]
    fn lookback_scan_tails_large_live_codex_sessions() {
        let home = tempdir().unwrap();
        let live = home
            .path()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("05")
            .join("28");
        fs::create_dir_all(&live).unwrap();
        let path = live.join("large.jsonl");
        let padding = strings_of_length("a", CODEX_LIVE_COLD_READ_LIMIT as usize);
        fs::write(
            &path,
            format!(
                r#"{{"timestamp":"2026-05-28T16:00:00Z","type":"session_meta","payload":{{"id":"session_live_tail","cwd":"/repo"}}}}
{{"timestamp":"2026-05-28T16:00:01Z","type":"event_msg","payload":{{"type":"ignored"}},"padding":"{padding}"}}
{}"#,
                codex_token_row("2026-05-28T16:00:02Z", 377, 377)
            ),
        )
        .unwrap();

        let since = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let (events, _) = Reader::new(home.path()).events_since(Some(since)).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id.as_deref(), Some("session_live_tail"));
        assert_eq!(events[0].cwd.as_deref(), Some(Path::new("/repo")));
        assert_eq!(events[0].total_tokens, 377);
    }

    #[test]
    fn live_tail_metadata_reader_keeps_session_identity_for_large_logs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("large.jsonl");
        let padding = strings_of_length("x", CODEX_LIVE_COLD_READ_LIMIT as usize);
        fs::write(
            &path,
            format!(
                r#"{{"timestamp":"2026-05-28T16:00:00Z","type":"session_meta","payload":{{"id":"expected_session","cwd":"/expected/repo"}}}}
{{"timestamp":"2026-05-28T16:00:01Z","type":"event_msg","payload":{{"type":"ignored"}},"padding":"{padding}"}}
{}"#,
                codex_token_row("2026-05-28T16:00:02Z", 144, 144)
            ),
        )
        .unwrap();

        let events = read_codex_live_tail(&path).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id.as_deref(), Some("expected_session"));
        assert_eq!(events[0].cwd.as_deref(), Some(Path::new("/expected/repo")));
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

    #[test]
    fn reader_caches_returned_events_without_caller_mutation() {
        let home = tempdir().unwrap();
        let codex = home.path().join(".codex").join("archived_sessions");
        fs::create_dir_all(&codex).unwrap();
        fs::write(
            codex.join("rollout.jsonl"),
            codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107),
        )
        .unwrap();
        let reader = Reader::new(home.path());

        let (mut events, _) = reader.events_since(None).unwrap();
        events[0].session_id = Some("mutated".to_string());
        let (events, _) = reader.events_since(None).unwrap();

        assert_eq!(events[0].session_id.as_deref(), Some("session_codex"));
    }

    #[test]
    fn reader_prunes_deleted_provider_files() {
        let home = tempdir().unwrap();
        let codex = home.path().join(".codex").join("archived_sessions");
        fs::create_dir_all(&codex).unwrap();
        let path = codex.join("rollout.jsonl");
        fs::write(
            &path,
            codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107),
        )
        .unwrap();
        let reader = Reader::new(home.path());
        assert_eq!(reader.events_since(None).unwrap().0.len(), 1);

        fs::remove_file(&path).unwrap();
        let (events, reports) = reader.events_since(None).unwrap();

        assert!(events.is_empty());
        assert_eq!(reports[0].files, 0);
        assert_eq!(reports[0].events, 0);
    }

    #[test]
    fn reader_persists_cache_and_reads_appended_bytes_after_restart() {
        let home = tempdir().unwrap();
        let state = tempdir().unwrap();
        let codex = home.path().join(".codex").join("archived_sessions");
        fs::create_dir_all(&codex).unwrap();
        let path = codex.join("rollout.jsonl");
        fs::write(
            &path,
            codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107),
        )
        .unwrap();
        let reader = Reader::with_state(home.path(), state.path());
        assert_eq!(reader.events_since(None).unwrap().0.len(), 1);

        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(codex_token_row("2026-05-19T16:02:00Z", 211, 318).as_bytes())
            .unwrap();
        let restarted = Reader::with_state(home.path(), state.path());
        let (events, _) = restarted.events_since(None).unwrap();

        assert!(has_event(&events, 107, "2026-05-19T16:00:00Z"));
        assert!(has_event(&events, 211, "2026-05-19T16:02:00Z"));
        assert!(state.path().join("usage-cache.json").exists());
    }

    #[test]
    fn reader_rejects_same_path_replacement_as_append() {
        let home = tempdir().unwrap();
        let state = tempdir().unwrap();
        let codex = home.path().join(".codex").join("archived_sessions");
        fs::create_dir_all(&codex).unwrap();
        let path = codex.join("rollout.jsonl");
        let initial = codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107);
        fs::write(&path, &initial).unwrap();
        let reader = Reader::with_state(home.path(), state.path());
        assert_eq!(reader.events_since(None).unwrap().0.len(), 1);

        let replaced = strings_of_length("not-json", initial.len())
            + &codex_token_row("2026-05-19T16:02:00Z", 211, 318);
        fs::write(&path, replaced).unwrap();
        let restarted = Reader::with_state(home.path(), state.path());

        assert!(restarted.events_since(None).is_err());
    }

    #[test]
    fn reader_rejects_same_path_replacement_after_unchanged_prefix() {
        let home = tempdir().unwrap();
        let state = tempdir().unwrap();
        let codex = home.path().join(".codex").join("archived_sessions");
        fs::create_dir_all(&codex).unwrap();
        let path = codex.join("rollout.jsonl");
        let prefix = strings_of_length(" ", 4096);
        let initial = prefix.clone()
            + &codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107);
        fs::write(&path, &initial).unwrap();
        let reader = Reader::with_state(home.path(), state.path());
        assert_eq!(reader.events_since(None).unwrap().0.len(), 1);

        let replaced = prefix
            + &strings_of_length("not-json", initial.len() - 4096)
            + &codex_token_row("2026-05-19T16:02:00Z", 211, 318);
        fs::write(&path, replaced).unwrap();
        let restarted = Reader::with_state(home.path(), state.path());

        assert!(restarted.events_since(None).is_err());
    }

    #[test]
    fn reader_scan_reports_provider_errors_without_losing_other_provider() {
        let home = tempdir().unwrap();
        let codex = home.path().join(".codex").join("archived_sessions");
        fs::create_dir_all(&codex).unwrap();
        fs::write(codex.join("bad.jsonl"), r#"{"bad""#).unwrap();
        let claude = home.path().join(".claude").join("projects").join("-repo");
        fs::create_dir_all(&claude).unwrap();
        fs::write(
            claude.join("session.jsonl"),
            r#"{"timestamp":"2026-05-19T20:00:00Z","requestId":"req_1","sessionId":"session_claude","uuid":"turn_1","cwd":"/repo","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_creation_input_tokens":30,"cache_read_input_tokens":40,"output_tokens":5}}}
"#,
        )
        .unwrap();

        let scan = Reader::new(home.path()).scan_since(None).unwrap();

        assert!(scan.error.is_some());
        assert_eq!(scan.sources[0].provider, "codex");
        assert!(scan.sources[0].error.is_some());
        assert_eq!(scan.sources[1].provider, "claude");
        assert_eq!(scan.sources[1].events, 1);
        assert_eq!(scan.events.len(), 1);
    }

    #[test]
    fn reader_hydrates_persisted_dedup_keys() {
        let home = tempdir().unwrap();
        let state = tempdir().unwrap();
        let codex = home.path().join(".codex").join("archived_sessions");
        fs::create_dir_all(&codex).unwrap();
        let path = codex.join("rollout.jsonl");
        fs::write(
            &path,
            codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107),
        )
        .unwrap();
        let reader = Reader::with_state(home.path(), state.path());
        assert_eq!(reader.events_since(None).unwrap().0.len(), 1);

        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(codex_token_row("2026-05-19T16:01:00Z", 107, 107).as_bytes())
            .unwrap();
        let restarted = Reader::with_state(home.path(), state.path());
        let (events, _) = restarted.events_since(None).unwrap();

        assert_eq!(events.len(), 1);
    }

    fn spent_after_last_boundary(events: &[Event]) -> i64 {
        let start = events
            .iter()
            .rposition(|event| matches!(event.kind, EventKind::UserInput))
            .map_or(0, |index| index + 1);
        events[start..]
            .iter()
            .filter(|event| matches!(event.kind, EventKind::TokenCheckpoint))
            .map(|event| event.spent_tokens)
            .sum()
    }

    #[test]
    fn codex_user_message_emits_a_turn_boundary() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        fs::write(
            &path,
            format!(
                r#"{{"timestamp":"2026-05-29T16:00:00Z","type":"session_meta","payload":{{"id":"s","cwd":"/repo"}}}}
{{"timestamp":"2026-05-29T16:00:01Z","type":"event_msg","payload":{{"type":"user_message"}}}}
{}{}"#,
                codex_token_row("2026-05-29T16:00:02Z", 100, 100),
                codex_token_row("2026-05-29T16:00:03Z", 200, 300),
            ),
        )
        .unwrap();

        let parsed = parse_codex_file(&path, 0, None, None).unwrap();
        let boundaries = parsed
            .events
            .iter()
            .filter(|event| matches!(event.kind, EventKind::UserInput))
            .count();

        assert_eq!(boundaries, 1);
        // Both checkpoints land after the boundary → one turn's spend. Each
        // fixture row spends uncached input (100-20) + output 5 + reasoning 2 = 87,
        // independent of the cached context size: 87 + 87 = 174.
        assert_eq!(spent_after_last_boundary(&parsed.events), 174);
    }

    #[test]
    fn codex_response_item_user_message_resets_the_turn() {
        // Codex often records a prompt only as a `response_item` message with
        // role "user" (no `user_message` UI event). That must still end the turn,
        // or spend accumulates across prompts instead of resetting.
        let dir = tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        fs::write(
            &path,
            format!(
                r#"{{"timestamp":"2026-05-29T16:00:00Z","type":"session_meta","payload":{{"id":"s","cwd":"/repo"}}}}
{{"timestamp":"2026-05-29T16:00:01Z","type":"response_item","payload":{{"type":"message","role":"user"}}}}
{}{{"timestamp":"2026-05-29T16:00:03Z","type":"response_item","payload":{{"type":"message","role":"user"}}}}
{}"#,
                codex_token_row("2026-05-29T16:00:02Z", 100, 100),
                codex_token_row("2026-05-29T16:00:04Z", 200, 300),
            ),
        )
        .unwrap();

        let parsed = parse_codex_file(&path, 0, None, None).unwrap();
        let boundaries = parsed
            .events
            .iter()
            .filter(|event| matches!(event.kind, EventKind::UserInput))
            .count();
        assert_eq!(boundaries, 2);
        // Spend resets at the second prompt, so only the final checkpoint counts:
        // uncached (100-20) + output 5 + reasoning 2 = 87, not 174.
        assert_eq!(spent_after_last_boundary(&parsed.events), 87);
    }

    #[test]
    fn codex_turn_spend_excludes_re_read_cached_context() {
        // A realistic tool-loop turn: three model calls, each re-reading a large
        // cached context. Fresh work each call is tiny (uncached input + output).
        // Turn spend must reflect only the fresh work, never the cached prefix.
        let dir = tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let row = |at: &str, input: i64, cached: i64, output: i64, total: i64| {
            format!(
                r#"{{"timestamp":"{at}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{output},"reasoning_output_tokens":0,"total_tokens":{total}}},"total_token_usage":{{"total_tokens":{total}}},"model_context_window":258400}}}}}}
"#
            )
        };
        fs::write(
            &path,
            format!(
                r#"{{"timestamp":"2026-05-29T16:00:00Z","type":"session_meta","payload":{{"id":"s","cwd":"/repo"}}}}
{{"timestamp":"2026-05-29T16:00:01Z","type":"event_msg","payload":{{"type":"user_message"}}}}
{}{}{}"#,
                row("2026-05-29T16:00:02Z", 50_000, 49_000, 200, 50_200),
                row("2026-05-29T16:00:03Z", 120_000, 119_000, 300, 120_300),
                row("2026-05-29T16:00:04Z", 260_000, 259_000, 400, 260_400),
            ),
        )
        .unwrap();

        let parsed = parse_codex_file(&path, 0, None, None).unwrap();
        // Naive sum of per-call totals would be ~430k. True fresh spend is just
        // (1000+200) + (1000+300) + (1000+400) = 3900.
        assert_eq!(spent_after_last_boundary(&parsed.events), 3900);
    }

    #[test]
    fn claude_human_text_is_a_boundary_but_tool_results_are_not() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("projects");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("session.jsonl");
        fs::write(
            &path,
            r#"{"timestamp":"2026-05-29T20:00:00Z","type":"user","sessionId":"s","cwd":"/repo","message":{"role":"user","content":"do the thing"}}
{"timestamp":"2026-05-29T20:00:01Z","type":"assistant","sessionId":"s","cwd":"/repo","message":{"id":"m1","model":"claude-opus-4-8","usage":{"input_tokens":10,"cache_creation_input_tokens":20,"cache_read_input_tokens":9999,"output_tokens":30}}}
{"timestamp":"2026-05-29T20:00:02Z","type":"user","sessionId":"s","cwd":"/repo","toolUseResult":{"ok":true},"message":{"role":"user","content":[{"type":"tool_result","content":"x"}]}}
{"timestamp":"2026-05-29T20:00:03Z","type":"assistant","sessionId":"s","cwd":"/repo","message":{"id":"m2","model":"claude-opus-4-8","usage":{"input_tokens":5,"cache_creation_input_tokens":0,"cache_read_input_tokens":9999,"output_tokens":40}}}
"#,
        )
        .unwrap();

        let events = parse_claude_file(&path, 0).unwrap();
        let boundaries = events
            .iter()
            .filter(|event| matches!(event.kind, EventKind::UserInput))
            .count();

        // The typed message is a boundary; the tool_result is not.
        assert_eq!(boundaries, 1);
        // Turn spend excludes cache_read: (10+20+30) + (5+0+40) = 105.
        assert_eq!(spent_after_last_boundary(&events), 105);
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

    fn has_event(events: &[Event], total: i64, at: &str) -> bool {
        events.iter().any(|event| {
            event.total_tokens == total
                && event.timestamp
                    == DateTime::parse_from_rfc3339(at)
                        .ok()
                        .map(|time| time.with_timezone(&Utc))
        })
    }

    fn strings_of_length(pattern: &str, len: usize) -> String {
        pattern.chars().cycle().take(len).collect()
    }
}
