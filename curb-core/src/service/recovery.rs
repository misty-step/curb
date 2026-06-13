use crate::usage::SourceReport;

use super::RecoveryItemView;

pub(crate) fn sanitize_source_reports(sources: Vec<SourceReport>) -> Vec<SourceReport> {
    sources
        .into_iter()
        .map(|mut source| {
            if source.provider != "processes" {
                source.error = source.error.as_deref().map(sanitize_source_error);
            }
            source
        })
        .collect()
}

pub(crate) fn source_health_recovery(sources: &[SourceReport]) -> Vec<RecoveryItemView> {
    sources
        .iter()
        .filter(|source| source.provider != "processes")
        .filter_map(|source| {
            let error = source.error.as_deref()?;
            Some(RecoveryItemView {
                id: format!("source-{}", source.provider),
                label: format!("{} source", source.provider),
                status: "error".to_string(),
                message: format!(
                    "{} usage metadata could not be read: {}. Raw provider paths and payloads are not shown in recovery.",
                    source.provider, error
                ),
                action: "Run `curb usage --since 24h`.".to_string(),
                command: Some("curb usage --since 24h".to_string()),
                path: None,
                runbook: Some("docs/user-guide.md#recovery-surface".to_string()),
            })
        })
        .collect()
}

fn sanitize_source_error(error: &str) -> String {
    let lower = error.to_ascii_lowercase();
    if lower.contains("usage line exceeds") {
        "usage line exceeded the 1 MiB metadata safety cap"
    } else if lower.contains("utf-8") || lower.contains("utf8") {
        "provider usage metadata contains invalid UTF-8"
    } else if lower.contains("symlink") {
        "provider usage metadata resolved through a refused symlink"
    } else if lower.contains("outside") && lower.contains("root") {
        "provider usage metadata resolved outside its trusted root"
    } else if lower.contains("permission denied") {
        "provider usage metadata could not be read because permission was denied"
    } else if lower.contains("json") {
        "provider usage metadata JSON could not be parsed"
    } else {
        "provider usage metadata failed a metadata-only read"
    }
    .to_string()
}
