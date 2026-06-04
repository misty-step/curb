use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const CODEX_LIVE_COLD_READ_LIMIT: u64 = 256 * 1024;
const USAGE_FILE_MAX_BYTES: u64 = 256 * 1024 * 1024;

mod cache;
mod claude;
mod codex;
mod discovery;
mod events;
mod lines;
mod pi;
mod provider;

use cache::{CachedFile, CachedRead, ReaderState};
use discovery::{
    jsonl_files_one_level, jsonl_files_recursive, modified_since, reject_symlink,
    validate_full_usage_file,
};
use events::{dedupe, event_is_since, parse_time, sort_events, user_input_event};
use lines::read_usage_line;
use provider::{Layout, Provider, ProviderRoot};

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceReport {
    pub provider: String,
    pub files: usize,
    pub events: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Scan {
    pub events: Vec<Event>,
    pub sources: Vec<SourceReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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

    pub fn scan_since(&self, since: Option<DateTime<Utc>>) -> Result<Scan, UsageError> {
        let mut state = self.state.lock().expect("usage reader mutex poisoned");
        state.load(self.state_dir.as_deref())?;
        let scan = provider::scan_all(&mut state, self.state_dir.as_deref(), &self.home, since)?;
        Ok(Scan {
            events: scan.events,
            sources: scan.sources,
            error: scan.error,
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
    codex::archived_sessions_since(root.as_ref(), since)
}

pub fn claude_projects_since(
    root: impl AsRef<Path>,
    since: Option<DateTime<Utc>>,
) -> Result<(Vec<Event>, SourceReport), UsageError> {
    claude::projects_since(root.as_ref(), since)
}

pub fn pi_sessions_since(
    root: impl AsRef<Path>,
    since: Option<DateTime<Utc>>,
) -> Result<(Vec<Event>, SourceReport), UsageError> {
    pi::sessions_since(root.as_ref(), since)
}

fn read_cached(
    state: &mut ReaderState,
    state_dir: Option<&Path>,
    path: &Path,
    read: impl FnOnce(u64, Option<&CachedFile>) -> Result<CachedRead, UsageError>,
) -> Result<Vec<Event>, UsageError> {
    state.read_cached(state_dir, path, read)
}

#[cfg(test)]
mod tests;
