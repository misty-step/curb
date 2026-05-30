use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("ledger event type is required")]
    MissingType,
    #[error("ledger path has no parent: {0}")]
    MissingParent(PathBuf),
    #[error("ledger io {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("ledger json {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Event {
    #[serde(rename = "type")]
    pub event_type: String,
    pub seq: i64,
    pub ts: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_hash: Option<String>,
}

impl Event {
    pub fn new(event_type: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
            seq: 0,
            ts: Utc::now(),
            run_id: None,
            agent_id: None,
            mode: None,
            message: None,
            data: None,
            prev_hash: None,
            event_hash: None,
        }
    }

    pub fn with_data(mut self, data: Map<String, Value>) -> Self {
        self.data = Some(data);
        self
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        let message = message.into();
        if !message.is_empty() {
            self.message = Some(message);
        }
        self
    }

    pub fn with_mode(mut self, mode: impl Into<String>) -> Self {
        let mode = mode.into();
        if !mode.is_empty() {
            self.mode = Some(mode);
        }
        self
    }
}

/// The closed taxonomy of ledger `event_type` strings.
///
/// Both the emit side (usagewatch/runtime/service) and the read side
/// (service's alert and event views) share this single mapping instead of
/// re-deriving an event's meaning from substring sniffing. The wire strings
/// stay an implementation detail behind [`LedgerEvent::as_str`] /
/// [`LedgerEvent::parse`]; on-disk ledgers and existing tests keep parsing
/// byte-identical strings.
///
/// Adding a future event means adding a variant here, which forces every
/// classification accessor's exhaustive `match` to handle it — a new event
/// fails to compile rather than silently mis-coloring the dashboard.
///
/// This is deliberately distinct from `usage::EventKind`, which classifies
/// the provider USAGE logs Curb parses; this taxonomy classifies the policy
/// lifecycle Curb itself records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LedgerEvent {
    ServiceStarted,
    ServiceStopped,
    RunStarted,
    RunStopped,
    AckReceived,
    SessionAckReceived,
    AckRejected,
    PolicyWarning,
    UsageWarning,
    UsageWouldTerminate,
    UsageKillBlocked,
    UsageGraceStarted,
    UsageTerminationStarted,
    UsageTerminationCompleted,
    UsageTerminationFailed,
    TerminationStarted,
    TerminationCompleted,
    TerminationFailed,
    UsageScanFailed,
    ScanFailed,
    NotificationFailed,
    ManualStopStarted,
    ManualStopCompleted,
}

/// Coarse category / kind labels for an [`EventView`]-style row, mirroring the
/// historical `event_class` mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewClass {
    pub category: &'static str,
    pub kind: &'static str,
}

/// Alert-view classification for events that surface as policy alerts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AlertClass {
    pub category: &'static str,
    pub severity: &'static str,
    pub label: &'static str,
    pub actionable: bool,
    pub explanation: &'static str,
}

impl LedgerEvent {
    /// Parse a wire `event_type` string into its typed variant, or `None` for
    /// an unknown event the read model treats generically.
    #[must_use]
    pub fn parse(event_type: &str) -> Option<Self> {
        let event = match event_type {
            "service_started" => Self::ServiceStarted,
            "service_stopped" => Self::ServiceStopped,
            "run_started" => Self::RunStarted,
            "run_stopped" => Self::RunStopped,
            "ack_received" => Self::AckReceived,
            "session_ack_received" => Self::SessionAckReceived,
            "ack_rejected" => Self::AckRejected,
            "policy_warning" => Self::PolicyWarning,
            "usage_warning" => Self::UsageWarning,
            "usage_would_terminate" => Self::UsageWouldTerminate,
            "usage_kill_blocked" => Self::UsageKillBlocked,
            "usage_grace_started" => Self::UsageGraceStarted,
            "usage_termination_started" => Self::UsageTerminationStarted,
            "usage_termination_completed" => Self::UsageTerminationCompleted,
            "usage_termination_failed" => Self::UsageTerminationFailed,
            "termination_started" => Self::TerminationStarted,
            "termination_completed" => Self::TerminationCompleted,
            "termination_failed" => Self::TerminationFailed,
            "usage_scan_failed" => Self::UsageScanFailed,
            "scan_failed" => Self::ScanFailed,
            "notification_failed" => Self::NotificationFailed,
            "manual_stop_started" => Self::ManualStopStarted,
            "manual_stop_completed" => Self::ManualStopCompleted,
            _ => return None,
        };
        Some(event)
    }

    /// The byte-identical wire string written to and read from the ledger.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ServiceStarted => "service_started",
            Self::ServiceStopped => "service_stopped",
            Self::RunStarted => "run_started",
            Self::RunStopped => "run_stopped",
            Self::AckReceived => "ack_received",
            Self::SessionAckReceived => "session_ack_received",
            Self::AckRejected => "ack_rejected",
            Self::PolicyWarning => "policy_warning",
            Self::UsageWarning => "usage_warning",
            Self::UsageWouldTerminate => "usage_would_terminate",
            Self::UsageKillBlocked => "usage_kill_blocked",
            Self::UsageGraceStarted => "usage_grace_started",
            Self::UsageTerminationStarted => "usage_termination_started",
            Self::UsageTerminationCompleted => "usage_termination_completed",
            Self::UsageTerminationFailed => "usage_termination_failed",
            Self::TerminationStarted => "termination_started",
            Self::TerminationCompleted => "termination_completed",
            Self::TerminationFailed => "termination_failed",
            Self::UsageScanFailed => "usage_scan_failed",
            Self::ScanFailed => "scan_failed",
            Self::NotificationFailed => "notification_failed",
            Self::ManualStopStarted => "manual_stop_started",
            Self::ManualStopCompleted => "manual_stop_completed",
        }
    }

    /// The coarse `(category, kind)` an [`EventView`] row uses.
    #[must_use]
    pub fn view_class(self) -> ViewClass {
        let (category, kind) = match self {
            Self::ServiceStarted => ("service", "started"),
            Self::ServiceStopped => ("service", "stopped"),
            Self::RunStarted => ("run", "started"),
            Self::RunStopped => ("run", "stopped"),
            Self::AckReceived | Self::SessionAckReceived => ("ack", "received"),
            Self::AckRejected => ("ack", "rejected"),
            Self::PolicyWarning | Self::UsageWarning => ("alert", "warning"),
            Self::UsageWouldTerminate => ("alert", "would_stop"),
            Self::UsageKillBlocked => ("alert", "blocked"),
            Self::UsageGraceStarted => ("alert", "grace"),
            Self::UsageTerminationStarted | Self::TerminationStarted => ("termination", "started"),
            Self::UsageTerminationCompleted | Self::TerminationCompleted => {
                ("termination", "completed")
            }
            Self::UsageTerminationFailed | Self::TerminationFailed => ("termination", "failed"),
            Self::ScanFailed | Self::UsageScanFailed => ("error", "scan_failed"),
            Self::NotificationFailed => ("error", "notification_failed"),
            Self::ManualStopStarted | Self::ManualStopCompleted => ("other", "recorded"),
        };
        ViewClass { category, kind }
    }

    /// Whether this event surfaces as a policy alert in the alert feed.
    ///
    /// Mirrors the historical `alert_event` predicate (warning / terminate /
    /// termination / kill / grace), expressed exhaustively.
    #[must_use]
    pub fn is_alert(self) -> bool {
        matches!(
            self,
            Self::PolicyWarning
                | Self::UsageWarning
                | Self::UsageWouldTerminate
                | Self::UsageKillBlocked
                | Self::UsageGraceStarted
                | Self::UsageTerminationStarted
                | Self::UsageTerminationCompleted
                | Self::UsageTerminationFailed
                | Self::TerminationStarted
                | Self::TerminationCompleted
                | Self::TerminationFailed
        )
    }

    /// Full alert-view classification, for events where [`is_alert`](Self::is_alert)
    /// holds. The three termination phases stay distinct: `*GraceStarted` is the
    /// pre-kill waiting state (`grace`), `*TerminationStarted` is the
    /// kill-in-progress state (`stopping`), and `*TerminationCompleted` is the
    /// finished state (`stopped`).
    #[must_use]
    pub fn alert_class(self) -> AlertClass {
        let category = match self {
            Self::UsageTerminationCompleted | Self::TerminationCompleted => "stopped",
            Self::UsageGraceStarted => "grace",
            Self::UsageTerminationStarted | Self::TerminationStarted => "stopping",
            Self::UsageWouldTerminate => "would_stop",
            Self::UsageKillBlocked => "blocked",
            Self::UsageTerminationFailed | Self::TerminationFailed => "failed",
            _ => "warning",
        };
        let severity = match self {
            Self::UsageTerminationCompleted => "stop",
            Self::UsageTerminationFailed | Self::TerminationFailed => "error",
            Self::UsageKillBlocked => "blocked",
            Self::UsageWouldTerminate | Self::UsageGraceStarted => "watch",
            _ => "warn",
        };
        let label = match self {
            Self::UsageTerminationCompleted | Self::TerminationCompleted => "stopped",
            Self::UsageGraceStarted => "grace",
            Self::UsageTerminationStarted | Self::TerminationStarted => "stopping",
            Self::UsageWouldTerminate => "would stop",
            Self::UsageKillBlocked => "blocked",
            Self::UsageTerminationFailed | Self::TerminationFailed => "failed",
            _ => "warning",
        };
        let actionable = matches!(
            self,
            Self::UsageTerminationStarted | Self::UsageTerminationCompleted
        );
        let explanation = match self {
            Self::UsageWouldTerminate => {
                "Alert mode: Curb would stop this correlated worker in enforcement mode."
            }
            Self::UsageKillBlocked => {
                "Curb did not stop anything because the session was uncorrelated or watch-only."
            }
            Self::UsageGraceStarted => "Enforcement grace period started for a correlated worker.",
            Self::UsageTerminationStarted => "Curb started terminating a correlated worker.",
            Self::UsageTerminationCompleted => {
                "Curb completed termination for a correlated worker."
            }
            Self::PolicyWarning | Self::UsageWarning => {
                "Usage or runtime crossed the warning policy."
            }
            _ => "",
        };
        AlertClass {
            category,
            severity,
            label,
            actionable,
            explanation,
        }
    }
}

type AppendHook = Arc<dyn Fn(&Event, &[u8]) + Send + Sync>;

#[derive(Clone, Default)]
pub struct Options {
    pub metadata: Map<String, Value>,
    pub after_append: Option<AppendHook>,
}

pub struct Ledger {
    path: PathBuf,
    state: Mutex<State>,
    metadata: Map<String, Value>,
    after_append: Option<AppendHook>,
}

#[derive(Default)]
struct State {
    seq: i64,
    prev_hash: Option<String>,
}

impl Ledger {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LedgerError> {
        Self::open_with_options(path, Options::default())
    }

    pub fn open_with_options(
        path: impl AsRef<Path>,
        options: Options,
    ) -> Result<Self, LedgerError> {
        let path = path.as_ref().to_path_buf();
        let parent = path
            .parent()
            .ok_or_else(|| LedgerError::MissingParent(path.clone()))?;
        fs::create_dir_all(parent).map_err(|source| LedgerError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        let events = read(&path)?;
        let state = events.last().map_or_else(State::default, |tail| State {
            seq: tail.seq,
            prev_hash: tail.event_hash.clone(),
        });
        Ok(Self {
            path,
            state: Mutex::new(state),
            metadata: options.metadata,
            after_append: options.after_append,
        })
    }

    pub fn append(&self, mut event: Event) -> Result<Event, LedgerError> {
        if event.event_type.is_empty() {
            return Err(LedgerError::MissingType);
        }

        let mut state = self.state.lock().expect("ledger mutex poisoned");
        event.seq = state.seq + 1;
        event.ts = Utc::now();
        event.prev_hash = state.prev_hash.clone();
        event.data = merge_metadata(event.data.take(), &self.metadata).map(scrub_sensitive_data);
        event.event_hash = Some(hash_event(&event)?);
        let line = serde_json::to_vec(&event).map_err(|source| LedgerError::Json {
            path: self.path.clone(),
            source,
        })?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|source| LedgerError::Io {
                path: self.path.clone(),
                source,
            })?;
        file.write_all(&line).map_err(|source| LedgerError::Io {
            path: self.path.clone(),
            source,
        })?;
        file.write_all(b"\n").map_err(|source| LedgerError::Io {
            path: self.path.clone(),
            source,
        })?;

        state.seq = event.seq;
        state.prev_hash = event.event_hash.clone();
        if let Some(hook) = &self.after_append {
            hook(&event, &line);
        }
        Ok(event)
    }
}

pub fn read(path: impl AsRef<Path>) -> Result<Vec<Event>, LedgerError> {
    let path = path.as_ref();
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(LedgerError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|source| LedgerError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let event = serde_json::from_str(&line).map_err(|source| LedgerError::Json {
            path: path.to_path_buf(),
            source,
        })?;
        events.push(event);
    }
    Ok(events)
}

fn hash_event(event: &Event) -> Result<String, LedgerError> {
    let canonical = json!({
        "type": event.event_type,
        "seq": event.seq,
        "ts": event.ts,
        "run_id": event.run_id,
        "agent_id": event.agent_id,
        "mode": event.mode,
        "message": event.message,
        "data": event.data,
        "prev_hash": event.prev_hash,
    });
    let raw = serde_json::to_vec(&canonical).map_err(|source| LedgerError::Json {
        path: PathBuf::from("<canonical-event>"),
        source,
    })?;
    Ok(hex::encode(Sha256::digest(raw)))
}

fn merge_metadata(
    data: Option<Map<String, Value>>,
    metadata: &Map<String, Value>,
) -> Option<Map<String, Value>> {
    if data.is_none() && metadata.is_empty() {
        return None;
    }
    let mut out = data.unwrap_or_default();
    for (key, value) in metadata {
        out.entry(key.clone()).or_insert_with(|| value.clone());
    }
    Some(out)
}

fn scrub_sensitive_data(data: Map<String, Value>) -> Map<String, Value> {
    data.into_iter()
        .map(|(key, value)| {
            if sensitive_data_key(&key) {
                (key, Value::String("[redacted]".to_string()))
            } else {
                (key, scrub_sensitive_value(value))
            }
        })
        .collect()
}

fn scrub_sensitive_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(scrub_sensitive_data(map)),
        Value::Array(items) => Value::Array(items.into_iter().map(scrub_sensitive_value).collect()),
        other => other,
    }
}

fn sensitive_data_key(key: &str) -> bool {
    matches!(
        key.to_lowercase().replace('-', "_").as_str(),
        "prompt"
            | "prompts"
            | "response"
            | "responses"
            | "completion"
            | "completions"
            | "message_content"
            | "content"
            | "contents"
            | "file_content"
            | "file_contents"
            | "screenshot"
            | "screenshots"
            | "keystroke"
            | "keystrokes"
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::{Map, json};
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn append_and_read_hash_chain() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("runs.ndjson");
        let ledger = Ledger::open(&path).unwrap();

        ledger.append(Event::new("run_started")).unwrap();
        ledger.append(Event::new("run_stopped")).unwrap();

        let events = read(&path).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].prev_hash, events[0].event_hash);
    }

    #[test]
    fn append_enriches_events_with_metadata_without_overwriting_explicit_data() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("runs.ndjson");
        let mut metadata = Map::new();
        metadata.insert("machine_id".to_string(), json!("machine_test"));
        let ledger = Ledger::open_with_options(
            &path,
            Options {
                metadata,
                after_append: None,
            },
        )
        .unwrap();

        let mut explicit = Map::new();
        explicit.insert("machine_id".to_string(), json!("explicit"));
        explicit.insert("session".to_string(), json!("s1"));
        ledger
            .append(Event::new("usage_warning").with_data(explicit))
            .unwrap();
        ledger.append(Event::new("usage_warning")).unwrap();

        let events = read(&path).unwrap();
        assert_eq!(
            events[0].data.as_ref().unwrap().get("machine_id"),
            Some(&json!("explicit"))
        );
        assert_eq!(
            events[1].data.as_ref().unwrap().get("machine_id"),
            Some(&json!("machine_test"))
        );
    }

    #[test]
    fn append_redacts_sensitive_data_fields() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("runs.ndjson");
        let ledger = Ledger::open(&path).unwrap();
        let mut data = Map::new();
        data.insert("prompt".to_string(), json!("secret prompt"));
        data.insert(
            "nested".to_string(),
            json!({"response": "secret response", "safe": "metadata"}),
        );
        data.insert(
            "items".to_string(),
            json!([{"file_contents": "secret file"}]),
        );

        ledger
            .append(Event::new("usage_warning").with_data(data))
            .unwrap();

        let events = read(&path).unwrap();
        let data = events[0].data.as_ref().unwrap();
        assert_eq!(data.get("prompt"), Some(&json!("[redacted]")));
        assert_eq!(data["nested"]["response"], json!("[redacted]"));
        assert_eq!(data["nested"]["safe"], json!("metadata"));
        assert_eq!(data["items"][0]["file_contents"], json!("[redacted]"));
    }

    #[test]
    fn append_calls_hook_after_local_write() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("runs.ndjson");
        let received: Arc<Mutex<Option<Event>>> = Arc::new(Mutex::new(None));
        let hook_received = Arc::clone(&received);
        let ledger = Ledger::open_with_options(
            &path,
            Options {
                metadata: Map::new(),
                after_append: Some(Arc::new(move |event, _line| {
                    *hook_received.lock().unwrap() = Some(event.clone());
                })),
            },
        )
        .unwrap();

        ledger.append(Event::new("usage_warning")).unwrap();

        assert_eq!(read(&path).unwrap().len(), 1);
        assert_eq!(
            received.lock().unwrap().as_ref().unwrap().event_type,
            "usage_warning"
        );
    }

    /// Every wire string Curb emits (or still reads for back-compat) must
    /// round-trip through the taxonomy to a non-default classification, so a
    /// renamed or added event can never silently fall back to the generic
    /// "other/recorded" bucket without a deliberate variant.
    #[test]
    fn ledger_event_round_trips_every_emitted_wire_string() {
        // The full set of event_type strings emitted across the codebase plus
        // the legacy aliases the read model still classifies. Keep in sync
        // with `grep -rno '"usage_[a-z_]*"\|"manual_[a-z_]*"' src/` and the
        // non-usage event types in `event_class`.
        let wire_strings = [
            "service_started",
            "service_stopped",
            "run_started",
            "run_stopped",
            "ack_received",
            "session_ack_received",
            "ack_rejected",
            "policy_warning",
            "usage_warning",
            "usage_would_terminate",
            "usage_kill_blocked",
            "usage_grace_started",
            "usage_termination_started",
            "usage_termination_completed",
            "usage_termination_failed",
            "termination_started",
            "termination_completed",
            "termination_failed",
            "usage_scan_failed",
            "scan_failed",
            "notification_failed",
            "manual_stop_started",
            "manual_stop_completed",
        ];

        for wire in wire_strings {
            let event = LedgerEvent::parse(wire)
                .unwrap_or_else(|| panic!("{wire} should parse into the taxonomy"));
            assert_eq!(
                event.as_str(),
                wire,
                "{wire} must round-trip byte-identically for wire compatibility"
            );
            let view = event.view_class();
            assert!(
                (view.category, view.kind) != ("other", "recorded")
                    // manual_stop_* are recorded generically by design.
                    || wire.starts_with("manual_stop_"),
                "{wire} fell through to the default view class"
            );
            if event.is_alert() {
                let alert = event.alert_class();
                assert!(
                    !alert.category.is_empty()
                        && !alert.severity.is_empty()
                        && !alert.label.is_empty(),
                    "{wire} is an alert but produced an empty classification"
                );
            }
        }
    }

    #[test]
    fn unknown_event_type_does_not_parse() {
        assert_eq!(LedgerEvent::parse("totally_made_up"), None);
    }

    #[test]
    fn termination_phases_classify_distinctly() {
        // grace = waiting before the kill; stopping = kill in progress;
        // stopped = finished. A kill-in-progress must not be mislabeled grace.
        let grace = LedgerEvent::UsageGraceStarted.alert_class();
        assert_eq!((grace.category, grace.label), ("grace", "grace"));

        for started in [
            LedgerEvent::UsageTerminationStarted,
            LedgerEvent::TerminationStarted,
        ] {
            let class = started.alert_class();
            assert_eq!(
                (class.category, class.label),
                ("stopping", "stopping"),
                "{started:?} should classify as stopping, not grace"
            );
        }
        // The live (emitted) start event is actionable; the legacy read-compat
        // alias is not — preserve that distinction.
        assert!(
            LedgerEvent::UsageTerminationStarted
                .alert_class()
                .actionable
        );

        let done = LedgerEvent::UsageTerminationCompleted.alert_class();
        assert_eq!((done.category, done.label), ("stopped", "stopped"));
    }

    #[test]
    fn reopening_continues_existing_hash_chain() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("runs.ndjson");
        Ledger::open(&path)
            .unwrap()
            .append(Event::new("one"))
            .unwrap();

        Ledger::open(&path)
            .unwrap()
            .append(Event::new("two"))
            .unwrap();

        let events = read(&path).unwrap();
        assert_eq!(events[1].seq, 2);
        assert_eq!(events[1].prev_hash, events[0].event_hash);
    }
}
