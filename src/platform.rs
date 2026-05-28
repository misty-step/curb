use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

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
            && self.started_at.is_some()
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
        let current = self.processes.get(&expected.pid)?;
        if !same_process_identity(expected, current) {
            return None;
        }
        let scope = self.process_tree(expected.pid);
        Some(TerminationTarget {
            root: current.clone(),
            scope,
        })
    }

    fn process_tree(&self, root: Pid) -> Vec<Pid> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        self.collect_process_tree(root, &mut seen, &mut out);
        out
    }

    fn collect_process_tree(&self, pid: Pid, seen: &mut HashSet<Pid>, out: &mut Vec<Pid>) {
        if !seen.insert(pid) {
            return;
        }
        for child in self
            .processes
            .values()
            .filter(|process| process.ppid == Some(pid))
        {
            self.collect_process_tree(child.pid, seen, out);
        }
        out.push(pid);
    }
}

fn same_process_identity(expected: &Process, current: &Process) -> bool {
    expected.has_termination_identity()
        && current.has_termination_identity()
        && expected.pid == current.pid
        && expected.started_at == current.started_at
        && expected.username == current.username
        && expected.executable == current.executable
        && expected.bundle_id == current.bundle_id
        && expected.team_id == current.team_id
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminationTarget {
    root: Process,
    scope: Vec<Pid>,
}

impl TerminationTarget {
    pub fn root(&self) -> &Process {
        &self.root
    }

    pub fn scope(&self) -> &[Pid] {
        &self.scope
    }
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

pub trait Platform {
    fn capture(&self) -> Result<Snapshot, PlatformError>;
    fn notify(&self, title: &str, body: &str) -> Result<(), PlatformError>;
    fn terminate(&self, target: &TerminationTarget) -> Result<(), PlatformError>;
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn termination_requires_pid_start_owner_and_executable_identity() {
        let process = process(42, None);
        let target = Snapshot::new([process.clone()])
            .termination_target(&process)
            .expect("target");

        assert_eq!(target.root().pid, Pid::new(42));
        assert_eq!(target.scope(), &[Pid::new(42)]);
    }

    #[test]
    fn termination_rejects_reused_pid_with_different_start_time() {
        let expected = process(42, None);
        let mut current = expected.clone();
        current.started_at = Some(Utc.with_ymd_and_hms(2026, 5, 28, 15, 0, 1).unwrap());

        assert!(
            Snapshot::new([current])
                .termination_target(&expected)
                .is_none()
        );
    }

    #[test]
    fn termination_rejects_missing_owner_or_executable_evidence() {
        let mut missing_owner = process(42, None);
        missing_owner.username = None;
        assert!(
            Snapshot::new([missing_owner.clone()])
                .termination_target(&missing_owner)
                .is_none()
        );

        let mut missing_executable = process(43, None);
        missing_executable.executable = None;
        missing_executable.bundle_id = None;
        missing_executable.team_id = None;
        assert!(
            Snapshot::new([missing_executable.clone()])
                .termination_target(&missing_executable)
                .is_none()
        );
    }

    #[test]
    fn termination_scope_is_child_first() {
        let root = process(42, None);
        let child = process(43, Some(Pid::new(42)));
        let grandchild = process(44, Some(Pid::new(43)));

        let target = Snapshot::new([root.clone(), child, grandchild])
            .termination_target(&root)
            .expect("target");

        assert_eq!(target.scope(), &[Pid::new(44), Pid::new(43), Pid::new(42)]);
    }

    fn process(pid: i32, ppid: Option<Pid>) -> Process {
        Process {
            pid: Pid::new(pid),
            ppid,
            name: "agent".to_string(),
            executable: Some(PathBuf::from("/usr/local/bin/agent")),
            command: "agent run".to_string(),
            cwd: Some(PathBuf::from("/repo")),
            started_at: Some(Utc.with_ymd_and_hms(2026, 5, 28, 15, 0, 0).unwrap()),
            username: Some("tester".to_string()),
            bundle_id: None,
            team_id: None,
        }
    }
}
