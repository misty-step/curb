use std::fs;

use crate::config::Config;
use crate::onboarding::NotificationView;
use crate::platform::TerminationCapability;
use crate::runtime::RuntimeError;
use crate::service::{ReadinessCheckView, ReadinessView, RecoveryItemView};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SnapshotCacheStatus {
    Ready,
    Unavailable,
    Busy,
    RefreshingCached,
    Poisoned,
}

pub(crate) fn readiness_view(
    cfg: &Config,
    notifications: Result<NotificationView, RuntimeError>,
    termination: TerminationCapability,
    snapshot_cache: SnapshotCacheStatus,
) -> ReadinessView {
    let checks = vec![
        readiness_check("config", Ok::<(), String>(())),
        readiness_check(
            "ledger",
            crate::ledger::Ledger::open(&cfg.ledger.path).map(|_| ()),
        ),
        readiness_check(
            "usage_reader",
            fs::create_dir_all(cfg.service.state_dir.join("usage")),
        ),
        readiness_check(
            "platform_capabilities",
            if termination.supported {
                Ok(())
            } else {
                Err(termination.message)
            },
        ),
        readiness_check("notifications", notifications.map(|_| ())),
        snapshot_cache_readiness(snapshot_cache),
    ];
    let ready = checks.iter().all(|check| check.status == "ok");
    ReadinessView {
        status: if ready { "ready" } else { "degraded" }.to_string(),
        app: "curb".to_string(),
        api_version: 1,
        recovery: readiness_recovery(cfg, &checks),
        checks,
    }
}

fn readiness_check<E: ToString>(name: &str, result: Result<(), E>) -> ReadinessCheckView {
    match result {
        Ok(()) => ReadinessCheckView {
            name: name.to_string(),
            status: "ok".to_string(),
            reason: None,
        },
        Err(error) => ReadinessCheckView {
            name: name.to_string(),
            status: "error".to_string(),
            reason: Some(error.to_string()),
        },
    }
}

fn snapshot_cache_readiness(status: SnapshotCacheStatus) -> ReadinessCheckView {
    match status {
        SnapshotCacheStatus::Ready => readiness_check("watcher_runtime", Ok::<(), String>(())),
        SnapshotCacheStatus::RefreshingCached => ReadinessCheckView {
            name: "watcher_runtime".to_string(),
            status: "ok".to_string(),
            reason: Some("snapshot refresh in progress; serving cached snapshot".to_string()),
        },
        SnapshotCacheStatus::Unavailable => {
            readiness_check("watcher_runtime", Err("snapshot unavailable"))
        }
        SnapshotCacheStatus::Busy => readiness_check("watcher_runtime", Err("cache busy")),
        SnapshotCacheStatus::Poisoned => {
            readiness_check("watcher_runtime", Err("cache mutex poisoned"))
        }
    }
}

fn readiness_recovery(cfg: &Config, checks: &[ReadinessCheckView]) -> Vec<RecoveryItemView> {
    checks
        .iter()
        .filter(|check| check.status != "ok")
        .map(|check| {
            let (label, message, command, path) = match check.name.as_str() {
                "ledger" => (
                    "Ledger",
                    format!(
                        "The append-only ledger is not writable: {}",
                        check.reason.as_deref().unwrap_or("unknown error")
                    ),
                    Some("curb doctor".to_string()),
                    Some(cfg.ledger.path.display().to_string()),
                ),
                "usage_reader" => (
                    "Usage reader",
                    "Curb could not prepare the local usage-reader state directory.".to_string(),
                    Some("curb usage --since 24h".to_string()),
                    Some(cfg.service.state_dir.join("usage").display().to_string()),
                ),
                "platform_capabilities" => (
                    "Platform capabilities",
                    check
                        .reason
                        .clone()
                        .unwrap_or_else(|| "platform capability check failed".to_string()),
                    Some("curb doctor".to_string()),
                    None,
                ),
                "notifications" => (
                    "Notifications",
                    check
                        .reason
                        .clone()
                        .unwrap_or_else(|| "notification health check failed".to_string()),
                    Some("curb doctor --test-notification".to_string()),
                    None,
                ),
                "watcher_runtime" => (
                    "Watcher runtime",
                    format!(
                        "The daemon snapshot cache is not ready: {}",
                        check.reason.as_deref().unwrap_or("unknown error")
                    ),
                    Some("curb watch --once".to_string()),
                    Some(cfg.service.state_dir.display().to_string()),
                ),
                _ => (
                    "Readiness",
                    check
                        .reason
                        .clone()
                        .unwrap_or_else(|| "readiness check failed".to_string()),
                    Some("curb doctor".to_string()),
                    Some(cfg.service.state_dir.display().to_string()),
                ),
            };
            RecoveryItemView {
                id: format!("readiness-{}", check.name),
                label: label.to_string(),
                status: check.status.clone(),
                message,
                action: match &path {
                    Some(path) => format!(
                        "Run `{}` and inspect {}.",
                        command.as_deref().unwrap_or("curb doctor"),
                        path
                    ),
                    None => format!("Run `{}`.", command.as_deref().unwrap_or("curb doctor")),
                },
                command,
                path,
                runbook: Some("docs/user-guide.md#recovery-surface".to_string()),
            }
        })
        .collect()
}
