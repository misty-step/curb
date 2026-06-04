use std::io::{BufRead, Read};
use std::path::Path;

use super::UsageError;

const USAGE_LINE_MAX_BYTES: usize = 1024 * 1024;

pub(super) fn read_usage_line(
    reader: &mut impl BufRead,
    path: &Path,
) -> Result<Option<Vec<u8>>, UsageError> {
    let mut line = Vec::new();
    let read = reader
        .by_ref()
        .take((USAGE_LINE_MAX_BYTES + 1) as u64)
        .read_until(b'\n', &mut line)
        .map_err(|source| UsageError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if read == 0 {
        return Ok(None);
    }
    if line.len() > USAGE_LINE_MAX_BYTES {
        return Err(UsageError::Scan(format!(
            "usage line exceeds {} bytes: {}",
            USAGE_LINE_MAX_BYTES,
            path.display()
        )));
    }
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    Ok(Some(line))
}

#[cfg(test)]
pub(super) fn oversized_line_padding() -> String {
    std::iter::repeat_n("x", USAGE_LINE_MAX_BYTES).collect()
}
