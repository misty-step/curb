use std::path::PathBuf;
use std::sync::Mutex;

use chrono::TimeZone;

use super::*;
use crate::config::Config;
use crate::platform::{PlatformError, TerminationTarget};
use crate::service::{StopExpectedIdentity, build_snapshot_with_processes};

#[test]
fn acknowledge_session_persists_and_suppresses_actionability() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
    cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
    cfg.mode = crate::config::Mode::Enforcement;
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    cfg.defaults.ack_extension = crate::config::HumanDuration::seconds(60);
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let events = vec![event("codex", "s1", now, 250)];
    let platform = FakePlatform::new(process_snapshot(now, "codex", "/repo"));
    let service = Service::new(&cfg, &events, &platform);

    let ack = service
        .acknowledge_session(
            "s1",
            AckRequest {
                extend_seconds: 300,
                reason: "still supervising".to_string(),
            },
            now,
        )
        .unwrap();

    assert_eq!(ack.session_key, "codex:s1");
    assert_eq!(ack.extend_seconds, 60);
    let stored = active_session_ack(&cfg.service.state_dir, "codex:s1", now).unwrap();
    assert!(stored.is_some());
    let snapshot = build_snapshot_with_processes(
        &cfg,
        Some(&platform.capture().unwrap()),
        &events,
        Vec::new(),
        now,
    );
    assert_eq!(snapshot.sessions[0].alert, "ok");
    assert!(snapshot.sessions[0].acknowledged_until.is_some());
    assert!(!snapshot.sessions[0].can_stop);
    assert!(!snapshot.sessions[0].can_acknowledge);
}

#[test]
fn stop_session_revalidates_identity_and_terminates_tree() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.mode = crate::config::Mode::Enforcement;
    cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
    cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let events = vec![event("codex", "s1", now, 250)];
    let root = process(now, 100, "codex", "/repo");
    let mut child = process(now, 101, "node", "/repo");
    child.ppid = Some(platform::Pid::new(100));
    let platform = FakePlatform::new(platform::Snapshot::new([root.clone(), child]));
    let service = Service::new(&cfg, &events, &platform);

    let view = service
        .stop_session("s1", stop_request_for(&root), now)
        .unwrap();

    assert_eq!(view.session_key, "codex:s1");
    assert_eq!(view.agent_id, "codex-cli");
    assert_eq!(view.scope_pids, vec![101, 100]);
    assert_eq!(*platform.terminated.lock().unwrap(), vec![vec![101, 100]]);
}

#[test]
fn stop_session_records_structured_termination_result_errors() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.mode = crate::config::Mode::Enforcement;
    cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
    cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let events = vec![event("codex", "s1", now, 250)];
    let root = process(now, 100, "codex", "/repo");
    let platform = FakePlatform::new(platform::Snapshot::new([root.clone()]))
        .with_terminate_error("unsupported in this slice");
    let service = Service::new(&cfg, &events, &platform);

    let view = service
        .stop_session("s1", stop_request_for(&root), now)
        .unwrap();

    assert_eq!(
        view.result.errors,
        vec!["unsupported in this slice".to_string()]
    );
    let events = crate::ledger::read(cfg.ledger.path.clone()).unwrap();
    assert_eq!(events[0].event_type, "manual_stop_started");
    assert_eq!(events[1].event_type, "manual_stop_completed");
    assert_eq!(
        events[1].data.as_ref().unwrap().get("result").unwrap(),
        "completed"
    );
}

#[test]
fn stop_session_treats_process_capture_failure_as_stop_conflict() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.mode = crate::config::Mode::Enforcement;
    cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
    cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let events = vec![event("codex", "s1", now, 250)];
    let root = process(now, 100, "codex", "/repo");
    let platform = FakePlatform::capture_error("ps unavailable");
    let service = Service::new(&cfg, &events, &platform);

    let err = service
        .stop_session("s1", stop_request_for(&root), now)
        .unwrap_err();

    assert!(matches!(err, ServiceError::StopConflict(_)));
    assert!(platform.terminated.lock().unwrap().is_empty());
    assert!(
        crate::ledger::read(cfg.ledger.path.clone())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn stop_session_rejects_stale_identity_without_terminating() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.mode = crate::config::Mode::Enforcement;
    cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
    cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let events = vec![event("codex", "s1", now, 250)];
    let root = process(now, 100, "codex", "/repo");
    let platform = FakePlatform::new(platform::Snapshot::new([root.clone()]));
    let service = Service::new(&cfg, &events, &platform);
    let mut request = stop_request_for(&root);
    request.expected.started_at = root.started_at.map(|at| at - chrono::Duration::seconds(1));

    let err = service.stop_session("s1", request, now).unwrap_err();

    assert!(matches!(err, ServiceError::StopConflict(_)));
    assert!(platform.terminated.lock().unwrap().is_empty());
}

#[test]
fn stop_session_rejects_watch_only_uncorrelated_acknowledged_and_alert_mode() {
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let events = vec![event("codex", "s1", now, 250)];
    let root = process(now, 100, "codex", "/repo");

    let mut alert_cfg = Config::load(crate::config::example_config_path()).unwrap();
    alert_cfg.mode = crate::config::Mode::Alert;
    alert_cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
    alert_cfg.ledger.path = alert_cfg.service.state_dir.join("runs.ndjson");
    alert_cfg.usage.warn_turn_tokens = 100;
    alert_cfg.usage.kill_turn_tokens = 200;
    let alert_platform = FakePlatform::new(platform::Snapshot::new([root.clone()]));
    assert!(matches!(
        Service::new(&alert_cfg, &events, &alert_platform).stop_session(
            "s1",
            stop_request_for(&root),
            now
        ),
        Err(ServiceError::StopConflict(_))
    ));

    let mut uncorrelated_cfg = alert_cfg;
    uncorrelated_cfg.mode = crate::config::Mode::Enforcement;
    let other = process(now, 100, "codex", "/other");
    let uncorrelated_platform = FakePlatform::new(platform::Snapshot::new([other.clone()]));
    assert!(matches!(
        Service::new(&uncorrelated_cfg, &events, &uncorrelated_platform).stop_session(
            "s1",
            stop_request_for(&other),
            now
        ),
        Err(ServiceError::StopConflict(_))
    ));

    let mut watch_cfg = uncorrelated_cfg.clone();
    watch_cfg.agents[1].kind = crate::config::AgentKind::App;
    let watch_platform = FakePlatform::new(platform::Snapshot::new([root.clone()]));
    assert!(matches!(
        Service::new(&watch_cfg, &events, &watch_platform).stop_session(
            "s1",
            stop_request_for(&root),
            now
        ),
        Err(ServiceError::StopConflict(_))
    ));

    let ack_cfg = uncorrelated_cfg;
    write_session_ack(
        &ack_cfg.service.state_dir,
        "codex:s1",
        std::time::Duration::from_secs(60),
        "still supervising",
        now,
    )
    .unwrap();
    let ack_platform = FakePlatform::new(platform::Snapshot::new([root.clone()]));
    assert!(matches!(
        Service::new(&ack_cfg, &events, &ack_platform).stop_session(
            "s1",
            stop_request_for(&root),
            now
        ),
        Err(ServiceError::StopConflict(_))
    ));
}

fn event(provider: &str, session: &str, at: DateTime<Utc>, total: i64) -> Event {
    Event {
        kind: crate::usage::EventKind::TokenCheckpoint,
        provider: provider.to_string(),
        source: "test".to_string(),
        source_path: "fixture.jsonl".into(),
        session_id: Some(session.to_string()),
        turn_id: None,
        request_id: None,
        model: Some("model".to_string()),
        cwd: Some("/repo".into()),
        timestamp: Some(at),
        input_tokens: total,
        cached_input_tokens: 0,
        cache_creation_input_tokens: 0,
        output_tokens: 0,
        reasoning_output_tokens: 0,
        total_tokens: total,
        spent_tokens: total,
        cumulative_tokens: total,
        model_context_window: 0,
    }
}

fn process_snapshot(now: DateTime<Utc>, name: &str, cwd: &str) -> platform::Snapshot {
    platform::Snapshot::new([process(now, 100, name, cwd)])
}

fn process(now: DateTime<Utc>, pid: i32, name: &str, cwd: &str) -> platform::Process {
    platform::Process {
        pid: platform::Pid::new(pid),
        ppid: None,
        name: name.to_string(),
        executable: Some(PathBuf::from("/usr/local/bin/codex")),
        command: name.to_string(),
        cwd: Some(PathBuf::from(cwd)),
        started_at: Some(now - chrono::Duration::minutes(10)),
        username: Some("tester".to_string()),
        bundle_id: None,
        team_id: None,
    }
}

fn stop_request_for(process: &platform::Process) -> StopRequest {
    StopRequest {
        confirm: true,
        scope: "tree".to_string(),
        reason: "test".to_string(),
        expected: StopExpectedIdentity {
            pid: process.pid.get(),
            started_at: process.started_at,
            owner: process.username.clone().unwrap_or_default(),
            executable: process.executable.clone(),
            bundle_id: process.bundle_id.clone(),
            team_id: process.team_id.clone(),
        },
    }
}

struct FakePlatform {
    capture: Result<platform::Snapshot, PlatformError>,
    terminated: Mutex<Vec<Vec<i32>>>,
    terminate_error: Option<String>,
}

impl FakePlatform {
    fn new(snapshot: platform::Snapshot) -> Self {
        Self {
            capture: Ok(snapshot),
            terminated: Mutex::new(Vec::new()),
            terminate_error: None,
        }
    }

    fn capture_error(message: &str) -> Self {
        Self {
            capture: Err(PlatformError::Capture(message.to_string())),
            terminated: Mutex::new(Vec::new()),
            terminate_error: None,
        }
    }

    fn with_terminate_error(mut self, message: &str) -> Self {
        self.terminate_error = Some(message.to_string());
        self
    }
}

impl Platform for FakePlatform {
    fn capture(&self) -> Result<platform::Snapshot, PlatformError> {
        self.capture.clone()
    }

    fn notification_capability(&self) -> platform::NotificationCapability {
        platform::NotificationCapability {
            supported: true,
            status: "available".to_string(),
            message: "available".to_string(),
        }
    }

    fn termination_capability(&self) -> platform::TerminationCapability {
        platform::TerminationCapability {
            supported: true,
            status: "available".to_string(),
            message: "test platform can terminate process trees".to_string(),
        }
    }

    fn notify(&self, _title: &str, _body: &str) -> Result<(), PlatformError> {
        Ok(())
    }

    fn terminate(
        &self,
        target: &TerminationTarget,
        _grace: std::time::Duration,
    ) -> platform::TerminationResult {
        if let Some(message) = &self.terminate_error {
            return platform::TerminationResult {
                errors: vec![message.clone()],
                ..platform::TerminationResult::default()
            };
        }
        self.terminated
            .lock()
            .unwrap()
            .push(target.scope().iter().map(|pid| pid.get()).collect());
        platform::TerminationResult {
            soft_signaled: target.scope().iter().map(|pid| pid.get()).collect(),
            ..platform::TerminationResult::default()
        }
    }
}
