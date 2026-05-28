use std::path::PathBuf;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::config::Config;
use crate::platform::{Platform, PlatformError};
use crate::service::{
    self, AckRequest, AckView, Service, ServiceError, SessionView, Snapshot, StopRequest, StopView,
    TurnView,
};
use crate::usage::{Reader, UsageError};

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("snapshot unavailable")]
    SnapshotUnavailable,
    #[error(transparent)]
    Usage(#[from] UsageError),
    #[error(transparent)]
    Service(#[from] ServiceError),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TurnQuery {
    pub since: Option<DateTime<Utc>>,
    pub limit: usize,
}

pub struct Runtime<P: Platform> {
    cfg: Config,
    reader: Reader,
    platform: P,
    cache: Mutex<Option<Snapshot>>,
}

impl<P: Platform> Runtime<P> {
    pub fn new(cfg: Config, home: impl Into<PathBuf>, platform: P) -> Self {
        let state_dir = cfg.service.state_dir.join("usage");
        Self {
            cfg,
            reader: Reader::with_state(home, state_dir),
            platform,
            cache: Mutex::new(None),
        }
    }

    pub fn with_reader(cfg: Config, reader: Reader, platform: P) -> Self {
        Self {
            cfg,
            reader,
            platform,
            cache: Mutex::new(None),
        }
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn rescan(&self, now: DateTime<Utc>) -> Result<Snapshot, RuntimeError> {
        let snapshot = self.build_snapshot(now)?;
        *self.cache.lock().expect("runtime cache mutex poisoned") = Some(snapshot.clone());
        Ok(snapshot)
    }

    pub fn snapshot(&self, now: DateTime<Utc>) -> Result<Snapshot, RuntimeError> {
        if let Some(snapshot) = self
            .cache
            .lock()
            .expect("runtime cache mutex poisoned")
            .clone()
        {
            return Ok(snapshot);
        }
        self.rescan(now)
    }

    pub fn session(&self, key: &str, now: DateTime<Utc>) -> Result<SessionView, RuntimeError> {
        find_session_view(&self.snapshot(now)?, key).ok_or(ServiceError::SessionNotFound.into())
    }

    pub fn turns(
        &self,
        key: &str,
        query: TurnQuery,
        now: DateTime<Utc>,
    ) -> Result<Vec<TurnView>, RuntimeError> {
        let lookback_start =
            now - chrono::Duration::from_std(self.cfg.usage.lookback.as_std()).unwrap();
        let read_since = match query.since {
            Some(since) if since < lookback_start => since,
            _ => lookback_start,
        };
        let scan = self.reader.scan_since(Some(read_since))?;
        let canonical = service::canonical_session_key(&scan.events, key)
            .ok_or(ServiceError::SessionNotFound)?;
        let turn_since = query.since.unwrap_or(lookback_start);
        Ok(service::session_turns(
            &scan.events,
            &canonical,
            Some(turn_since),
            query.limit,
        )?)
    }

    pub fn acknowledge_session(
        &self,
        key: &str,
        request: AckRequest,
        now: DateTime<Utc>,
    ) -> Result<AckView, RuntimeError> {
        let events = self.fresh_events(now)?;
        let ack = Service::new(&self.cfg, &events, &self.platform)
            .acknowledge_session(key, request, now)?;
        let _ = self.rescan(now);
        Ok(ack)
    }

    pub fn stop_session(
        &self,
        key: &str,
        request: StopRequest,
        now: DateTime<Utc>,
    ) -> Result<StopView, RuntimeError> {
        let events = self.fresh_events(now)?;
        let stop =
            Service::new(&self.cfg, &events, &self.platform).stop_session(key, request, now)?;
        let _ = self.rescan(now);
        Ok(stop)
    }

    fn build_snapshot(&self, now: DateTime<Utc>) -> Result<Snapshot, RuntimeError> {
        let scan = self.reader.scan_since(Some(self.lookback_start(now)))?;
        let mut sources = scan.sources;
        let captured = match self.platform.capture() {
            Ok(processes) => Some(processes),
            Err(error) => {
                sources.push(capture_source_error(error));
                None
            }
        };
        Ok(service::build_snapshot_with_processes(
            &self.cfg,
            captured.as_ref(),
            &scan.events,
            sources,
            now,
        ))
    }

    fn fresh_events(&self, now: DateTime<Utc>) -> Result<Vec<crate::usage::Event>, RuntimeError> {
        Ok(self
            .reader
            .scan_since(Some(self.lookback_start(now)))?
            .events)
    }

    fn lookback_start(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        now - chrono::Duration::from_std(self.cfg.usage.lookback.as_std()).unwrap()
    }
}

fn find_session_view(snapshot: &Snapshot, key: &str) -> Option<SessionView> {
    snapshot
        .sessions
        .iter()
        .find(|session| session.key == key || session.id == key)
        .cloned()
}

fn capture_source_error(error: PlatformError) -> crate::usage::SourceReport {
    crate::usage::SourceReport {
        provider: "processes".to_string(),
        files: 0,
        events: 0,
        error: Some(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use chrono::TimeZone;
    use tempfile::TempDir;

    use super::*;
    use crate::config::{AgentKind, Config, HumanDuration, Mode};
    use crate::platform::{self, Platform, TerminationTarget};
    use crate::service::{StopExpectedIdentity, StopRequest};

    #[test]
    fn rescan_builds_correlated_snapshot_from_real_usage_logs() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 250, 250);
        let cfg = test_config(home.path(), Mode::Enforcement);
        let platform = FakePlatform::new(Ok(platform::Snapshot::new([process(
            now, 100, "codex", "/repo",
        )])));
        let runtime = Runtime::new(cfg, home.path(), platform);

        let snapshot = runtime.rescan(now).unwrap();

        assert_eq!(snapshot.overview.status, "ACTION");
        assert_eq!(snapshot.sessions[0].key, "codex:s1");
        assert_eq!(snapshot.sessions[0].correlated_pid, Some(100));
        assert!(snapshot.sessions[0].actionable);
    }

    #[test]
    fn snapshot_uses_cache_until_explicit_rescan() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 90, 90);
        let cfg = test_config(home.path(), Mode::Visibility);
        let runtime = Runtime::new(
            cfg,
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::new([process(
                now, 100, "codex", "/repo",
            )]))),
        );

        let first = runtime.snapshot(now).unwrap();
        write_codex_session(home.path(), "s2", "/repo", now, 90, 90);
        let cached = runtime.snapshot(now).unwrap();
        let rescanned = runtime.rescan(now).unwrap();

        assert_eq!(first.sessions.len(), 1);
        assert_eq!(cached.sessions.len(), 1);
        assert_eq!(rescanned.sessions.len(), 2);
    }

    #[test]
    fn turns_are_filtered_limited_and_accept_session_id_alias() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(
            home.path(),
            "s1",
            "/repo",
            now - chrono::Duration::minutes(10),
            10,
            10,
        );
        append_codex_token(
            home.path(),
            "s1",
            now - chrono::Duration::minutes(1),
            20,
            30,
        );
        append_codex_token(home.path(), "s1", now, 30, 60);
        let cfg = test_config(home.path(), Mode::Visibility);
        let runtime = Runtime::new(
            cfg,
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::new(Vec::new()))),
        );

        let turns = runtime
            .turns(
                "s1",
                TurnQuery {
                    since: Some(now - chrono::Duration::minutes(2)),
                    limit: 1,
                },
                now,
            )
            .unwrap();

        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].total_tokens, 30);
    }

    #[test]
    fn acknowledge_refreshes_snapshot_and_suppresses_actionability() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 250, 250);
        let cfg = test_config(home.path(), Mode::Enforcement);
        let runtime = Runtime::new(
            cfg,
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::new([process(
                now, 100, "codex", "/repo",
            )]))),
        );

        let ack = runtime
            .acknowledge_session(
                "s1",
                AckRequest {
                    extend_seconds: 600,
                    reason: "watching".to_string(),
                },
                now,
            )
            .unwrap();
        let snapshot = runtime.snapshot(now).unwrap();
        let events = crate::ledger::read(runtime.config().ledger.path.clone()).unwrap();

        assert_eq!(ack.session_key, "codex:s1");
        assert_eq!(ack.extend_seconds, 60);
        assert_eq!(snapshot.sessions[0].state, "acknowledged");
        assert!(!snapshot.sessions[0].actionable);
        assert_eq!(events[0].event_type, "session_ack_received");
    }

    #[test]
    fn stop_uses_fresh_usage_and_revalidates_identity() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 90, 90);
        let root = process(now, 100, "codex", "/repo");
        let runtime = Runtime::new(
            test_config(home.path(), Mode::Enforcement),
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::new([root.clone()]))),
        );
        runtime.rescan(now).unwrap();
        append_codex_token(home.path(), "s1", now, 250, 340);

        let stop = runtime
            .stop_session("s1", stop_request_for(&root), now)
            .unwrap();

        assert_eq!(stop.result, "terminated");
        assert_eq!(stop.scope_pids, vec![100]);
        assert_eq!(
            *runtime.platform.terminated.lock().unwrap(),
            vec![vec![100]]
        );
        let events = crate::ledger::read(runtime.config().ledger.path.clone()).unwrap();
        assert_eq!(events[0].event_type, "manual_stop_started");
        assert_eq!(events[1].event_type, "manual_stop_completed");
    }

    #[test]
    fn acknowledge_rolls_back_ack_file_when_ledger_append_fails() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 250, 250);
        let mut cfg = test_config(home.path(), Mode::Enforcement);
        let blocked_parent = cfg.service.state_dir.join("not-a-directory");
        fs::create_dir_all(&cfg.service.state_dir).unwrap();
        fs::write(&blocked_parent, "file").unwrap();
        cfg.ledger.path = blocked_parent.join("runs.ndjson");
        let runtime = Runtime::new(
            cfg,
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::new([process(
                now, 100, "codex", "/repo",
            )]))),
        );

        let err = runtime
            .acknowledge_session(
                "s1",
                AckRequest {
                    extend_seconds: 60,
                    reason: "watching".to_string(),
                },
                now,
            )
            .unwrap_err();

        assert!(matches!(
            err,
            RuntimeError::Service(ServiceError::Ledger(_))
        ));
        assert!(
            crate::service::read_session_ack(&runtime.config().service.state_dir, "codex:s1")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn process_capture_failure_keeps_usage_visible_and_marks_source() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 250, 250);
        let runtime = Runtime::new(
            test_config(home.path(), Mode::Enforcement),
            home.path(),
            FakePlatform::new(Err(PlatformError::Capture("ps unavailable".to_string()))),
        );

        let snapshot = runtime.rescan(now).unwrap();

        assert_eq!(snapshot.sessions[0].state, "uncorrelated");
        assert!(snapshot.overview.sources.iter().any(|source| {
            source.provider == "processes"
                && source
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains("ps unavailable"))
        }));
    }

    fn temp_home() -> TempDir {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".codex/archived_sessions")).unwrap();
        fs::create_dir_all(home.path().join(".claude/projects")).unwrap();
        home
    }

    fn test_config(home: &Path, mode: Mode) -> Config {
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = mode;
        cfg.service.state_dir = home.join(".curb");
        cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        cfg.usage.lookback = HumanDuration::hours(24);
        cfg.usage.window = HumanDuration::minutes(15);
        cfg.defaults.ack_extension = HumanDuration::seconds(60);
        cfg.agents.retain(|agent| agent.id == "codex-cli");
        cfg.agents[0].kind = AgentKind::Process;
        cfg
    }

    fn write_codex_session(
        home: &Path,
        session_id: &str,
        cwd: &str,
        at: DateTime<Utc>,
        total: i64,
        cumulative: i64,
    ) {
        let path = codex_path(home, session_id);
        let content = format!(
            r#"{{"timestamp":"{}","type":"session_meta","payload":{{"id":"{}","cwd":"{}"}}}}
{}"#,
            at.to_rfc3339(),
            session_id,
            cwd,
            codex_token_row(at, total, cumulative)
        );
        fs::write(path, content).unwrap();
    }

    fn append_codex_token(
        home: &Path,
        session_id: &str,
        at: DateTime<Utc>,
        total: i64,
        cumulative: i64,
    ) {
        use std::io::Write;

        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(codex_path(home, session_id))
            .unwrap();
        file.write_all(codex_token_row(at, total, cumulative).as_bytes())
            .unwrap();
    }

    fn codex_path(home: &Path, session_id: &str) -> PathBuf {
        home.join(".codex")
            .join("archived_sessions")
            .join(format!("{session_id}.jsonl"))
    }

    fn codex_token_row(at: DateTime<Utc>, total: i64, cumulative: i64) -> String {
        format!(
            r#"{{"timestamp":"{}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":{total}}},"total_token_usage":{{"total_tokens":{cumulative}}},"model_context_window":258400}}}}}}
"#,
            at.to_rfc3339()
        )
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
    }

    impl FakePlatform {
        fn new(capture: Result<platform::Snapshot, PlatformError>) -> Self {
            Self {
                capture,
                terminated: Mutex::new(Vec::new()),
            }
        }
    }

    impl Platform for FakePlatform {
        fn capture(&self) -> Result<platform::Snapshot, PlatformError> {
            self.capture.clone()
        }

        fn notify(&self, _title: &str, _body: &str) -> Result<(), PlatformError> {
            Ok(())
        }

        fn terminate(&self, target: &TerminationTarget) -> Result<(), PlatformError> {
            self.terminated
                .lock()
                .unwrap()
                .push(target.scope().iter().map(|pid| pid.get()).collect());
            Ok(())
        }
    }
}
