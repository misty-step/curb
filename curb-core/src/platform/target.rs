use std::collections::HashSet;

use super::{Pid, Process, Snapshot};

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

pub(super) fn termination_target(
    snapshot: &Snapshot,
    expected: &Process,
) -> Option<TerminationTarget> {
    let current = snapshot.processes.get(&expected.pid)?;
    if !same_process_identity(expected, current) {
        return None;
    }
    Some(TerminationTarget {
        root: current.clone(),
        scope: process_tree(snapshot, expected.pid),
    })
}

pub(super) fn supervisor_target(
    snapshot: &Snapshot,
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
        let parent = snapshot.processes.get(&ppid)?;
        if family_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&parent.name))
        {
            return termination_target(snapshot, parent);
        }
        current = parent;
    }
    None
}

pub(super) fn same_process_identity(expected: &Process, current: &Process) -> bool {
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

fn process_tree(snapshot: &Snapshot, root: Pid) -> Vec<Pid> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    collect_process_tree(snapshot, root, &mut seen, &mut out);
    out
}

fn collect_process_tree(
    snapshot: &Snapshot,
    pid: Pid,
    seen: &mut HashSet<Pid>,
    out: &mut Vec<Pid>,
) {
    if !seen.insert(pid) {
        return;
    }
    let mut children = snapshot
        .processes
        .values()
        .filter(|process| process.ppid == Some(pid))
        .collect::<Vec<_>>();
    children.sort_by_key(|process| process.pid.get());
    for child in children {
        collect_process_tree(snapshot, child.pid, seen, out);
    }
    out.push(pid);
}
