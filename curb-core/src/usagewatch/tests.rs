use std::sync::Mutex;

use chrono::TimeZone;
use serde_json::json;
use tempfile::tempdir;

use super::*;
use crate::config::{Config, HumanDuration};

/// An opaque stop token for tests, identified by the pids it would kill and
/// whether the safety guard should accept or reject it at stop time.
#[derive(Clone, Debug)]
struct FakeToken {
    pids: Vec<i32>,
    supervisor_pids: Vec<i32>,
    valid: bool,
}

impl StopToken for FakeToken {
    fn clone_token(&self) -> Box<dyn StopToken> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Records which pids it was asked to terminate, mirroring the real local
/// enforcer: it picks the supervisor scope when escalating, else the leaf,
/// and rejects an invalid (pid-reuse) token.
#[derive(Default)]
struct FakeEnforcer {
    terminated: Mutex<Vec<i32>>,
    notifications: Mutex<Vec<(String, String)>>,
}

impl Enforcer for FakeEnforcer {
    fn notify(&self, title: &str, message: &str) {
        self.notifications
            .lock()
            .unwrap()
            .push((title.to_string(), message.to_string()));
    }

    fn stop(&self, token: &dyn StopToken, escalate: bool) -> StopResolution {
        let token = token
            .as_any()
            .downcast_ref::<FakeToken>()
            .expect("fake token");
        if !token.valid {
            return StopResolution::Rejected;
        }
        let scope = if escalate && !token.supervisor_pids.is_empty() {
            token.supervisor_pids.clone()
        } else {
            token.pids.clone()
        };
        self.terminated.lock().unwrap().extend(scope);
        StopResolution::Stopped(json!({}))
    }
}

fn policy_session(
    now: DateTime<Utc>,
    key: &str,
    id: &str,
    spent: i64,
    target: AgentTarget,
) -> PolicySession {
    PolicySession {
        key: key.to_string(),
        id: id.to_string(),
        provider: "codex".to_string(),
        cwd: Some("/repo".into()),
        models: BTreeSet::new(),
        last: Some(now),
        last_usage: Some(now),
        calls: 1,
        latest_turn_tokens: spent,
        latest_spent_tokens: spent,
        window_spent_tokens: spent,
        total_tokens: spent,
        target,
        acknowledged: false,
    }
}

fn terminable_target(pid: i32) -> AgentTarget {
    AgentTarget {
        matched: true,
        agent_id: Some("codex-cli".to_string()),
        can_terminate: true,
        supervised: false,
        pid: Some(pid as i64),
        score: 125,
        reason: "provider+cwd".to_string(),
        stop_token: Some(Box::new(FakeToken {
            pids: vec![pid],
            supervisor_pids: Vec::new(),
            valid: true,
        })),
    }
}

fn enforcement_cfg(state: &std::path::Path) -> Config {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.mode = Mode::Enforcement;
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    cfg.usage.grace_period = HumanDuration::seconds(30);
    cfg.service.state_dir = state.to_path_buf();
    cfg.ledger.path = state.join("runs.ndjson");
    cfg
}

fn window_start(cfg: &Config, now: DateTime<Utc>) -> DateTime<Utc> {
    now - chrono::Duration::from_std(cfg.usage.window.as_std()).unwrap()
}

/// The decision the scan recorded, read back from the real temp ledger.
fn ledger_event_types(cfg: &Config) -> Vec<String> {
    ledger::read(&cfg.ledger.path)
        .unwrap()
        .into_iter()
        .map(|event| event.event_type)
        .collect()
}

#[test]
fn enforcement_auto_kills_a_correlated_worker_after_grace() {
    let state = tempdir().unwrap();
    let cfg = enforcement_cfg(state.path());

    let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
    let enforcer = FakeEnforcer::default();
    let mut watch = UsageWatch::default();

    // Over the kill line: grace starts, nothing is terminated yet.
    let session = policy_session(now, "codex:s1", "s1", 250, terminable_target(4242));
    let first = watch
        .scan(&cfg, &[session], &enforcer, window_start(&cfg, now), now)
        .unwrap();
    assert_eq!(first.observed_sessions, 1);
    assert_eq!(first.warnings, 1);
    assert_eq!(first.grace_started, 1);
    assert_eq!(first.stop_attempted, 0);
    assert!(enforcer.terminated.lock().unwrap().is_empty());

    // After the grace period, still over the line: the worker is terminated.
    let after = now + chrono::Duration::seconds(31);
    let session = policy_session(after, "codex:s1", "s1", 250, terminable_target(4242));
    let second = watch
        .scan(
            &cfg,
            &[session],
            &enforcer,
            window_start(&cfg, after),
            after,
        )
        .unwrap();
    assert_eq!(second.observed_sessions, 1);
    assert_eq!(second.stop_attempted, 1);
    assert_eq!(second.stop_completed, 1);
    assert_eq!(second.terminated_sessions, 1);
    assert_eq!(*enforcer.terminated.lock().unwrap(), vec![4242]);
}

#[test]
fn watch_mode_never_terminates() {
    let state = tempdir().unwrap();
    let mut cfg = enforcement_cfg(state.path());
    cfg.mode = Mode::Alert; // watch, not enforce
    cfg.usage.grace_period = HumanDuration::seconds(0);

    let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
    let enforcer = FakeEnforcer::default();
    let mut watch = UsageWatch::default();

    let session = policy_session(now, "codex:s1", "s1", 250, terminable_target(4242));
    let report = watch
        .scan(&cfg, &[session], &enforcer, window_start(&cfg, now), now)
        .unwrap();
    assert_eq!(report.would_stop, 1);
    let later = now + chrono::Duration::seconds(5);
    let session = policy_session(later, "codex:s1", "s1", 250, terminable_target(4242));
    let later_report = watch
        .scan(
            &cfg,
            &[session],
            &enforcer,
            window_start(&cfg, later),
            later,
        )
        .unwrap();
    assert_eq!(later_report.would_stop, 0);
    assert!(enforcer.terminated.lock().unwrap().is_empty());
}

/// A supervised desktop worker: terminable, but watch-only unless escalated.
fn supervised_target(leaf: i32, supervisor: i32, can_terminate: bool) -> AgentTarget {
    AgentTarget {
        matched: true,
        agent_id: Some("codex-desktop".to_string()),
        can_terminate,
        supervised: true,
        pid: Some(leaf as i64),
        score: 125,
        reason: "provider+cwd".to_string(),
        stop_token: Some(Box::new(FakeToken {
            pids: vec![leaf],
            supervisor_pids: vec![supervisor, leaf],
            valid: true,
        })),
    }
}

#[test]
fn supervised_desktop_worker_is_watch_only_by_default() {
    let state = tempdir().unwrap();
    let mut cfg = enforcement_cfg(state.path());
    cfg.usage.grace_period = HumanDuration::seconds(0);

    let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
    let enforcer = FakeEnforcer::default();
    let mut watch = UsageWatch::default();

    for at in [now, now + chrono::Duration::seconds(5)] {
        // Escalation off: the matched agent is not terminable.
        let session = policy_session(
            at,
            "codex:s1",
            "s1",
            250,
            supervised_target(7731, 3032, false),
        );
        watch
            .scan(&cfg, &[session], &enforcer, window_start(&cfg, at), at)
            .unwrap();
    }
    // Killing the leaf is futile (it respawns), so Curb refuses by default.
    assert!(enforcer.terminated.lock().unwrap().is_empty());
    assert!(watch.terminated_keys().is_empty());
}

#[test]
fn escalate_supervised_kills_the_supervisor() {
    let state = tempdir().unwrap();
    let mut cfg = enforcement_cfg(state.path());
    cfg.usage.grace_period = HumanDuration::seconds(0);
    cfg.usage.escalate_supervised = true;

    let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
    let enforcer = FakeEnforcer::default();
    let mut watch = UsageWatch::default();

    for at in [now, now + chrono::Duration::seconds(5)] {
        // Escalation on: the agent is terminable.
        let session = policy_session(
            at,
            "codex:s1",
            "s1",
            250,
            supervised_target(7731, 3032, true),
        );
        watch
            .scan(&cfg, &[session], &enforcer, window_start(&cfg, at), at)
            .unwrap();
    }
    // With escalation on, Curb targets the supervisor's whole tree.
    let killed = enforcer.terminated.lock().unwrap().clone();
    assert!(
        killed.contains(&3032),
        "supervisor not terminated: {killed:?}"
    );
    assert!(killed.contains(&7731), "leaf not terminated: {killed:?}");
}

#[test]
fn killed_worker_is_marked_terminated_and_not_rekilled() {
    let state = tempdir().unwrap();
    let mut cfg = enforcement_cfg(state.path());
    cfg.usage.grace_period = HumanDuration::seconds(0);

    let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
    let enforcer = FakeEnforcer::default();
    let mut watch = UsageWatch::default();

    // scan 1: grace starts. scan 2: terminate. scan 3: already dead (no new
    // activity since the kill) — skip, do not kill again.
    let killed_at = now + chrono::Duration::seconds(5);
    for (at, last) in [
        (now, now),
        (killed_at, killed_at),
        (killed_at + chrono::Duration::seconds(5), killed_at),
    ] {
        let mut session = policy_session(at, "codex:s1", "s1", 250, terminable_target(4242));
        session.last_usage = Some(last);
        session.last = Some(last);
        watch
            .scan(&cfg, &[session], &enforcer, window_start(&cfg, at), at)
            .unwrap();
    }
    // Killed exactly once, and remembered so the read model drops its row.
    assert_eq!(*enforcer.terminated.lock().unwrap(), vec![4242]);
    assert!(watch.terminated_keys().contains("codex:s1"));
}

/// Scenario 1: over the kill line, but no live process correlates to the
/// session. Curb must refuse and say so — `usage_kill_blocked` — never kill.
#[test]
fn uncorrelated_over_kill_blocks_and_records_decision() {
    let state = tempdir().unwrap();
    let mut cfg = enforcement_cfg(state.path());
    cfg.usage.grace_period = HumanDuration::seconds(0);

    let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
    let enforcer = FakeEnforcer::default();
    let mut watch = UsageWatch::default();

    // Nothing correlates: an empty AgentTarget.
    let session = policy_session(now, "codex:s1", "s1", 250, AgentTarget::default());
    watch
        .scan(&cfg, &[session], &enforcer, window_start(&cfg, now), now)
        .unwrap();

    assert_eq!(
        ledger_event_types(&cfg),
        ["usage_warning", "usage_kill_blocked"]
    );
    assert!(enforcer.terminated.lock().unwrap().is_empty());
    assert!(watch.terminated_keys().is_empty());
}

/// Scenario 2: a supervised desktop worker is over the kill line in enforce
/// mode with escalation off. Killing the leaf is futile, so Curb refuses —
/// `usage_kill_blocked` — and nothing dies.
#[test]
fn supervised_over_kill_blocks_and_records_decision() {
    let state = tempdir().unwrap();
    let mut cfg = enforcement_cfg(state.path());
    cfg.usage.grace_period = HumanDuration::seconds(0);
    cfg.usage.escalate_supervised = false;

    let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
    let enforcer = FakeEnforcer::default();
    let mut watch = UsageWatch::default();

    let session = policy_session(
        now,
        "codex:s1",
        "s1",
        250,
        supervised_target(7731, 3032, false),
    );
    watch
        .scan(&cfg, &[session], &enforcer, window_start(&cfg, now), now)
        .unwrap();

    assert_eq!(
        ledger_event_types(&cfg),
        ["usage_warning", "usage_kill_blocked"]
    );
    assert!(enforcer.terminated.lock().unwrap().is_empty());
    assert!(watch.terminated_keys().is_empty());
}

/// Scenario 3: a correlated, terminable worker is over the kill line, but
/// Curb is in watch (alert) mode. It must record `usage_would_terminate`,
/// not actually terminate.
#[test]
fn alert_mode_over_kill_records_would_terminate() {
    let state = tempdir().unwrap();
    let mut cfg = enforcement_cfg(state.path());
    cfg.mode = Mode::Alert;
    cfg.usage.grace_period = HumanDuration::seconds(0);

    let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
    let enforcer = FakeEnforcer::default();
    let mut watch = UsageWatch::default();

    let session = policy_session(now, "codex:s1", "s1", 250, terminable_target(4242));
    watch
        .scan(&cfg, &[session], &enforcer, window_start(&cfg, now), now)
        .unwrap();

    assert_eq!(
        ledger_event_types(&cfg),
        ["usage_warning", "usage_would_terminate"]
    );
    assert!(enforcer.terminated.lock().unwrap().is_empty());
    assert!(watch.terminated_keys().is_empty());
}

/// Scenario 4: the worker correlates and grace elapses, but by kill time the
/// safety guard rejects the stored token (reused pid). Termination fails —
/// `usage_termination_failed` — and nothing dies.
#[test]
fn pid_reuse_at_kill_time_records_termination_failed() {
    let state = tempdir().unwrap();
    let mut cfg = enforcement_cfg(state.path());
    cfg.usage.grace_period = HumanDuration::seconds(1);

    let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
    let enforcer = FakeEnforcer::default();
    let mut watch = UsageWatch::default();

    // scan 1: grace starts against a valid token.
    let session = policy_session(now, "codex:s1", "s1", 250, terminable_target(4242));
    watch
        .scan(&cfg, &[session], &enforcer, window_start(&cfg, now), now)
        .unwrap();
    // scan 2 (grace elapsed): the stored token no longer revalidates. The
    // grace-time token is the one the enforcer checks, so reuse is modeled
    // by storing an invalid token at grace time.
    let after = now + chrono::Duration::seconds(2);
    let mut target = terminable_target(4242);
    target.stop_token = Some(Box::new(FakeToken {
        pids: vec![4242],
        supervisor_pids: Vec::new(),
        valid: true,
    }));
    let session = policy_session(after, "codex:s1", "s1", 250, target);
    // Replace the stored grace-time token with an invalid one to model that
    // the worker behind it changed identity.
    watch.targets.insert(
        "codex:s1".to_string(),
        Box::new(FakeToken {
            pids: vec![4242],
            supervisor_pids: Vec::new(),
            valid: false,
        }),
    );
    watch
        .scan(
            &cfg,
            &[session],
            &enforcer,
            window_start(&cfg, after),
            after,
        )
        .unwrap();

    assert_eq!(
        ledger_event_types(&cfg),
        [
            "usage_warning",
            "usage_grace_started",
            "usage_termination_failed"
        ]
    );
    assert!(enforcer.terminated.lock().unwrap().is_empty());
    assert!(watch.terminated_keys().is_empty());
}

/// Scenario 5: a killed worker logs fresh usage after the kill — it came
/// back. The next scans must re-arm and re-kill it, not treat it as dead.
#[test]
fn killed_worker_that_resumes_is_rearmed_and_rekilled() {
    let state = tempdir().unwrap();
    let mut cfg = enforcement_cfg(state.path());
    cfg.usage.grace_period = HumanDuration::seconds(0);

    let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
    let enforcer = FakeEnforcer::default();
    let mut watch = UsageWatch::default();

    // scan 1: grace. scan 2: first kill.
    let killed_at = now + chrono::Duration::seconds(5);
    let session = policy_session(now, "codex:s1", "s1", 250, terminable_target(4242));
    watch
        .scan(&cfg, &[session], &enforcer, window_start(&cfg, now), now)
        .unwrap();
    let mut session = policy_session(killed_at, "codex:s1", "s1", 250, terminable_target(4242));
    session.last_usage = Some(killed_at);
    watch
        .scan(
            &cfg,
            &[session],
            &enforcer,
            window_start(&cfg, killed_at),
            killed_at,
        )
        .unwrap();
    assert_eq!(*enforcer.terminated.lock().unwrap(), vec![4242]);

    // Fresh usage after the kill: the session resumed. scan 3 re-arms grace,
    // scan 4 re-kills it.
    let resumed = killed_at + chrono::Duration::seconds(5);
    let mut session = policy_session(resumed, "codex:s1", "s1", 250, terminable_target(4242));
    session.last_usage = Some(resumed);
    watch
        .scan(
            &cfg,
            &[session],
            &enforcer,
            window_start(&cfg, resumed),
            resumed,
        )
        .unwrap();
    let rekill = resumed + chrono::Duration::seconds(5);
    let mut session = policy_session(rekill, "codex:s1", "s1", 250, terminable_target(4242));
    session.last_usage = Some(rekill);
    watch
        .scan(
            &cfg,
            &[session],
            &enforcer,
            window_start(&cfg, rekill),
            rekill,
        )
        .unwrap();

    // Re-armed (a second grace) and re-killed (the pid appears twice).
    assert_eq!(*enforcer.terminated.lock().unwrap(), vec![4242, 4242]);
    assert!(watch.terminated_keys().contains("codex:s1"));
    assert_eq!(
        ledger_event_types(&cfg),
        [
            "usage_warning",
            "usage_grace_started",
            "usage_termination_started",
            "usage_termination_completed",
            "usage_grace_started",
            "usage_termination_started",
            "usage_termination_completed",
        ]
    );
}

/// Scenario 6: after a kill, time moves past the window with no further
/// activity from that session. The terminated row must age out (so the read
/// model stops showing it) and the worker must not be re-killed.
#[test]
fn kill_aged_out_of_window_drops_row_without_rekill() {
    let state = tempdir().unwrap();
    let mut cfg = enforcement_cfg(state.path());
    cfg.usage.grace_period = HumanDuration::seconds(0);
    cfg.usage.window = HumanDuration::minutes(5);

    let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
    let enforcer = FakeEnforcer::default();
    let mut watch = UsageWatch::default();

    // scan 1: grace. scan 2: kill, remembered at killed_at.
    let killed_at = now + chrono::Duration::seconds(5);
    let session = policy_session(now, "codex:s1", "s1", 250, terminable_target(4242));
    watch
        .scan(&cfg, &[session], &enforcer, window_start(&cfg, now), now)
        .unwrap();
    let mut session = policy_session(killed_at, "codex:s1", "s1", 250, terminable_target(4242));
    session.last_usage = Some(killed_at);
    watch
        .scan(
            &cfg,
            &[session],
            &enforcer,
            window_start(&cfg, killed_at),
            killed_at,
        )
        .unwrap();
    assert!(watch.terminated_keys().contains("codex:s1"));

    // scan 3 runs past the window. Only an unrelated session is active, so
    // s1 never enters the loop — the age-out retain is the one thing that
    // can drop its remembered kill.
    let aged_out = killed_at + chrono::Duration::minutes(5) + chrono::Duration::seconds(10);
    let mut session = policy_session(aged_out, "codex:s2", "s2", 250, terminable_target(5555));
    session.cwd = Some("/elsewhere".into());
    watch
        .scan(
            &cfg,
            &[session],
            &enforcer,
            window_start(&cfg, aged_out),
            aged_out,
        )
        .unwrap();

    // The remembered kill aged out, and the original worker was not killed
    // again (only the first kill stands).
    assert!(!watch.terminated_keys().contains("codex:s1"));
    assert_eq!(*enforcer.terminated.lock().unwrap(), vec![4242]);
}
