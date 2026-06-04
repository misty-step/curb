use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use super::cache::ReaderState;
use super::{
    Event, SourceReport, UsageError, claude, codex, dedupe, event_is_since, jsonl_files_one_level,
    jsonl_files_recursive, modified_since, pi, sort_events,
};

/// How a provider's log files are laid out under a root directory.
#[derive(Clone, Copy)]
pub(super) enum Layout {
    /// `*.jsonl` directly in the root (e.g. Codex archived sessions).
    OneLevel,
    /// `*.jsonl` anywhere beneath the root (e.g. Codex live sessions, Claude
    /// projects).
    Recursive,
}

/// One log directory a provider writes to.
pub(super) struct ProviderRoot {
    pub(super) path: PathBuf,
    pub(super) layout: Layout,
    /// When true, a live scan (`since` set) reads only the tail of large files
    /// instead of the whole thing. Codex live sessions grow without bound.
    pub(super) tailable: bool,
}

/// Reads one file into events, creating/refreshing its incremental cache entry.
type FileReader = fn(&mut ReaderState, Option<&Path>, &Path) -> Result<Vec<Event>, UsageError>;
/// Reads only the tail of a large, still-growing live file.
type FileTailer = fn(&Path) -> Result<Vec<Event>, UsageError>;

/// A usage provider Curb ingests.
///
/// Adding another agent means adding a provider module that owns source roots,
/// parser wire structs, cached reads, tail behavior, and metadata-only parsing
/// without exposing prompt, response, screenshot, keystroke, or file-content
/// payloads. The scan loop stays provider-agnostic.
pub(super) struct Provider {
    /// Stable id used as the source name and as the session-key prefix.
    pub(super) id: &'static str,
    /// Where this provider writes logs, relative to the user's home.
    pub(super) roots: fn(&Path) -> Vec<ProviderRoot>,
    /// Read one file into events, refreshing the incremental cache.
    pub(super) read_file: FileReader,
    /// Read only the tail of a large live file. Used when a root is `tailable`.
    pub(super) tail_file: FileTailer,
}

pub(super) struct ProviderScan {
    pub(super) events: Vec<Event>,
    pub(super) sources: Vec<SourceReport>,
    pub(super) error: Option<String>,
}

/// The registered providers, in display order. Each provider owns its roots,
/// cached reads, tail behavior, and parser wire structs behind a module.
pub(super) fn scan_all(
    state: &mut ReaderState,
    state_dir: Option<&Path>,
    home: &Path,
    since: Option<DateTime<Utc>>,
) -> Result<ProviderScan, UsageError> {
    let mut events = Vec::new();
    let mut sources = Vec::new();
    let mut errors = Vec::new();
    for provider in providers() {
        match scan_provider(state, state_dir, home, &provider, since) {
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
    Ok(ProviderScan {
        events,
        sources,
        error: if errors.is_empty() {
            None
        } else {
            Some(errors.join("; "))
        },
    })
}

fn providers() -> Vec<Provider> {
    vec![codex::provider(), claude::provider(), pi::provider()]
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
        state.prune_missing(state_dir, &root.path, &paths)?;
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
