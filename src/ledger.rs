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
