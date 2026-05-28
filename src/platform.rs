use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::PathBuf;

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind, Users};
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
        let mut children = self
            .processes
            .values()
            .filter(|process| process.ppid == Some(pid))
            .collect::<Vec<_>>();
        children.sort_by_key(|process| process.pid.get());
        for child in children {
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
        && matching_expected_identity(expected, current)
}

fn matching_expected_identity(expected: &Process, current: &Process) -> bool {
    let mut matched = false;
    if let Some(executable) = &expected.executable {
        if current.executable.as_ref() != Some(executable) {
            return false;
        }
        matched = true;
    }
    if let Some(bundle_id) = &expected.bundle_id {
        if current.bundle_id.as_ref() != Some(bundle_id) {
            return false;
        }
        matched = true;
    }
    if let Some(team_id) = &expected.team_id {
        if current.team_id.as_ref() != Some(team_id) {
            return false;
        }
        matched = true;
    }
    matched
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
            .filter_map(|process| observed_process(process, &users))
            .collect::<Vec<_>>();
        Ok(Snapshot::new(processes))
    }

    fn notify(&self, _title: &str, _body: &str) -> Result<(), PlatformError> {
        Ok(())
    }

    fn terminate(&self, _target: &TerminationTarget) -> Result<(), PlatformError> {
        Err(PlatformError::Terminate(
            "rust live platform has not ported termination yet".to_string(),
        ))
    }
}

fn observed_process(process: &sysinfo::Process, users: &Users) -> Option<Process> {
    let pid = convert_pid(process.pid())?;
    let ppid = process.parent().and_then(convert_pid);
    let name = process.name().to_string_lossy().into_owned();
    let executable = non_empty_path(process.exe());
    let command = command_line(process.cmd());
    let cwd = non_empty_path(process.cwd());
    let started_at = timestamp(process.start_time());
    let username = process
        .user_id()
        .and_then(|user_id| users.get_user_by_id(user_id))
        .map(|user| user.name().to_string())
        .filter(|name| !name.is_empty());

    let out = Process {
        pid,
        ppid,
        name,
        executable,
        command,
        cwd,
        started_at,
        username,
        bundle_id: None,
        team_id: None,
    };
    if out.name.is_empty()
        && out.executable.is_none()
        && out.command.is_empty()
        && out.started_at.is_none()
    {
        None
    } else {
        Some(out)
    }
}

fn convert_pid(pid: sysinfo::Pid) -> Option<Pid> {
    i32::try_from(pid.as_u32()).ok().map(Pid::new)
}

fn non_empty_path(path: Option<&std::path::Path>) -> Option<PathBuf> {
    path.filter(|path| !path.as_os_str().is_empty())
        .map(PathBuf::from)
}

fn command_line(command: &[OsString]) -> String {
    command
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

fn timestamp(seconds: u64) -> Option<DateTime<Utc>> {
    if seconds == 0 {
        return None;
    }
    Utc.timestamp_opt(seconds as i64, 0).single()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyPlatform;

impl Platform for EmptyPlatform {
    fn capture(&self) -> Result<Snapshot, PlatformError> {
        Ok(Snapshot::default())
    }

    fn notify(&self, _title: &str, _body: &str) -> Result<(), PlatformError> {
        Ok(())
    }

    fn terminate(&self, _target: &TerminationTarget) -> Result<(), PlatformError> {
        Err(PlatformError::Terminate(
            "empty platform cannot terminate processes".to_string(),
        ))
    }
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
    fn termination_rejects_epoch_start_time() {
        let mut process = process(42, None);
        process.started_at = Some(Utc.timestamp_opt(0, 0).unwrap());

        assert!(
            Snapshot::new([process.clone()])
                .termination_target(&process)
                .is_none()
        );
    }

    #[test]
    fn termination_allows_current_process_to_have_extra_identity_fields() {
        let expected = process(42, None);
        let mut current = expected.clone();
        current.bundle_id = Some("com.example.agent".to_string());
        current.team_id = Some("TEAMID".to_string());

        assert!(
            Snapshot::new([current])
                .termination_target(&expected)
                .is_some()
        );
    }

    #[test]
    fn termination_rejects_changed_expected_identity_fields() {
        let expected = process(42, None);
        let mut current = expected.clone();
        current.executable = Some(PathBuf::from("/usr/local/bin/other-agent"));

        assert!(
            Snapshot::new([current])
                .termination_target(&expected)
                .is_none()
        );
    }

    #[test]
    fn termination_scope_is_child_first() {
        let root = process(42, None);
        let child = process(45, Some(Pid::new(42)));
        let sibling = process(43, Some(Pid::new(42)));
        let grandchild = process(44, Some(Pid::new(43)));

        let target = Snapshot::new([root.clone(), child, sibling, grandchild])
            .termination_target(&root)
            .expect("target");

        assert_eq!(
            target.scope(),
            &[Pid::new(44), Pid::new(43), Pid::new(45), Pid::new(42)]
        );
    }

    #[test]
    fn system_platform_captures_a_live_child_process() {
        let mut child = sleeping_child();
        let pid = Pid::new(i32::try_from(child.id()).expect("child pid fits i32"));
        let snapshot = SystemPlatform.capture().expect("capture");
        let observed = snapshot.process(pid).expect("child process in snapshot");

        assert_eq!(observed.pid, pid);
        assert!(observed.started_at.is_some());
        assert!(
            !observed.name.is_empty()
                || observed.executable.is_some()
                || !observed.command.is_empty()
        );

        child.kill().ok();
        child.wait().ok();
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

    #[cfg(unix)]
    fn sleeping_child() -> std::process::Child {
        std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 5")
            .spawn()
            .expect("spawn sleep child")
    }

    #[cfg(windows)]
    fn sleeping_child() -> std::process::Child {
        std::process::Command::new("cmd")
            .args(["/C", "ping -n 6 127.0.0.1 >NUL"])
            .spawn()
            .expect("spawn sleep child")
    }
}
