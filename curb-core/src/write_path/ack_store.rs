use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};

use crate::service::{ServiceError, SessionAck, session_ack_path};

pub fn write_session_ack(
    state_dir: &Path,
    session_key: &str,
    extend: std::time::Duration,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<SessionAck, ServiceError> {
    if session_key.is_empty() {
        return Err(ServiceError::InvalidAck(
            "session key is required".to_string(),
        ));
    }
    if extend.is_zero() {
        return Err(ServiceError::InvalidAck(
            "extension must be positive".to_string(),
        ));
    }
    let ack = SessionAck {
        session_key: session_key.to_string(),
        reason: reason.to_string(),
        until: now + chrono::Duration::from_std(extend).unwrap(),
        created_at: now,
    };
    let path = session_ack_path(state_dir, session_key);
    let dir = path.parent().unwrap_or(state_dir);
    fs::create_dir_all(dir).map_err(|source| ServiceError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|source| {
            ServiceError::Io {
                path: dir.to_path_buf(),
                source,
            }
        })?;
    }
    let content = serde_json::to_vec_pretty(&ack).map_err(|source| ServiceError::Json {
        path: path.clone(),
        source,
    })?;
    fs::write(&path, content).map_err(|source| ServiceError::Io {
        path: path.clone(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            ServiceError::Io {
                path: path.clone(),
                source,
            }
        })?;
    }
    Ok(ack)
}

fn delete_session_ack(state_dir: &Path, session_key: &str) -> Result<(), ServiceError> {
    let path = session_ack_path(state_dir, session_key);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ServiceError::Io { path, source }),
    }
}

pub(super) fn rollback_session_ack(
    state_dir: &Path,
    session_key: &str,
    previous: Option<SessionAck>,
) -> Result<(), ServiceError> {
    match previous {
        Some(previous) => {
            let extend = previous
                .until
                .signed_duration_since(previous.created_at)
                .to_std()
                .map_err(|_| {
                    ServiceError::InvalidAck("previous ack duration is invalid".to_string())
                })?;
            write_session_ack(
                state_dir,
                session_key,
                extend,
                &previous.reason,
                previous.created_at,
            )?;
            Ok(())
        }
        None => delete_session_ack(state_dir, session_key),
    }
}
