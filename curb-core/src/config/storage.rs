use std::fs;
use std::path::Path;

use super::ConfigError;

pub(super) fn write_private_file(path: &Path, content: &[u8]) -> Result<(), ConfigError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    use std::io::Write;
    file.write_all(content)
        .map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source,
        })
}

pub(super) fn set_dir_private(path: &Path) -> Result<(), ConfigError> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
            ConfigError::Write {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    Ok(())
}

pub(super) fn set_file_private(path: &Path) -> Result<(), ConfigError> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            ConfigError::Write {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    Ok(())
}
