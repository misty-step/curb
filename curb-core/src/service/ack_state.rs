use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::ServiceError;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionAck {
    pub session_key: String,
    pub reason: String,
    pub until: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

// This is a READ for snapshot derivation: `build_session_view` needs the
// `acknowledged_until` to compute the alert/can_stop flags. All ack mutation
// lives in `write_path`; this module owns the shared ack file shape and lookup.
pub fn read_session_ack(
    state_dir: &Path,
    session_key: &str,
) -> Result<Option<SessionAck>, ServiceError> {
    let path = session_ack_path(state_dir, session_key);
    let content = match fs::read(&path) {
        Ok(content) => content,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(ServiceError::Io { path, source }),
    };
    serde_json::from_slice(&content)
        .map(Some)
        .map_err(|source| ServiceError::Json { path, source })
}

pub fn active_session_ack(
    state_dir: &Path,
    session_key: &str,
    now: DateTime<Utc>,
) -> Result<Option<SessionAck>, ServiceError> {
    let Some(ack) = read_session_ack(state_dir, session_key)? else {
        return Ok(None);
    };
    if now < ack.until {
        Ok(Some(ack))
    } else {
        Ok(None)
    }
}

pub(crate) fn session_ack_path(state_dir: &Path, session_key: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(session_key.as_bytes());
    state_dir
        .join("usage-acks")
        .join(format!("{}.json", hex::encode(hasher.finalize())))
}
