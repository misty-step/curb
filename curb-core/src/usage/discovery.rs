use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use super::{USAGE_FILE_MAX_BYTES, UsageError};

pub(super) fn modified_since(
    path: &Path,
    since: Option<DateTime<Utc>>,
) -> Result<bool, UsageError> {
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

pub(super) fn validate_full_usage_file(path: &Path) -> Result<(), UsageError> {
    reject_symlink(path)?;
    let metadata = fs::metadata(path).map_err(|source| UsageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > USAGE_FILE_MAX_BYTES {
        return Err(UsageError::Scan(format!(
            "usage file {} exceeds {} bytes",
            path.display(),
            USAGE_FILE_MAX_BYTES
        )));
    }
    Ok(())
}

pub(super) fn reject_symlink(path: &Path) -> Result<(), UsageError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| UsageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(UsageError::Scan(format!(
            "usage file {} is a symlink",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn jsonl_files_one_level(root: &Path) -> Result<Vec<PathBuf>, UsageError> {
    let mut out = Vec::new();
    let Some(root_canonical) = canonical_existing_dir(root)? else {
        return Ok(out);
    };
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
            validate_discovered_usage_file(&root_canonical, &path)?;
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

pub(super) fn jsonl_files_recursive(root: &Path) -> Result<Vec<PathBuf>, UsageError> {
    let mut out = Vec::new();
    let Some(root_canonical) = canonical_existing_dir(root)? else {
        return Ok(out);
    };
    collect_jsonl(&root_canonical, root, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_jsonl(
    root_canonical: &Path,
    directory: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), UsageError> {
    validate_discovered_directory(root_canonical, directory)?;
    let Ok(entries) = fs::read_dir(directory) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry.map_err(|source| UsageError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| UsageError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_symlink() {
            return Err(UsageError::Scan(format!(
                "usage path {} is a symlink",
                path.display()
            )));
        }
        if file_type.is_dir() {
            collect_jsonl(root_canonical, &path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            validate_discovered_usage_file(root_canonical, &path)?;
            out.push(path);
        }
    }
    Ok(())
}

fn canonical_existing_dir(path: &Path) -> Result<Option<PathBuf>, UsageError> {
    match fs::canonicalize(path) {
        Ok(canonical) => Ok(Some(canonical)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(UsageError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validate_discovered_directory(root_canonical: &Path, path: &Path) -> Result<(), UsageError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| UsageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(UsageError::Scan(format!(
            "usage path {} is a symlink",
            path.display()
        )));
    }
    let canonical = fs::canonicalize(path).map_err(|source| UsageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical.starts_with(root_canonical) {
        return Err(UsageError::Scan(format!(
            "usage path {} escapes root {}",
            path.display(),
            root_canonical.display()
        )));
    }
    Ok(())
}

fn validate_discovered_usage_file(root_canonical: &Path, path: &Path) -> Result<(), UsageError> {
    reject_symlink(path)?;
    let canonical = fs::canonicalize(path).map_err(|source| UsageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical.starts_with(root_canonical) {
        return Err(UsageError::Scan(format!(
            "usage file {} escapes root {}",
            path.display(),
            root_canonical.display()
        )));
    }
    Ok(())
}
