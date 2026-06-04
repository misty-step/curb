use std::process::Command;
use std::time::Duration;

use sysinfo::{ProcessRefreshKind, RefreshKind, System};

use super::{CommandSpec, TerminationResult, TerminationTarget, capture};

pub(super) fn terminate_tree(target: &TerminationTarget, grace: Duration) -> TerminationResult {
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
    let spec = unix_signal_command(signal, pid);
    let status = Command::new(&spec.program)
        .args(&spec.args)
        .status()
        .map_err(|source| source.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("kill {signal} {pid} exited with {status}"))
    }
}

#[cfg(unix)]
pub(super) fn unix_signal_command(signal: &str, pid: i32) -> CommandSpec {
    CommandSpec {
        program: "/bin/kill".to_string(),
        args: vec![signal.to_string(), pid.to_string()],
    }
}

#[cfg(windows)]
fn soft_terminate(pid: i32) -> Result<(), String> {
    let spec = windows_taskkill_command(pid, false);
    let status = Command::new(&spec.program)
        .args(&spec.args)
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
    let spec = windows_taskkill_command(pid, true);
    let status = Command::new(&spec.program)
        .args(&spec.args)
        .status()
        .map_err(|source| source.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("taskkill /PID {pid} /T /F exited with {status}"))
    }
}

#[cfg(windows)]
pub(super) fn windows_taskkill_command(pid: i32, force: bool) -> CommandSpec {
    let mut args = vec!["/PID".to_string(), pid.to_string(), "/T".to_string()];
    if force {
        args.push("/F".to_string());
    }
    CommandSpec {
        program: r"C:\Windows\System32\taskkill.exe".to_string(),
        args,
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
    // Keep retry behavior aligned with capture filtering: exited/zombie rows are
    // treated as gone in both snapshots and termination follow-up checks.
    system
        .process(pid)
        .is_some_and(|process| capture::is_live_process_status(process.status()))
}
