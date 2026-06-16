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
            let advice = source_recovery_advice(&source.provider, error);
            Some(RecoveryItemView {
                id: format!("source-{}", source.provider),
                label: format!("{} source", source.provider),
                status: advice.status,
                message: format!(
                    "{} usage metadata could not be read: {}. Raw provider paths and payloads are not shown in recovery.",
                    source.provider, error
                ),
                action: advice.action,
                command: Some("curb usage --since 24h".to_string()),
                path: None,
                runbook: Some("docs/runbooks/source-health.md".to_string()),
            })
        })
        .collect()
}

struct SourceRecoveryAdvice {
    status: String,
    action: String,
}

fn source_recovery_advice(provider: &str, error: &str) -> SourceRecoveryAdvice {
    let lower = error.to_ascii_lowercase();
    if lower.contains("invalid utf-8") {
        return SourceRecoveryAdvice {
            status: "invalid utf-8".to_string(),
            action: format!(
                "Run `curb usage --since 24h`; if the error repeats, rotate or archive the malformed {provider} provider log and rerun."
            ),
        };
    }
    if lower.contains("json") {
        return SourceRecoveryAdvice {
            status: "invalid json".to_string(),
            action: format!(
                "Run `curb usage --since 24h`; if the error repeats, rotate or archive the malformed {provider} provider log and rerun."
            ),
        };
    }
    if lower.contains("1 mib") || lower.contains("safety cap") {
        return SourceRecoveryAdvice {
            status: "oversized line".to_string(),
            action: format!(
                "Rotate or archive the oversized {provider} provider log, then run `curb usage --since 24h`."
            ),
        };
    }
    if lower.contains("permission") {
        return SourceRecoveryAdvice {
            status: "permission denied".to_string(),
            action: format!(
                "Restore read permission for the {provider} provider metadata directory, then run `curb usage --since 24h`."
            ),
        };
    }
    if lower.contains("symlink") || lower.contains("outside") || lower.contains("trusted root") {
        return SourceRecoveryAdvice {
            status: "trusted root".to_string(),
            action: format!(
                "Remove the refused symlink or move {provider} metadata back under its trusted provider root, then run `curb usage --since 24h`."
            ),
        };
    }
    SourceRecoveryAdvice {
        status: "unreadable".to_string(),
        action: "Run `curb usage --since 24h`; if the error repeats, inspect the provider metadata source listed by that command.".to_string(),
    }
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
