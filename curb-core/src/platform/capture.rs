use std::ffi::OsString;
use std::path::PathBuf;

use chrono::{DateTime, TimeZone, Utc};
use sysinfo::{ProcessStatus, Users};

use super::{Pid, Process};

pub(super) fn observed_process(process: &sysinfo::Process, users: &Users) -> Option<Process> {
    if !is_live_process_status(process.status()) {
        return None;
    }
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

pub(super) fn is_live_process_status(status: ProcessStatus) -> bool {
    !matches!(status, ProcessStatus::Zombie | ProcessStatus::Dead)
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
