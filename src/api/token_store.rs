use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::ApiError;

pub fn load_or_create_token(state_dir: impl AsRef<Path>) -> Result<(String, PathBuf), ApiError> {
    let state_dir = state_dir.as_ref();
    let path = state_dir.join("api.token");
    match fs::read_to_string(&path) {
        Ok(content) => {
            set_file_private(&path)?;
            let token = content.trim().to_string();
            if token.is_empty() {
                return Err(ApiError::Config("api token file is empty".to_string()));
            }
            Ok((token, path))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(state_dir)
                .map_err(|source| ApiError::Internal(format!("create state dir: {source}")))?;
            set_dir_private(state_dir)?;
            let mut raw = [0u8; 32];
            getrandom::fill(&mut raw)
                .map_err(|source| ApiError::Internal(format!("generate api token: {source}")))?;
            let token = hex::encode(raw);
            write_new_private_file(&path, format!("{token}\n").as_bytes())?;
            Ok((token, path))
        }
        Err(error) => Err(ApiError::Internal(format!("read api token: {error}"))),
    }
}

fn set_dir_private(path: &Path) -> Result<(), ApiError> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|source| ApiError::Internal(format!("chmod state dir: {source}")))?;
    }
    Ok(())
}

fn set_file_private(path: &Path) -> Result<(), ApiError> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|source| ApiError::Internal(format!("chmod api token: {source}")))?;
    }
    Ok(())
}

fn write_new_private_file(path: &Path, content: &[u8]) -> Result<(), ApiError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|source| ApiError::Internal(format!("create api token: {source}")))?;
    file.write_all(content)
        .map_err(|source| ApiError::Internal(format!("write api token: {source}")))?;
    set_file_private(path)
}
