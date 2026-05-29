//! Write-path persistence for sessions.
//!
//! The read-model boundary in [`crate::service`] derives the pure snapshot and
//! its view transforms. This module owns the side-effecting half: the
//! [`Service`] entry points that acknowledge or stop a session, the
//! session-ack files those operations write and roll back, and the ledger
//! appends they emit. Keeping disk and ledger mutation here means a reader of
//! the snapshot-derivation code path never encounters write-path I/O.

use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};

use crate::config::{Config, Mode};
use crate::ledger::{self, Ledger};
use crate::platform::{self, Platform};
use crate::service::{
    AckRequest, AckView, Correlation, ServiceError, Session, SessionAck, StopExpectedIdentity,
    StopRequest, StopView, active_session_ack, build_session_view, correlate, find_session,
    process_matches, read_session_ack, session_ack_path, usage_activity_start,
};
use crate::usage::Event;

/// Side-effecting session operations.
///
/// `Service` revalidates identity and persists acknowledgements or terminations
/// for one scan's events. It borrows the read-model derivation from
/// [`crate::service`] to decide whether an action is permitted, then performs the
/// disk and ledger writes that the read model deliberately does not.
pub struct Service<'a, P: Platform> {
    cfg: &'a Config,
    events: &'a [Event],
    platform: &'a P,
}

impl<'a, P: Platform> Service<'a, P> {
    pub fn new(cfg: &'a Config, events: &'a [Event], platform: &'a P) -> Self {
        Self {
            cfg,
            events,
            platform,
        }
    }

    pub fn acknowledge_session(
        &self,
        session_key: &str,
        request: AckRequest,
        now: DateTime<Utc>,
    ) -> Result<AckView, ServiceError> {
        if session_key.is_empty() {
            return Err(ServiceError::InvalidAck(
                "session key is required".to_string(),
            ));
        }
        if request.extend_seconds < 0 {
            return Err(ServiceError::InvalidAck(
                "extension must be positive".to_string(),
            ));
        }
        let session =
            find_session(self.events, session_key).ok_or(ServiceError::SessionNotFound)?;
        let default_extend = self.cfg.defaults.ack_extension.as_std();
        let mut extend = if request.extend_seconds == 0 {
            default_extend
        } else {
            std::time::Duration::from_secs(request.extend_seconds as u64)
        };
        if extend.is_zero() {
            return Err(ServiceError::InvalidAck(
                "ack extension must be configured".to_string(),
            ));
        }
        if !default_extend.is_zero() && extend > default_extend {
            extend = default_extend;
        }
        let previous_ack = read_session_ack(&self.cfg.service.state_dir, &session.key)?;
        let ack = write_session_ack(
            &self.cfg.service.state_dir,
            &session.key,
            extend,
            &request.reason,
            now,
        )?;
        if let Err(err) = self.append_session_ack_event(&ack, extend) {
            rollback_session_ack(&self.cfg.service.state_dir, &session.key, previous_ack)?;
            return Err(err);
        }
        Ok(AckView {
            session_key: ack.session_key,
            extend_seconds: extend.as_secs() as i64,
            until: ack.until,
            reason: ack.reason,
        })
    }

    pub fn stop_session(
        &self,
        session_key: &str,
        request: StopRequest,
        now: DateTime<Utc>,
    ) -> Result<StopView, ServiceError> {
        if session_key.is_empty() {
            return Err(ServiceError::InvalidStop(
                "session key is required".to_string(),
            ));
        }
        if !request.confirm {
            return Err(ServiceError::InvalidStop(
                "confirmation is required".to_string(),
            ));
        }
        let scope = if request.scope.is_empty() {
            "tree"
        } else {
            request.scope.as_str()
        };
        if scope != "tree" {
            return Err(ServiceError::InvalidStop(
                "only process tree scope is supported".to_string(),
            ));
        }
        validate_expected_stop_identity(&request.expected)?;
        if self.cfg.mode != Mode::Enforcement {
            return Err(ServiceError::StopConflict(
                "enforcement mode is required".to_string(),
            ));
        }
        let session =
            find_session(self.events, session_key).ok_or(ServiceError::SessionNotFound)?;
        if active_session_ack(&self.cfg.service.state_dir, &session.key, now)?.is_some() {
            return Err(ServiceError::StopConflict(
                "session is acknowledged".to_string(),
            ));
        }
        let snapshot = self.platform.capture().map_err(|error| {
            ServiceError::StopConflict(format!("process snapshot unavailable: {error}"))
        })?;
        let matches = process_matches(self.cfg, &snapshot);
        let correlation = correlate(&session, &matches);
        if !correlation.matched {
            return Err(ServiceError::StopConflict(
                "no live process correlation".to_string(),
            ));
        }
        let agent = correlation.agent.as_ref().expect("matched agent");
        if !agent.can_terminate(self.cfg.usage.escalate_supervised) {
            return Err(ServiceError::StopConflict(
                "matched agent is watch-only".to_string(),
            ));
        }
        let window_start =
            now - chrono::Duration::from_std(self.cfg.usage.window.as_std()).unwrap();
        let fresh_start = usage_activity_start(self.cfg, now);
        let view = build_session_view(
            self.cfg,
            &session,
            &correlation,
            window_start,
            fresh_start,
            now,
        );
        if view.alert != "kill" || !view.can_stop {
            return Err(ServiceError::StopConflict(
                "session is not an actionable stop candidate".to_string(),
            ));
        }
        let process = correlation.process.as_ref().expect("matched process");
        validate_stop_expectation(&request.expected, process)?;
        let target = snapshot.termination_target(process).ok_or_else(|| {
            ServiceError::StopConflict("process identity could not be revalidated".to_string())
        })?;
        self.append_manual_stop_event(
            ledger::LedgerEvent::ManualStopStarted,
            &session,
            &correlation,
            &target,
            None,
            &request.reason,
        )?;
        let result = self
            .platform
            .terminate(&target, self.cfg.usage.grace_period.as_std());
        self.append_manual_stop_event(
            ledger::LedgerEvent::ManualStopCompleted,
            &session,
            &correlation,
            &target,
            Some("completed"),
            &request.reason,
        )?;
        let root = target.root();
        Ok(StopView {
            session_key: session.key,
            agent_id: agent.id.clone(),
            pid: root.pid.get(),
            started_at: root.started_at.expect("validated start time"),
            owner: root.username.clone().unwrap_or_default(),
            executable: root.executable.clone(),
            bundle_id: root.bundle_id.clone(),
            team_id: root.team_id.clone(),
            scope: scope.to_string(),
            scope_pids: target.scope().iter().map(|pid| pid.get()).collect(),
            result,
        })
    }

    fn append_session_ack_event(
        &self,
        ack: &SessionAck,
        extend: std::time::Duration,
    ) -> Result<(), ServiceError> {
        let mut data = Map::new();
        data.insert(
            "session_key".to_string(),
            Value::String(ack.session_key.clone()),
        );
        data.insert(
            "extend_seconds".to_string(),
            Value::Number(extend.as_secs().into()),
        );
        data.insert("until".to_string(), Value::String(ack.until.to_rfc3339()));
        self.append_ledger_event(
            ledger::Event::new(ledger::LedgerEvent::SessionAckReceived.as_str())
                .with_data(data)
                .with_message(ack.reason.clone()),
        )
    }

    fn append_manual_stop_event(
        &self,
        event_type: ledger::LedgerEvent,
        session: &Session,
        correlation: &Correlation,
        target: &platform::TerminationTarget,
        result: Option<&str>,
        reason: &str,
    ) -> Result<(), ServiceError> {
        let mut event = ledger::Event::new(event_type.as_str()).with_data(manual_stop_event_data(
            session,
            correlation,
            target,
            result,
        ));
        event.agent_id = correlation.agent.as_ref().map(|agent| agent.id.clone());
        event.mode = Some(self.cfg.mode.to_string());
        if !reason.is_empty() {
            event.message = Some(reason.to_string());
        }
        self.append_ledger_event(event)
    }

    fn append_ledger_event(&self, event: ledger::Event) -> Result<(), ServiceError> {
        Ledger::open(&self.cfg.ledger.path)?.append(event)?;
        Ok(())
    }
}

pub fn write_session_ack(
    state_dir: &Path,
    session_key: &str,
    extend: std::time::Duration,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<SessionAck, ServiceError> {
    if session_key.is_empty() {
        return Err(ServiceError::InvalidAck(
            "session key is required".to_string(),
        ));
    }
    if extend.is_zero() {
        return Err(ServiceError::InvalidAck(
            "extension must be positive".to_string(),
        ));
    }
    let ack = SessionAck {
        session_key: session_key.to_string(),
        reason: reason.to_string(),
        until: now + chrono::Duration::from_std(extend).unwrap(),
        created_at: now,
    };
    let path = session_ack_path(state_dir, session_key);
    let dir = path.parent().unwrap_or(state_dir);
    fs::create_dir_all(dir).map_err(|source| ServiceError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|source| {
            ServiceError::Io {
                path: dir.to_path_buf(),
                source,
            }
        })?;
    }
    let content = serde_json::to_vec_pretty(&ack).map_err(|source| ServiceError::Json {
        path: path.clone(),
        source,
    })?;
    fs::write(&path, content).map_err(|source| ServiceError::Io {
        path: path.clone(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            ServiceError::Io {
                path: path.clone(),
                source,
            }
        })?;
    }
    Ok(ack)
}

fn delete_session_ack(state_dir: &Path, session_key: &str) -> Result<(), ServiceError> {
    let path = session_ack_path(state_dir, session_key);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ServiceError::Io { path, source }),
    }
}

fn rollback_session_ack(
    state_dir: &Path,
    session_key: &str,
    previous: Option<SessionAck>,
) -> Result<(), ServiceError> {
    match previous {
        Some(previous) => {
            let extend = previous
                .until
                .signed_duration_since(previous.created_at)
                .to_std()
                .map_err(|_| {
                    ServiceError::InvalidAck("previous ack duration is invalid".to_string())
                })?;
            write_session_ack(
                state_dir,
                session_key,
                extend,
                &previous.reason,
                previous.created_at,
            )?;
            Ok(())
        }
        None => delete_session_ack(state_dir, session_key),
    }
}

fn manual_stop_event_data(
    session: &Session,
    correlation: &Correlation,
    target: &platform::TerminationTarget,
    result: Option<&str>,
) -> Map<String, Value> {
    let root = target.root();
    let mut data = Map::new();
    data.insert(
        "session_key".to_string(),
        Value::String(session.key.clone()),
    );
    data.insert("session_id".to_string(), Value::String(session.id.clone()));
    data.insert(
        "provider".to_string(),
        Value::String(session.provider.clone()),
    );
    if let Some(cwd) = &session.cwd {
        data.insert("cwd".to_string(), Value::String(cwd.display().to_string()));
    }
    data.insert("turn_tokens".to_string(), json!(session.latest_turn_tokens));
    data.insert(
        "latest_spent_tokens".to_string(),
        json!(session.latest_spent_tokens),
    );
    data.insert(
        "window_spent_tokens".to_string(),
        json!(session.window_spent_tokens),
    );
    if let Some(agent) = &correlation.agent {
        data.insert("agent_id".to_string(), Value::String(agent.id.clone()));
    }
    data.insert("pid".to_string(), json!(root.pid.get()));
    if let Some(started_at) = root.started_at {
        data.insert(
            "started_at".to_string(),
            Value::String(started_at.to_rfc3339()),
        );
    }
    if let Some(owner) = &root.username {
        data.insert("owner".to_string(), Value::String(owner.clone()));
    }
    if let Some(executable) = &root.executable {
        data.insert(
            "executable".to_string(),
            Value::String(executable.display().to_string()),
        );
    }
    if let Some(bundle_id) = &root.bundle_id {
        data.insert("bundle_id".to_string(), Value::String(bundle_id.clone()));
    }
    if let Some(team_id) = &root.team_id {
        data.insert("team_id".to_string(), Value::String(team_id.clone()));
    }
    data.insert("scope".to_string(), Value::String("tree".to_string()));
    data.insert(
        "scope_pids".to_string(),
        Value::Array(target.scope().iter().map(|pid| json!(pid.get())).collect()),
    );
    data.insert(
        "correlation".to_string(),
        Value::String(correlation.reason.clone()),
    );
    data.insert("correlation_score".to_string(), json!(correlation.score));
    if let Some(result) = result {
        data.insert("result".to_string(), Value::String(result.to_string()));
    }
    data
}

fn validate_expected_stop_identity(expected: &StopExpectedIdentity) -> Result<(), ServiceError> {
    if expected.pid == 0 {
        return Err(ServiceError::InvalidStop(
            "expected pid is required".to_string(),
        ));
    }
    if expected.started_at.is_none() {
        return Err(ServiceError::InvalidStop(
            "expected process start time is required".to_string(),
        ));
    }
    if expected.owner.is_empty() {
        return Err(ServiceError::InvalidStop(
            "expected owner is required".to_string(),
        ));
    }
    if expected.executable.is_none() && expected.bundle_id.is_none() && expected.team_id.is_none() {
        return Err(ServiceError::InvalidStop(
            "expected executable or app identity is required".to_string(),
        ));
    }
    Ok(())
}

fn validate_stop_expectation(
    expected: &StopExpectedIdentity,
    actual: &platform::Process,
) -> Result<(), ServiceError> {
    if actual.pid.get() != expected.pid {
        return Err(ServiceError::StopConflict("pid changed".to_string()));
    }
    if actual.started_at != expected.started_at {
        return Err(ServiceError::StopConflict(
            "process start time changed".to_string(),
        ));
    }
    if actual.username.as_deref() != Some(expected.owner.as_str()) {
        return Err(ServiceError::StopConflict(
            "process owner changed".to_string(),
        ));
    }
    if let Some(executable) = &expected.executable
        && actual.executable.as_ref() != Some(executable)
    {
        return Err(ServiceError::StopConflict("executable changed".to_string()));
    }
    if let Some(bundle_id) = &expected.bundle_id
        && actual.bundle_id.as_ref() != Some(bundle_id)
    {
        return Err(ServiceError::StopConflict("bundle id changed".to_string()));
    }
    if let Some(team_id) = &expected.team_id
        && actual.team_id.as_ref() != Some(team_id)
    {
        return Err(ServiceError::StopConflict("team id changed".to_string()));
    }
    if !actual.has_termination_identity() {
        return Err(ServiceError::StopConflict(
            "process identity is incomplete".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use chrono::TimeZone;

    use super::*;
    use crate::config::Config;
    use crate::platform::{PlatformError, TerminationTarget};
    use crate::service::build_snapshot_with_processes;

    #[test]
    fn acknowledge_session_persists_and_suppresses_actionability() {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
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
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
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
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
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
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
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
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
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

        let mut alert_cfg = Config::load("configs/curb.example.yaml").unwrap();
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
}
