//! End-to-end enforcement against real spawned subprocesses.
//!
//! These tests drive the real decision engine (`UsageWatch::scan`) against the
//! real `SystemPlatform` — real process capture, the real identity seal, and the
//! real OS kill primitive — over genuinely spawned worker processes. They prove
//! Curb kills the exact correlated worker, spares a plausible app-root-like
//! sibling, records the grace -> termination_started -> termination_completed
//! lifecycle in the ledger, and does not re-kill a session it already terminated.
//!
//! Unix only: verified on macOS locally and on GitHub's macOS runner; Linux
//! coverage is restored by accepting Ubuntu's `/bin/sh` process name (`dash`) in
//! the synthetic-worker matcher. Windows is out of scope (different kill
//! primitive and process-tree semantics).
#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use curb_core::config::{Agent, AgentKind, Config, HumanDuration, Match, Mode};
use curb_core::ledger;
use curb_core::local_enforcer::{self, LocalEnforcer};
use curb_core::platform::{Pid, Platform, Process, Snapshot, SystemPlatform};
use curb_core::usage::{Event as UsageEvent, EventKind};
use curb_core::usagewatch::{PolicySession, UsageWatch};
use tempfile::TempDir;

/// Monotonic disambiguator so concurrently-running tests in the same binary
/// never share a marker even when spawned in the same nanosecond.
static SEQ: AtomicU64 = AtomicU64::new(0);

/// A unique-per-spawn marker that is greppable as `curb-e2e` (so CI can assert
/// no leaks) and can never collide with a real process on the machine. Embedded
/// in the worker's command line and required verbatim by the matcher.
fn unique_marker(role: &str) -> String {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Only `[A-Za-z0-9-]` so the marker is a literal regex fragment.
    format!("curb-e2e-{role}-{}-{seq}-{nanos}", std::process::id())
}

/// A real spawned long-lived worker carrying a unique marker in its command
/// line. The `while` loop keeps the shell resident (so the marker survives in
/// the captured argv) and gives the process a real child to terminate, exercising
/// process-tree termination. Drop tears the whole tree down so nothing leaks even
/// on panic.
struct Worker {
    child: Child,
}

impl Worker {
    fn spawn(cwd: &std::path::Path, marker: &str) -> Self {
        let script = format!("while :; do sleep 1; done # {marker}");
        let child = Command::new("sh")
            .arg("-c")
            .arg(&script)
            .current_dir(cwd)
            .spawn()
            .expect("spawn worker");
        Self { child }
    }

    fn pid(&self) -> Pid {
        Pid::new(i32::try_from(self.child.id()).expect("worker pid fits i32"))
    }

    /// SIGKILL the whole tree (shell + inner `sleep`) so no `sleep` orphans
    /// survive, then reap the direct child.
    fn force_cleanup(&mut self) {
        if let Ok(snapshot) = SystemPlatform.capture()
            && let Some(process) = snapshot.process(self.pid())
            && let Some(target) = snapshot.termination_target(process)
        {
            SystemPlatform.terminate(&target, Duration::from_millis(0));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.force_cleanup();
    }
}

/// Poll the real process table until `pid` disappears or the deadline passes.
/// No fixed sleep — returns as soon as the kill lands.
fn wait_until_gone(pid: Pid, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let alive = SystemPlatform
            .capture()
            .map(|snapshot| snapshot.process(pid).is_some())
            .unwrap_or(false);
        if !alive {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn is_alive(pid: Pid) -> bool {
    SystemPlatform
        .capture()
        .map(|snapshot| snapshot.process(pid).is_some())
        .unwrap_or(false)
}

/// Capture the live snapshot and return a clone of the worker's `Process`,
/// retrying briefly so the test does not race the spawn settling into the table
/// before `/proc`/sysinfo exposes enough identity to seal a stop target.
fn observe(pid: Pid) -> (Snapshot, Process) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_seen = None;
    let mut previous_sealable: Option<Process> = None;
    loop {
        let snapshot = SystemPlatform.capture().expect("capture");
        if let Some(process) = snapshot.process(pid) {
            let process = process.clone();
            if snapshot.termination_target(&process).is_some() {
                if previous_sealable
                    .as_ref()
                    .is_some_and(|previous| stable_identity(previous, &process))
                {
                    return (snapshot, process);
                }
                previous_sealable = Some(process.clone());
            }
            last_seen = Some(process);
        }
        assert!(
            Instant::now() < deadline,
            "worker pid {} never exposed a revalidatable termination identity; last_seen={:?}",
            pid.get(),
            last_seen
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn stable_identity(previous: &Process, current: &Process) -> bool {
    previous.pid == current.pid
        && previous.started_at == current.started_at
        && previous.username == current.username
        && previous.executable == current.executable
        && previous.bundle_id == current.bundle_id
        && previous.team_id == current.team_id
}

/// An agent matcher that matches ONLY a worker carrying `marker` in its command
/// line. `require_command_regex` vetoes every process whose command lacks the
/// marker (so a sibling with a different marker scores 0), while the process-name
/// and command-regex signals lift the matched worker over the confidence floor.
/// This is the same shape as the production `codex-desktop-worker` matcher, which
/// keys on the `app-server` and `--listen stdio://` fragments in the command line.
fn marker_agent(marker: &str) -> Agent {
    Agent {
        id: format!("e2e-worker-{marker}"),
        label: "E2E Worker".to_string(),
        family: "codex".to_string(),
        kind: AgentKind::Process,
        // The marker is `[A-Za-z0-9-]` only, so it is already a literal regex
        // fragment (no escaping needed: `-` is not special outside a class).
        matcher: Match {
            process_names: vec!["bash".to_string(), "dash".to_string(), "sh".to_string()],
            command_regex: vec![marker.to_string()],
            require_command_regex: vec![marker.to_string()],
            ..Match::default()
        },
        policy: None,
    }
}

/// Enforcement config whose only agent matcher is `agent`, with a short grace so
/// the kill path is reachable inside a test. State and ledger live under `dir`.
fn enforcement_config(dir: &std::path::Path, agent: Agent, grace: HumanDuration) -> Config {
    let mut cfg = Config::local_default(Mode::Enforcement, dir.join("state"));
    cfg.agents = vec![agent];
    cfg.usage.enabled = Some(true);
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    cfg.usage.grace_period = grace;
    cfg.alerts.local_notifications = false; // never poke the OS notifier in CI
    cfg.ledger.path = dir.join("runs.ndjson");
    cfg.refresh_agent_policies();
    cfg.validate().expect("config valid");
    cfg
}

/// A synthetic over-the-kill-line usage checkpoint for a session that correlates
/// (provider `codex` + this `cwd`) to the worker process.
fn over_kill_event(now: DateTime<Utc>, cwd: &std::path::Path, spent: i64) -> UsageEvent {
    UsageEvent {
        kind: EventKind::TokenCheckpoint,
        provider: "codex".to_string(),
        source: "e2e".to_string(),
        source_path: PathBuf::from("e2e.jsonl"),
        session_id: Some("e2e-session".to_string()),
        turn_id: None,
        request_id: None,
        model: None,
        cwd: Some(cwd.to_path_buf()),
        timestamp: Some(now),
        input_tokens: spent,
        cached_input_tokens: 0,
        cache_creation_input_tokens: 0,
        output_tokens: 0,
        reasoning_output_tokens: 0,
        total_tokens: spent,
        spent_tokens: spent,
        cumulative_tokens: spent,
        model_context_window: 0,
    }
}

fn ledger_event_types(cfg: &Config) -> Vec<String> {
    ledger::read(&cfg.ledger.path)
        .expect("read ledger")
        .into_iter()
        .map(|event| event.event_type)
        .collect()
}

fn process_diag(pid: Pid, marker: &str) -> String {
    match SystemPlatform.capture() {
        Ok(snapshot) => match snapshot.process(pid) {
            Some(process) => {
                let target_scope = snapshot.termination_target(process).map(|target| {
                    target
                        .scope()
                        .iter()
                        .map(|pid| pid.get())
                        .collect::<Vec<_>>()
                });
                format!(
                    "pid={} alive=true name={:?} ppid={:?} cwd={:?} started_at={:?} user_present={} executable={:?} has_termination_identity={} target_scope={:?} command_has_marker={}",
                    pid.get(),
                    process.name,
                    process.ppid.map(|ppid| ppid.get()),
                    process.cwd,
                    process.started_at,
                    process
                        .username
                        .as_ref()
                        .is_some_and(|user| !user.is_empty()),
                    process.executable,
                    process.has_termination_identity(),
                    target_scope,
                    process.command.contains(marker),
                )
            }
            None => format!("pid={} alive=false", pid.get()),
        },
        Err(error) => format!("pid={} capture_error={error}", pid.get()),
    }
}

fn sessions_diag(sessions: &[PolicySession]) -> String {
    if sessions.is_empty() {
        return "sessions=[]".to_string();
    }
    let entries = sessions
        .iter()
        .map(|session| {
            format!(
                "{{key={} cwd={:?} last_usage={:?} window_spent={} matched={} can_terminate={} pid={:?} score={} reason={:?}}}",
                session.key,
                session.cwd,
                session.last_usage,
                session.window_spent_tokens,
                session.target.matched,
                session.target.can_terminate,
                session.target.pid,
                session.target.score,
                session.target.reason,
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("sessions=[{entries}]")
}

fn e2e_diag(
    cfg: &Config,
    watch: &UsageWatch,
    worker_pid: Pid,
    worker_marker: &str,
    sibling: Option<(Pid, &str)>,
    sessions: &[PolicySession],
) -> String {
    let mut lines = vec![
        format!("worker: {}", process_diag(worker_pid, worker_marker)),
        sessions_diag(sessions),
        format!("terminated_keys={:?}", watch.terminated_keys()),
        format!("ledger_path={}", cfg.ledger.path.display()),
        format!("ledger_events={:?}", ledger_event_types(cfg)),
    ];
    if let Some((pid, marker)) = sibling {
        lines.insert(1, format!("sibling: {}", process_diag(pid, marker)));
    }
    lines.join("\n")
}

/// Drive one policy scan through the local adapter against the real OS: build
/// the correlated policy inputs from the live snapshot, then evaluate them with
/// a `LocalEnforcer` that owns the sealed termination and the kill primitive.
fn local_scan(
    watch: &mut UsageWatch,
    cfg: &Config,
    events: &[UsageEvent],
    snapshot: &Snapshot,
    now: DateTime<Utc>,
) -> Vec<PolicySession> {
    let window_start = now - chrono::Duration::from_std(cfg.usage.window.as_std()).unwrap();
    let sessions =
        local_enforcer::build_policy_sessions(cfg, events, snapshot, now).expect("build sessions");
    let enforcer = LocalEnforcer::new(cfg, &SystemPlatform, snapshot);
    watch
        .scan(cfg, &sessions, &enforcer, window_start, now)
        .expect("scan");
    sessions
}

/// (a) Warn fires, grace elapses, then the exact correlated worker process is
/// terminated; (b) a plausible app-root-like SIBLING that shares cwd and process
/// name but carries a different command-line marker is NOT terminated; (c) the
/// ledger records grace -> termination_started -> termination_completed.
#[test]
fn enforcement_terminates_the_correlated_worker_and_spares_the_sibling() {
    let temp = TempDir::new().expect("tempdir");
    let cwd = temp.path().to_path_buf();

    let worker_marker = unique_marker("worker");
    let sibling_marker = unique_marker("sibling");
    // Spawn the sibling FIRST so it holds the lower pid. Correlation breaks score
    // ties by lowest pid, so a matcher that wrongly matched both at equal confidence
    // would correlate the session to the sibling and kill IT instead of the worker.
    // The command-line marker keeps the sibling off the worker's session two ways:
    // the require_command_regex veto AND the command_regex confidence delta both key
    // on it — exactly the production discriminator. Sharing cwd, process name, and
    // provider, the sibling is genuinely plausible to over-match.
    let mut sibling = Worker::spawn(&cwd, &sibling_marker);
    let mut worker = Worker::spawn(&cwd, &worker_marker);

    let worker_pid = worker.pid();
    let sibling_pid = sibling.pid();
    assert!(
        sibling_pid.get() < worker_pid.get(),
        "sibling pid {} should be lower than worker pid {} (spawned first)",
        sibling_pid.get(),
        worker_pid.get()
    );

    // Resolve the real, OS-normalized cwd from the captured process so the
    // synthetic session correlates by exact working directory.
    let (snapshot, worker_process) = observe(worker_pid);
    let session_cwd = worker_process.cwd.expect("worker has a captured cwd");
    // Sanity: both spawned in the same dir, so the sibling shares the cwd.
    let (_, sibling_process) = observe(sibling_pid);
    assert_eq!(
        sibling_process.cwd.as_ref(),
        Some(&session_cwd),
        "sibling must share the worker's cwd to be a real over-match risk"
    );

    let state = TempDir::new().expect("state dir");
    let cfg = enforcement_config(
        state.path(),
        marker_agent(&worker_marker),
        HumanDuration::seconds(1),
    );
    let mut watch = UsageWatch::default();

    // Scan 1: over the kill line — grace starts, nothing terminated yet.
    let now = Utc::now();
    let first_sessions = local_scan(
        &mut watch,
        &cfg,
        &[over_kill_event(now, &session_cwd, 250)],
        &snapshot,
        now,
    );
    assert!(
        is_alive(worker_pid),
        "worker must survive the grace scan\n{}",
        e2e_diag(
            &cfg,
            &watch,
            worker_pid,
            &worker_marker,
            Some((sibling_pid, &sibling_marker)),
            &first_sessions,
        )
    );

    // Scan 2: grace has elapsed — terminate the worker tree.
    let after = now + chrono::Duration::seconds(2);
    let (snapshot, _) = observe(worker_pid);
    let second_sessions = local_scan(
        &mut watch,
        &cfg,
        &[over_kill_event(after, &session_cwd, 250)],
        &snapshot,
        after,
    );

    assert!(
        wait_until_gone(worker_pid, Duration::from_secs(10)),
        "the correlated worker pid {} was not terminated\n{}",
        worker_pid.get(),
        e2e_diag(
            &cfg,
            &watch,
            worker_pid,
            &worker_marker,
            Some((sibling_pid, &sibling_marker)),
            &second_sessions,
        )
    );
    // The adversarial assertion: the plausible sibling must still be alive.
    assert!(
        is_alive(sibling_pid),
        "the app-root-like sibling pid {} was wrongly terminated\n{}",
        sibling_pid.get(),
        e2e_diag(
            &cfg,
            &watch,
            worker_pid,
            &worker_marker,
            Some((sibling_pid, &sibling_marker)),
            &second_sessions,
        )
    );

    let events = ledger_event_types(&cfg);
    assert!(
        events
            == [
                "usage_warning",
                "usage_grace_started",
                "usage_termination_started",
                "usage_termination_completed",
            ],
        "unexpected ledger lifecycle: {events:?}\n{}",
        e2e_diag(
            &cfg,
            &watch,
            worker_pid,
            &worker_marker,
            Some((sibling_pid, &sibling_marker)),
            &second_sessions,
        )
    );

    worker.force_cleanup();
    sibling.force_cleanup();
}

/// (d) A session Curb has already terminated is not re-killed on the next scan
/// while it logs no fresh activity after the kill.
#[test]
fn terminated_session_is_not_rekilled_on_the_next_scan() {
    let temp = TempDir::new().expect("tempdir");
    let cwd = temp.path().to_path_buf();
    let worker_marker = unique_marker("rekill");
    let mut worker = Worker::spawn(&cwd, &worker_marker);
    let worker_pid = worker.pid();

    let (snapshot, worker_process) = observe(worker_pid);
    let session_cwd = worker_process.cwd.expect("worker cwd");

    let state = TempDir::new().expect("state dir");
    let cfg = enforcement_config(
        state.path(),
        marker_agent(&worker_marker),
        HumanDuration::seconds(0), // grace 0: scan 1 starts grace, scan 2 kills
    );
    let mut watch = UsageWatch::default();

    let now = Utc::now();
    // Scan 1: grace.
    let first_sessions = local_scan(
        &mut watch,
        &cfg,
        &[over_kill_event(now, &session_cwd, 250)],
        &snapshot,
        now,
    );
    assert!(
        is_alive(worker_pid),
        "worker must survive the initial grace scan\n{}",
        e2e_diag(
            &cfg,
            &watch,
            worker_pid,
            &worker_marker,
            None,
            &first_sessions,
        )
    );

    // Scan 2: terminate.
    let killed_at = now + chrono::Duration::seconds(1);
    let (snapshot, _) = observe(worker_pid);
    let second_sessions = local_scan(
        &mut watch,
        &cfg,
        &[over_kill_event(killed_at, &session_cwd, 250)],
        &snapshot,
        killed_at,
    );
    assert!(
        wait_until_gone(worker_pid, Duration::from_secs(10)),
        "worker pid {} was not terminated on scan 2\n{}",
        worker_pid.get(),
        e2e_diag(
            &cfg,
            &watch,
            worker_pid,
            &worker_marker,
            None,
            &second_sessions,
        )
    );
    assert!(
        watch.terminated_keys().contains("codex:e2e-session"),
        "terminated key missing after scan 2\n{}",
        e2e_diag(
            &cfg,
            &watch,
            worker_pid,
            &worker_marker,
            None,
            &second_sessions,
        )
    );

    let after_kill = ledger_event_types(&cfg);
    assert_eq!(
        after_kill,
        [
            "usage_warning",
            "usage_grace_started",
            "usage_termination_started",
            "usage_termination_completed",
        ],
        "unexpected ledger lifecycle after scan 2\n{}",
        e2e_diag(
            &cfg,
            &watch,
            worker_pid,
            &worker_marker,
            None,
            &second_sessions,
        )
    );

    // Scan 3: the session has logged no new activity since the kill (the
    // checkpoint timestamp is at the kill time). The worker is gone from the
    // table. Curb must treat the session as dead and not attempt another kill.
    let later = killed_at + chrono::Duration::seconds(1);
    let snapshot = SystemPlatform.capture().expect("capture");
    let third_sessions = local_scan(
        &mut watch,
        &cfg,
        &[over_kill_event(killed_at, &session_cwd, 250)],
        &snapshot,
        later,
    );

    // No new termination lifecycle was appended: the session was not re-killed.
    assert_eq!(
        ledger_event_types(&cfg),
        after_kill,
        "a terminated session was re-processed on the next scan\n{}",
        e2e_diag(
            &cfg,
            &watch,
            worker_pid,
            &worker_marker,
            None,
            &third_sessions,
        )
    );
    assert!(
        watch.terminated_keys().contains("codex:e2e-session"),
        "terminated key missing after scan 3\n{}",
        e2e_diag(
            &cfg,
            &watch,
            worker_pid,
            &worker_marker,
            None,
            &third_sessions,
        )
    );

    worker.force_cleanup();
}
