use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind, Users};
use thiserror::Error;

mod capture;
mod notification;
mod target;
mod termination;
#[cfg(test)]
mod tests;

pub use target::TerminationTarget;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct Pid(i32);

impl Pid {
    pub const fn new(pid: i32) -> Self {
        Self(pid)
    }

    pub const fn get(self) -> i32 {
        self.0
    }

    fn is_dangerous(self) -> bool {
        self.0 <= 1 || self.0 == std::process::id() as i32
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Process {
    pub pid: Pid,
    pub ppid: Option<Pid>,
    pub name: String,
    pub executable: Option<PathBuf>,
    pub command: String,
    pub cwd: Option<PathBuf>,
    pub started_at: Option<DateTime<Utc>>,
    pub username: Option<String>,
    pub bundle_id: Option<String>,
    pub team_id: Option<String>,
}

impl Process {
    pub fn has_termination_identity(&self) -> bool {
        !self.pid.is_dangerous()
            && self
                .started_at
                .is_some_and(|started_at| started_at.timestamp() > 0)
            && self.username.as_ref().is_some_and(|user| !user.is_empty())
            && (self.executable.is_some() || self.bundle_id.is_some() || self.team_id.is_some())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    processes: HashMap<Pid, Process>,
}

impl Snapshot {
    pub fn new(processes: impl IntoIterator<Item = Process>) -> Self {
        Self {
            processes: processes
                .into_iter()
                .map(|process| (process.pid, process))
                .collect(),
        }
    }

    pub fn process(&self, pid: Pid) -> Option<&Process> {
        self.processes.get(&pid)
    }

    pub fn processes(&self) -> impl Iterator<Item = &Process> {
        self.processes.values()
    }

    pub fn termination_target(&self, expected: &Process) -> Option<TerminationTarget> {
        target::termination_target(self, expected)
    }

    /// Walk up from a leaf worker to the nearest ancestor of the same process
    /// family — the supervisor that respawns the leaf — and target its whole
    /// tree. Used for opt-in escalation against supervised desktop workers
    /// (killing the leaf alone is futile; the supervisor restarts it). Returns
    /// `None` when no same-family ancestor exists (e.g. a plain CLI worker).
    pub fn supervisor_target(
        &self,
        leaf: &Process,
        family_names: &[String],
    ) -> Option<TerminationTarget> {
        target::supervisor_target(self, leaf, family_names)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminationResult {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub soft_signaled: Vec<i32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hard_signaled: Vec<i32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gone: Vec<i32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Error)]
pub enum PlatformError {
    #[error("process capture failed: {0}")]
    Capture(String),
    #[error("notification failed: {0}")]
    Notify(String),
    #[error("termination failed: {0}")]
    Terminate(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NotificationCapability {
    pub supported: bool,
    pub status: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminationCapability {
    pub supported: bool,
    pub status: String,
    pub message: String,
}

pub trait Platform {
    fn capture(&self) -> Result<Snapshot, PlatformError>;
    fn notification_capability(&self) -> NotificationCapability;
    fn termination_capability(&self) -> TerminationCapability;
    fn notify(&self, title: &str, body: &str) -> Result<(), PlatformError>;
    fn terminate(&self, target: &TerminationTarget, grace: Duration) -> TerminationResult;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemPlatform;

impl Platform for SystemPlatform {
    fn capture(&self) -> Result<Snapshot, PlatformError> {
        let system = System::new_with_specifics(
            RefreshKind::nothing().with_processes(
                ProcessRefreshKind::nothing()
                    .with_user(UpdateKind::OnlyIfNotSet)
                    .with_cwd(UpdateKind::OnlyIfNotSet)
                    .with_cmd(UpdateKind::OnlyIfNotSet)
                    .with_exe(UpdateKind::OnlyIfNotSet),
            ),
        );
        let users = Users::new_with_refreshed_list();
        let processes = system
            .processes()
            .values()
            .filter_map(|process| capture::observed_process(process, &users))
            .collect::<Vec<_>>();
        Ok(Snapshot::new(processes))
    }

    fn notification_capability(&self) -> NotificationCapability {
        notification::capability_for(std::env::consts::OS, notification::command_exists)
    }

    fn termination_capability(&self) -> TerminationCapability {
        TerminationCapability {
            supported: true,
            status: "available".to_string(),
            message: "process-tree termination is available".to_string(),
        }
    }

    fn notify(&self, title: &str, body: &str) -> Result<(), PlatformError> {
        notification::run(notification::command(std::env::consts::OS, title, body)?)
    }

    fn terminate(&self, target: &TerminationTarget, grace: Duration) -> TerminationResult {
        termination::terminate_tree(target, grace)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CommandSpec {
    program: String,
    args: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyPlatform;

impl Platform for EmptyPlatform {
    fn capture(&self) -> Result<Snapshot, PlatformError> {
        Ok(Snapshot::default())
    }

    fn notification_capability(&self) -> NotificationCapability {
        NotificationCapability {
            supported: false,
            status: "unsupported".to_string(),
            message: "empty platform cannot deliver notifications".to_string(),
        }
    }

    fn termination_capability(&self) -> TerminationCapability {
        TerminationCapability {
            supported: false,
            status: "unsupported".to_string(),
            message: "empty platform cannot terminate processes".to_string(),
        }
    }

    fn notify(&self, _title: &str, _body: &str) -> Result<(), PlatformError> {
        Err(PlatformError::Notify(
            "empty platform cannot deliver notifications".to_string(),
        ))
    }

    fn terminate(&self, _target: &TerminationTarget, _grace: Duration) -> TerminationResult {
        TerminationResult {
            errors: vec!["empty platform cannot terminate processes".to_string()],
            ..TerminationResult::default()
        }
    }
}
