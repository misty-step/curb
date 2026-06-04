use std::fs;

use crate::config::Config;
use crate::onboarding::NotificationView;
use crate::platform::TerminationCapability;
use crate::runtime::RuntimeError;
use crate::service::{ReadinessCheckView, ReadinessView};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SnapshotCacheStatus {
    Ready,
    Unavailable,
    Busy,
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
        readiness_check("watcher_runtime", snapshot_cache.as_result()),
    ];
    let ready = checks.iter().all(|check| check.status == "ok");
    ReadinessView {
        status: if ready { "ready" } else { "degraded" }.to_string(),
        app: "curb".to_string(),
        api_version: 1,
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

impl SnapshotCacheStatus {
    fn as_result(self) -> Result<(), String> {
        match self {
            Self::Ready => Ok(()),
            Self::Unavailable => Err("snapshot unavailable".to_string()),
            Self::Busy => Err("cache busy".to_string()),
            Self::Poisoned => Err("cache mutex poisoned".to_string()),
        }
    }
}
