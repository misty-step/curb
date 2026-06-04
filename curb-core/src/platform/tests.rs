use chrono::TimeZone;
use sysinfo::ProcessStatus;

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
fn supervisor_target_uses_nearest_same_family_ancestor_tree() {
    let root = process_named(42, None, "Codex");
    let middle = process_named(43, Some(Pid::new(42)), "shell");
    let leaf = process_named(44, Some(Pid::new(43)), "codex");
    let sibling = process_named(45, Some(Pid::new(42)), "worker");
    let snapshot = Snapshot::new([root, middle, leaf.clone(), sibling]);

    let target = snapshot
        .supervisor_target(&leaf, &["codex".to_string()])
        .expect("supervisor target");

    assert_eq!(target.root().pid, Pid::new(42));
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
        !observed.name.is_empty() || observed.executable.is_some() || !observed.command.is_empty()
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
        notification::capability_for("macos", always_exists).status,
        "available"
    );
    assert_eq!(
        notification::capability_for("linux", never_exists).status,
        "unavailable"
    );
    assert_eq!(
        notification::capability_for("windows", always_exists).status,
        "unsupported"
    );
}

#[test]
fn notification_command_uses_argument_boundaries_and_escapes_applescript() {
    let linux = notification::command("linux", "Curb title", "body; rm -rf /").unwrap();
    assert_eq!(linux.program, "notify-send");
    assert_eq!(linux.args, vec!["Curb title", "body; rm -rf /"]);

    let macos = notification::command("macos", "Curb \"title\"", "body \\ text").unwrap();
    assert_eq!(macos.program, "osascript");
    assert_eq!(
        macos.args,
        vec![
            "-e".to_string(),
            "display notification \"body \\\\ text\" with title \"Curb \\\"title\\\"\"".to_string()
        ]
    );
}

#[test]
fn process_liveness_excludes_exited_unreaped_statuses() {
    assert!(!capture::is_live_process_status(ProcessStatus::Zombie));
    assert!(!capture::is_live_process_status(ProcessStatus::Dead));
    assert!(capture::is_live_process_status(ProcessStatus::Run));
    assert!(capture::is_live_process_status(ProcessStatus::Sleep));
}

#[cfg(unix)]
#[test]
fn unix_termination_command_uses_absolute_kill_path() {
    let spec = termination::unix_signal_command("-TERM", 42);

    assert_eq!(spec.program, "/bin/kill");
    assert_eq!(spec.args, vec!["-TERM", "42"]);
}

#[cfg(windows)]
#[test]
fn windows_termination_command_uses_absolute_taskkill_path() {
    let soft = termination::windows_taskkill_command(42, false);
    assert_eq!(soft.program, r"C:\Windows\System32\taskkill.exe");
    assert_eq!(soft.args, vec!["/PID", "42", "/T"]);

    let hard = termination::windows_taskkill_command(42, true);
    assert_eq!(hard.program, r"C:\Windows\System32\taskkill.exe");
    assert_eq!(hard.args, vec!["/PID", "42", "/T", "/F"]);
}

fn process(pid: i32, ppid: Option<Pid>) -> Process {
    process_named(pid, ppid, "agent")
}

fn process_named(pid: i32, ppid: Option<Pid>, name: &str) -> Process {
    Process {
        pid: Pid::new(pid),
        ppid,
        name: name.to_string(),
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
            prop_assert!(target::same_process_identity(&process, &process));
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

            prop_assert!(!target::same_process_identity(&expected, &current));
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

            prop_assert!(!target::same_process_identity(&expected, &current));
            prop_assert!(
                Snapshot::new([current])
                    .termination_target(&expected)
                    .is_none()
            );
        }
    }
}
