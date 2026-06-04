use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Event, UsageError, validate_full_usage_file};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(super) struct ReaderState {
    #[serde(skip)]
    loaded: bool,
    #[serde(default)]
    files: HashMap<PathBuf, CachedFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct CachedFile {
    size: u64,
    modified: DateTime<Utc>,
    prefix_hash: String,
    pub(super) events: Vec<Event>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) provider_state: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedReaderState {
    version: u8,
    files: HashMap<PathBuf, CachedFile>,
}

pub(super) struct CachedRead {
    pub(super) events: Vec<Event>,
    pub(super) provider_state: Option<serde_json::Value>,
}

const PERSISTED_READER_STATE_VERSION: u8 = 3;

impl ReaderState {
    pub(super) fn load(&mut self, state_dir: Option<&Path>) -> Result<(), UsageError> {
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

    pub(super) fn prune_missing(
        &mut self,
        state_dir: Option<&Path>,
        root: &Path,
        paths: &[PathBuf],
    ) -> Result<(), UsageError> {
        let current = paths.iter().collect::<std::collections::HashSet<_>>();
        let before = self.files.len();
        self.files
            .retain(|path, _| !path_within(path, root) || current.contains(path));
        if self.files.len() != before {
            self.save(state_dir)?;
        }
        Ok(())
    }

    pub(super) fn read_cached(
        &mut self,
        state_dir: Option<&Path>,
        path: &Path,
        read: impl FnOnce(u64, Option<&CachedFile>) -> Result<CachedRead, UsageError>,
    ) -> Result<Vec<Event>, UsageError> {
        validate_full_usage_file(path)?;
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(source) => {
                self.files.remove(path);
                let _ = self.save(state_dir);
                return Err(UsageError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        let modified =
            system_time_to_utc(metadata.modified().map_err(|source| UsageError::Io {
                path: path.to_path_buf(),
                source,
            })?);
        let size = metadata.len();
        if let Some(cached) = self.files.get(path)
            && cached.size == size
            && cached.modified == modified
        {
            return Ok(cached.events.clone());
        }

        let cached = self.files.get(path).cloned();
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
                self.files.remove(path);
                let _ = self.save(state_dir);
                return Err(error);
            }
        };
        let prefix_hash = match file_prefix_hash(path, size) {
            Ok(hash) => hash,
            Err(error) => {
                self.files.remove(path);
                let _ = self.save(state_dir);
                return Err(error);
            }
        };
        let cached_file = CachedFile {
            size,
            modified,
            prefix_hash,
            events: next.events.clone(),
            provider_state: next.provider_state,
        };
        self.files.insert(path.to_path_buf(), cached_file);
        self.save(state_dir)?;
        Ok(next.events)
    }
}

fn path_within(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root).is_ok()
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
