use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

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
        let mut current = leaf;
        let mut seen = HashSet::new();
        seen.insert(current.pid);
        while let Some(ppid) = current.ppid {
            if !seen.insert(ppid) {
                break;
            }
            let parent = self.processes.get(&ppid)?;
            if family_names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&parent.name))
            {
                return self.termination_target(parent);
            }
            current = parent;
        }
        None
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
            .filter_map(|process| observed_process(process, &users))
            .collect::<Vec<_>>();
        Ok(Snapshot::new(processes))
    }

    fn notification_capability(&self) -> NotificationCapability {
        notification_capability_for(std::env::consts::OS, command_exists)
    }

    fn termination_capability(&self) -> TerminationCapability {
        TerminationCapability {
            supported: true,
            status: "available".to_string(),
            message: "process-tree termination is available".to_string(),
        }
    }

    fn notify(&self, title: &str, body: &str) -> Result<(), PlatformError> {
        run_notification(notification_command(std::env::consts::OS, title, body)?)
    }

    fn terminate(&self, target: &TerminationTarget, grace: Duration) -> TerminationResult {
        terminate_tree(target, grace)
    }
}

fn terminate_tree(target: &TerminationTarget, grace: Duration) -> TerminationResult {
    let mut pids = target
        .scope()
        .iter()
        .map(|pid| pid.get())
        .collect::<Vec<_>>();
    if pids.is_empty() || target.root().pid.get() == 0 {
        return TerminationResult {
            errors: vec!["empty termination target".to_string()],
            ..TerminationResult::default()
        };
    }
    pids.sort_by(|left, right| right.cmp(left));

    let mut result = TerminationResult::default();
    for pid in &pids {
        match soft_terminate(*pid) {
            Ok(()) => result.soft_signaled.push(*pid),
            Err(_error) if !pid_alive(*pid) => result.gone.push(*pid),
            Err(error) => result.errors.push(format!("soft pid {pid}: {error}")),
        }
    }

    std::thread::sleep(grace);

    for pid in pids {
        if !pid_alive(pid) {
            if !result.gone.contains(&pid) {
                result.gone.push(pid);
            }
            continue;
        }
        match hard_terminate(pid) {
            Ok(()) => result.hard_signaled.push(pid),
            Err(error) => result.errors.push(format!("hard pid {pid}: {error}")),
        }
    }
    result
}

#[cfg(unix)]
fn soft_terminate(pid: i32) -> Result<(), String> {
    signal_with_kill("-TERM", pid)
}

#[cfg(unix)]
fn hard_terminate(pid: i32) -> Result<(), String> {
    signal_with_kill("-KILL", pid)
}

#[cfg(unix)]
fn signal_with_kill(signal: &str, pid: i32) -> Result<(), String> {
    let status = Command::new("kill")
        .args([signal, &pid.to_string()])
        .status()
        .map_err(|source| source.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("kill {signal} {pid} exited with {status}"))
    }
}

#[cfg(windows)]
fn soft_terminate(pid: i32) -> Result<(), String> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T"])
        .status()
        .map_err(|source| source.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("taskkill /PID {pid} /T exited with {status}"))
    }
}

#[cfg(windows)]
fn hard_terminate(pid: i32) -> Result<(), String> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .map_err(|source| source.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("taskkill /PID {pid} /T /F exited with {status}"))
    }
}

#[cfg(not(any(unix, windows)))]
fn soft_terminate(pid: i32) -> Result<(), String> {
    Err(format!("soft termination unsupported for pid {pid}"))
}

#[cfg(not(any(unix, windows)))]
fn hard_terminate(pid: i32) -> Result<(), String> {
    Err(format!("hard termination unsupported for pid {pid}"))
}

fn pid_alive(pid: i32) -> bool {
    let Some(pid) = u32::try_from(pid).ok().map(sysinfo::Pid::from_u32) else {
        return false;
    };
    let system = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
    );
    system.process(pid).is_some()
}

fn notification_capability_for(os: &str, exists: impl Fn(&str) -> bool) -> NotificationCapability {
    match os {
        "macos" => {
            if exists("osascript") {
                NotificationCapability {
                    supported: true,
                    status: "available".to_string(),
                    message: "macOS user notifications available through osascript".to_string(),
                }
            } else {
                NotificationCapability {
                    supported: false,
                    status: "unavailable".to_string(),
                    message: "osascript not found".to_string(),
                }
            }
        }
        "linux" => {
            if exists("notify-send") {
                NotificationCapability {
                    supported: true,
                    status: "available".to_string(),
                    message: "Desktop notification command found".to_string(),
                }
            } else {
                NotificationCapability {
                    supported: false,
                    status: "unavailable".to_string(),
                    message: "notify-send not found".to_string(),
                }
            }
        }
        "windows" => NotificationCapability {
            supported: false,
            status: "unsupported".to_string(),
            message: "Windows toast notifications are not implemented".to_string(),
        },
        other => NotificationCapability {
            supported: false,
            status: "unsupported".to_string(),
            message: format!("notifications unsupported on {other}"),
        },
    }
}

fn notification_command(os: &str, title: &str, body: &str) -> Result<CommandSpec, PlatformError> {
    match os {
        "macos" => Ok(CommandSpec {
            program: "osascript".to_string(),
            args: vec![
                "-e".to_string(),
                format!(
                    "display notification {} with title {}",
                    apple_script_string(body),
                    apple_script_string(title)
                ),
            ],
        }),
        "linux" => Ok(CommandSpec {
            program: "notify-send".to_string(),
            args: vec![title.to_string(), body.to_string()],
        }),
        "windows" => Err(PlatformError::Notify(
            "Windows toast notifications are not implemented".to_string(),
        )),
        other => Err(PlatformError::Notify(format!(
            "notifications unsupported on {other}"
        ))),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CommandSpec {
    program: String,
    args: Vec<String>,
}

fn run_notification(spec: CommandSpec) -> Result<(), PlatformError> {
    let status = Command::new(&spec.program)
        .args(&spec.args)
        .status()
        .map_err(|source| PlatformError::Notify(source.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(PlatformError::Notify(format!(
            "{} exited with {status}",
            spec.program
        )))
    }
}

fn command_exists(program: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(program).is_file())
}

fn apple_script_string(value: &str) -> String {
    let escaped = value
        .chars()
        .flat_map(|ch| match ch {
            '"' | '\\' => vec!['\\', ch],
            _ => vec![ch],
        })
        .collect::<String>();
    format!("\"{escaped}\"")
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

    #[test]
    fn system_platform_terminates_a_live_child_process() {
        let mut child = sleeping_child();
        let pid = Pid::new(i32::try_from(child.id()).expect("child pid fits i32"));
        let snapshot = SystemPlatform.capture().expect("capture");
        let observed = snapshot.process(pid).expect("child process in snapshot");
        let target = snapshot
            .termination_target(observed)
            .expect("termination identity");

        let result = SystemPlatform.terminate(&target, std::time::Duration::from_millis(50));
        let _ = child.wait();

        assert!(
            result.soft_signaled.contains(&pid.get())
                || result.hard_signaled.contains(&pid.get())
                || result.gone.contains(&pid.get()),
            "termination result = {result:?}"
        );
    }

    #[test]
    fn notification_capability_reports_platform_support() {
        assert_eq!(
            notification_capability_for("macos", always_exists).status,
            "available"
        );
        assert_eq!(
            notification_capability_for("linux", never_exists).status,
            "unavailable"
        );
        assert_eq!(
            notification_capability_for("windows", always_exists).status,
            "unsupported"
        );
    }

    #[test]
    fn notification_command_uses_argument_boundaries_and_escapes_applescript() {
        let linux = notification_command("linux", "Curb title", "body; rm -rf /").unwrap();
        assert_eq!(linux.program, "notify-send");
        assert_eq!(linux.args, vec!["Curb title", "body; rm -rf /"]);

        let macos = notification_command("macos", "Curb \"title\"", "body \\ text").unwrap();
        assert_eq!(macos.program, "osascript");
        assert_eq!(
            macos.args,
            vec![
                "-e".to_string(),
                "display notification \"body \\\\ text\" with title \"Curb \\\"title\\\"\""
                    .to_string()
            ]
        );
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

    fn always_exists(_: &str) -> bool {
        true
    }

    fn never_exists(_: &str) -> bool {
        false
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

    mod seal_properties {
        use super::*;
        use proptest::prelude::*;

        /// Which of the three optional identity facets a generated
        /// complete-identity process carries. At least one is always present so
        /// the base process satisfies `has_termination_identity`.
        #[derive(Clone, Copy, Debug)]
        struct FacetPresence {
            executable: bool,
            bundle_id: bool,
            team_id: bool,
        }

        /// The compared facets the seal checks; used to drive the single-facet
        /// mismatch property over exactly the fields `same_process_identity`
        /// inspects.
        #[derive(Clone, Copy, Debug)]
        enum ComparedFacet {
            Pid,
            StartedAt,
            Username,
            Executable,
            BundleId,
            TeamId,
        }

        /// A pid that is never dangerous: in `2..=1_000_000` and never equal to
        /// the running test process. Excluding `std::process::id()` keeps the
        /// strategy deterministic across runs even though the pid varies.
        fn safe_pid() -> impl Strategy<Value = i32> {
            (2i32..=1_000_000).prop_filter("pid must not equal the test process", |pid| {
                *pid != std::process::id() as i32
            })
        }

        /// A start time strictly after the Unix epoch, so
        /// `started_at.timestamp() > 0`.
        fn live_start_time() -> impl Strategy<Value = DateTime<Utc>> {
            (1i64..=4_000_000_000).prop_map(|secs| Utc.timestamp_opt(secs, 0).single().unwrap())
        }

        /// At least one of the three optional facets present, in every
        /// combination.
        fn facet_presence() -> impl Strategy<Value = FacetPresence> {
            (any::<bool>(), any::<bool>(), any::<bool>())
                .prop_map(|(executable, bundle_id, team_id)| FacetPresence {
                    executable,
                    bundle_id,
                    team_id,
                })
                .prop_filter("at least one optional facet must be present", |facets| {
                    facets.executable || facets.bundle_id || facets.team_id
                })
        }

        /// Build a complete-identity `Process` from generated parts. The result
        /// always satisfies `has_termination_identity`.
        fn complete_identity(
            pid: i32,
            started_at: DateTime<Utc>,
            username: String,
            facets: FacetPresence,
            exe: String,
            bundle: String,
            team: String,
        ) -> Process {
            Process {
                pid: Pid::new(pid),
                ppid: None,
                name: "agent".to_string(),
                executable: facets.executable.then(|| PathBuf::from(exe)),
                command: "agent run".to_string(),
                cwd: Some(PathBuf::from("/repo")),
                started_at: Some(started_at),
                username: Some(username),
                bundle_id: facets.bundle_id.then_some(bundle),
                team_id: facets.team_id.then_some(team),
            }
        }

        /// Strategy yielding a complete-identity `Process` plus the set of
        /// compared facets that apply to it (pid/started_at/username always;
        /// the optional facets only when present).
        fn complete_identity_process() -> impl Strategy<Value = (Process, Vec<ComparedFacet>)> {
            (
                safe_pid(),
                live_start_time(),
                "[a-z][a-z0-9]{0,15}",
                facet_presence(),
                "/usr/local/bin/[a-z]{1,12}",
                "com\\.[a-z]{1,8}\\.[a-z]{1,8}",
                "[A-Z0-9]{6,10}",
            )
                .prop_map(|(pid, started_at, username, facets, exe, bundle, team)| {
                    let process =
                        complete_identity(pid, started_at, username, facets, exe, bundle, team);
                    let mut compared = vec![
                        ComparedFacet::Pid,
                        ComparedFacet::StartedAt,
                        ComparedFacet::Username,
                    ];
                    if facets.executable {
                        compared.push(ComparedFacet::Executable);
                    }
                    if facets.bundle_id {
                        compared.push(ComparedFacet::BundleId);
                    }
                    if facets.team_id {
                        compared.push(ComparedFacet::TeamId);
                    }
                    (process, compared)
                })
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(256))]

            /// Reflexive: a complete-identity process seals against an identical
            /// clone, both at the `same_process_identity` predicate and through
            /// the public `Snapshot::termination_target` entry point.
            #[test]
            fn complete_identity_seals_against_itself((process, _compared) in complete_identity_process()) {
                prop_assert!(process.has_termination_identity());
                prop_assert!(same_process_identity(&process, &process));
                prop_assert!(
                    Snapshot::new([process.clone()])
                        .termination_target(&process)
                        .is_some()
                );
            }

            /// Single-facet mismatch rejects: mutating exactly one compared
            /// facet on `current` so it differs from `expected` must break the
            /// seal. Only facets the seal inspects are mutated (pid/started_at/
            /// username always; executable/bundle_id/team_id only when present
            /// on `expected`).
            #[test]
            fn single_compared_facet_mismatch_rejects(
                (expected, compared) in complete_identity_process(),
                facet_index in any::<prop::sample::Index>(),
            ) {
                let facet = compared[facet_index.index(compared.len())];
                let mut current = expected.clone();
                match facet {
                    ComparedFacet::Pid => {
                        let next = if expected.pid.get() == 1_000_000 {
                            2
                        } else {
                            expected.pid.get() + 1
                        };
                        let next = if next == std::process::id() as i32 {
                            next + 1
                        } else {
                            next
                        };
                        current.pid = Pid::new(next);
                    }
                    ComparedFacet::StartedAt => {
                        let secs = expected.started_at.unwrap().timestamp();
                        current.started_at = Some(Utc.timestamp_opt(secs + 1, 0).single().unwrap());
                    }
                    ComparedFacet::Username => {
                        current.username =
                            Some(format!("{}x", expected.username.as_ref().unwrap()));
                    }
                    ComparedFacet::Executable => {
                        current.executable = Some(PathBuf::from("/usr/local/bin/imposter"));
                    }
                    ComparedFacet::BundleId => {
                        current.bundle_id = Some("com.imposter.fake".to_string());
                    }
                    ComparedFacet::TeamId => {
                        current.team_id = Some("IMPOSTER".to_string());
                    }
                }

                prop_assert!(!same_process_identity(&expected, &current));
                // The public entry keys on pid; a pid mismatch means no process
                // at expected.pid, the rest exercise the identity rejection.
                prop_assert!(
                    Snapshot::new([current])
                        .termination_target(&expected)
                        .is_none()
                );
            }

            /// Incomplete identity rejects: clearing started_at, emptying the
            /// username, or removing all three optional facets on either side of
            /// the comparison must break the seal.
            #[test]
            fn incomplete_identity_rejects(
                (process, _compared) in complete_identity_process(),
                degrade in 0u8..3,
                degrade_side_expected in any::<bool>(),
            ) {
                let mut expected = process.clone();
                let mut current = process;
                let target = if degrade_side_expected {
                    &mut expected
                } else {
                    &mut current
                };
                match degrade {
                    0 => target.started_at = None,
                    1 => target.username = Some(String::new()),
                    _ => {
                        target.executable = None;
                        target.bundle_id = None;
                        target.team_id = None;
                    }
                }

                prop_assert!(!same_process_identity(&expected, &current));
                prop_assert!(
                    Snapshot::new([current])
                        .termination_target(&expected)
                        .is_none()
                );
            }
        }
    }
}
