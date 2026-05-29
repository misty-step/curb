use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::config::Config;
use crate::onboarding::{self, NotificationView, OnboardingView};
use crate::platform::{Platform, PlatformError};
use crate::service::{
    self, AckRequest, AckView, AlertView, ConfigUpdate, ConfigView, EventView, ServiceError,
    SessionView, Snapshot, StopRequest, StopView, TurnView,
};
use crate::usage::{Reader, UsageError};
use crate::usagewatch::{UsageWatch, UsageWatchError};
use crate::write_path::Service;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("snapshot unavailable")]
    SnapshotUnavailable,
    #[error(transparent)]
    Usage(#[from] UsageError),
    #[error(transparent)]
    UsageWatch(#[from] UsageWatchError),
    #[error(transparent)]
    Service(#[from] ServiceError),
    #[error(transparent)]
    Platform(#[from] PlatformError),
    #[error(transparent)]
    Ledger(#[from] crate::ledger::LedgerError),
    #[error("config path is unavailable")]
    ConfigPathUnavailable,
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    #[error("local notifications are disabled")]
    NotificationsDisabled(NotificationView),
    #[error("local notifications are unavailable")]
    NotificationsUnavailable(NotificationView),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TurnQuery {
    pub since: Option<DateTime<Utc>>,
    pub limit: usize,
}

pub struct WatcherHandle {
    shutdown: Arc<WatcherShutdown>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct WatcherShutdown {
    stopped: Mutex<bool>,
    changed: Condvar,
}

impl WatcherHandle {
    pub fn request_shutdown(&self) {
        let mut stopped = self
            .shutdown
            .stopped
            .lock()
            .expect("watcher mutex poisoned");
        *stopped = true;
        self.shutdown.changed.notify_all();
    }

    pub fn join(mut self) -> thread::Result<()> {
        self.request_shutdown();
        self.thread.take().map_or(Ok(()), JoinHandle::join)
    }
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

pub struct Runtime<P: Platform> {
    cfg: Mutex<Config>,
    config_path: Option<PathBuf>,
    reader: Reader,
    platform: P,
    cache: Mutex<Option<Snapshot>>,
    notification: Mutex<Option<NotificationView>>,
    usagewatch: Mutex<UsageWatch>,
}

impl<P> Runtime<P>
where
    P: Platform + Send + Sync + 'static,
{
    pub fn start_usage_watcher(self: Arc<Self>) -> WatcherHandle {
        let shutdown = Arc::new(WatcherShutdown::default());
        let thread_shutdown = Arc::clone(&shutdown);
        let thread = thread::spawn(move || {
            loop {
                if thread_shutdown.wait_timeout(self.config().usage.scan_interval.as_std()) {
                    return;
                }
                if let Err(error) = self.usage_tick(Utc::now()) {
                    eprintln!("curb: usage scan failed: {error:#}");
                }
            }
        });
        WatcherHandle {
            shutdown,
            thread: Some(thread),
        }
    }
}

impl WatcherShutdown {
    fn wait_timeout(&self, interval: std::time::Duration) -> bool {
        let stopped = self.stopped.lock().expect("watcher mutex poisoned");
        if *stopped {
            return true;
        }
        let (stopped, _) = self
            .changed
            .wait_timeout(stopped, interval)
            .expect("watcher condvar poisoned");
        *stopped
    }
}

impl<P: Platform> Runtime<P> {
    pub fn new(cfg: Config, home: impl Into<PathBuf>, platform: P) -> Self {
        let state_dir = cfg.service.state_dir.join("usage");
        Self {
            cfg: Mutex::new(cfg),
            config_path: None,
            reader: Reader::with_state(home, state_dir),
            platform,
            cache: Mutex::new(None),
            notification: Mutex::new(None),
            usagewatch: Mutex::new(UsageWatch::default()),
        }
    }

    pub fn with_reader(cfg: Config, reader: Reader, platform: P) -> Self {
        Self {
            cfg: Mutex::new(cfg),
            config_path: None,
            reader,
            platform,
            cache: Mutex::new(None),
            notification: Mutex::new(None),
            usagewatch: Mutex::new(UsageWatch::default()),
        }
    }

    pub fn with_config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_path = Some(path.into());
        self
    }

    pub fn config(&self) -> Config {
        self.cfg.lock().expect("config mutex poisoned").clone()
    }

    pub fn config_view(&self) -> Result<ConfigView, RuntimeError> {
        let cfg = self.config();
        Ok(service::config_view(self.config_path.as_deref(), &cfg))
    }

    pub fn update_config(&self, update: ConfigUpdate) -> Result<ConfigView, RuntimeError> {
        let path = self
            .config_path
            .as_ref()
            .ok_or(RuntimeError::ConfigPathUnavailable)?;
        let mut next = self.config();
        service::apply_config_update(&mut next, update)?;
        next.save(path)?;
        *self.cfg.lock().expect("config mutex poisoned") = next.clone();
        *self.cache.lock().expect("runtime cache mutex poisoned") = None;
        Ok(service::config_view(Some(path), &next))
    }

    pub fn onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, RuntimeError> {
        let cfg = self.config();
        let config = service::config_view(self.config_path.as_deref(), &cfg);
        let required = !onboarding_completed(&cfg.service.state_dir);
        let notifications = self.notification_health()?;
        let termination = self.platform.termination_capability();
        let snapshot = self.snapshot(now)?;
        Ok(onboarding::onboarding_view(
            config,
            required,
            notifications,
            termination,
            snapshot,
        ))
    }

    pub fn complete_onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, RuntimeError> {
        let cfg = self.config();
        write_onboarding_marker(&cfg.service.state_dir)?;
        self.onboarding(now)
    }

    pub fn rescan(&self, now: DateTime<Utc>) -> Result<Snapshot, RuntimeError> {
        let mut cache = self.cache.lock().expect("runtime cache mutex poisoned");
        let snapshot = service::annotate_overview_delta(cache.as_ref(), self.build_snapshot(now)?);
        *cache = Some(snapshot.clone());
        Ok(snapshot)
    }

    pub fn usage_scan(&self, now: DateTime<Utc>) -> Result<Snapshot, RuntimeError> {
        let scan = self.reader.scan_since(Some(self.lookback_start(now)))?;
        let processes = self.platform.capture()?;
        let cfg = self.config();
        self.usagewatch
            .lock()
            .expect("usage watcher mutex poisoned")
            .scan(&cfg, &scan.events, &processes, &self.platform, now)?;
        self.rescan(now)
    }

    pub fn usage_tick(&self, now: DateTime<Utc>) -> Result<Snapshot, RuntimeError> {
        match self.usage_scan(now) {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                self.append_runtime_event(
                    crate::ledger::Event::new(crate::ledger::LedgerEvent::UsageScanFailed.as_str())
                        .with_message(error.to_string()),
                )?;
                self.rescan(now)
            }
        }
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
        let cfg = self.config();
        let lookback_start = now - chrono::Duration::from_std(cfg.usage.lookback.as_std()).unwrap();
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

    pub fn events(&self, limit: usize) -> Result<Vec<EventView>, RuntimeError> {
        let cfg = self.config();
        let events = crate::ledger::read(&cfg.ledger.path)?;
        Ok(service::event_views(&events, limit))
    }

    pub fn alerts(&self, limit: usize, now: DateTime<Utc>) -> Result<Vec<AlertView>, RuntimeError> {
        let cfg = self.config();
        let events = crate::ledger::read(&cfg.ledger.path)?;
        let snapshot = self.snapshot(now).ok();
        Ok(service::alert_views(&events, snapshot.as_ref(), limit))
    }

    pub fn acknowledge_session(
        &self,
        key: &str,
        request: AckRequest,
        now: DateTime<Utc>,
    ) -> Result<AckView, RuntimeError> {
        let events = self.fresh_events(now)?;
        let cfg = self.config();
        let ack =
            Service::new(&cfg, &events, &self.platform).acknowledge_session(key, request, now)?;
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
        let cfg = self.config();
        let stop = Service::new(&cfg, &events, &self.platform).stop_session(key, request, now)?;
        let _ = self.rescan(now);
        Ok(stop)
    }

    pub fn notification_health(&self) -> Result<NotificationView, RuntimeError> {
        let cfg = self.config();
        Ok(onboarding::notification_view(
            cfg.alerts.local_notifications,
            self.platform.notification_capability(),
            self.notification
                .lock()
                .expect("notification mutex poisoned")
                .clone(),
        ))
    }

    pub fn test_notification(&self, now: DateTime<Utc>) -> Result<NotificationView, RuntimeError> {
        let cfg = self.config();
        let mut view = onboarding::notification_view(
            cfg.alerts.local_notifications,
            self.platform.notification_capability(),
            self.notification
                .lock()
                .expect("notification mutex poisoned")
                .clone(),
        );
        if !view.enabled {
            self.record_notification(view.clone());
            return Err(RuntimeError::NotificationsDisabled(view));
        }
        if !view.available {
            self.record_notification(view.clone());
            return Err(RuntimeError::NotificationsUnavailable(view));
        }
        match self.platform.notify(
            "Curb notification test",
            "Curb can deliver local agent alerts.",
        ) {
            Ok(()) => {
                view.status = "delivered".to_string();
                view.message = "test notification delivered".to_string();
                view.last_test_at = Some(now);
                self.record_notification(view.clone());
                Ok(view)
            }
            Err(error) => {
                let message = error.to_string();
                view.status = "error".to_string();
                view.message = message.clone();
                view.available = false;
                view.last_error = Some(message);
                view.last_test_at = Some(now);
                self.record_notification(view.clone());
                Err(RuntimeError::NotificationsUnavailable(view))
            }
        }
    }

    fn build_snapshot(&self, now: DateTime<Utc>) -> Result<Snapshot, RuntimeError> {
        let scan = self.reader.scan_since(Some(self.lookback_start(now)))?;
        let cfg = self.config();
        let mut sources = scan.sources;
        let mut capture_error = None;
        let captured = match self.platform.capture() {
            Ok(processes) => Some(processes),
            Err(error) => {
                sources.push(capture_source_error(error.clone()));
                capture_error = Some(error);
                None
            }
        };
        let terminated = self
            .usagewatch
            .lock()
            .expect("usage watcher mutex poisoned")
            .terminated_keys();
        let mut snapshot = service::build_snapshot_filtered(
            &cfg,
            captured.as_ref(),
            &scan.events,
            sources,
            now,
            &terminated,
        );
        snapshot.overview.capabilities = onboarding::platform_capabilities(
            &cfg,
            captured.as_ref(),
            capture_error.as_ref(),
            self.notification_health()?,
            self.platform.termination_capability(),
            &snapshot.agents,
        );
        Ok(snapshot)
    }

    fn fresh_events(&self, now: DateTime<Utc>) -> Result<Vec<crate::usage::Event>, RuntimeError> {
        Ok(self
            .reader
            .scan_since(Some(self.lookback_start(now)))?
            .events)
    }

    fn lookback_start(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        let cfg = self.config();
        now - chrono::Duration::from_std(cfg.usage.lookback.as_std()).unwrap()
    }

    fn record_notification(&self, view: NotificationView) {
        *self
            .notification
            .lock()
            .expect("notification mutex poisoned") = Some(view);
    }

    fn append_runtime_event(&self, mut event: crate::ledger::Event) -> Result<(), RuntimeError> {
        let cfg = self.config();
        event.mode = Some(cfg.mode.to_string());
        crate::ledger::Ledger::open(&cfg.ledger.path)?.append(event)?;
        Ok(())
    }
}

fn onboarding_completed(state_dir: &Path) -> bool {
    onboarding_marker_path(state_dir).is_file()
}

fn write_onboarding_marker(state_dir: &Path) -> Result<(), ServiceError> {
    fs::create_dir_all(state_dir).map_err(|source| ServiceError::Io {
        path: state_dir.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(state_dir, fs::Permissions::from_mode(0o700)).map_err(|source| {
            ServiceError::Io {
                path: state_dir.to_path_buf(),
                source,
            }
        })?;
    }
    let path = onboarding_marker_path(state_dir);
    fs::write(&path, b"complete\n").map_err(|source| ServiceError::Io {
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
    Ok(())
}

fn onboarding_marker_path(state_dir: &Path) -> PathBuf {
    state_dir.join("onboarding.complete")
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
    use std::sync::{Arc, Mutex};

    use chrono::TimeZone;
    use serde_json::{Map, Value};
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
        assert_eq!(snapshot.sessions[0].pid, Some(100));
        assert!(snapshot.sessions[0].can_stop);
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
        assert_eq!(first.overview.changes, Default::default());
        assert_eq!(cached.sessions.len(), 1);
        assert_eq!(rescanned.sessions.len(), 2);
        assert_eq!(rescanned.overview.changes.new_sessions, 1);
        assert_eq!(rescanned.overview.changes.sessions_with_new_turns, 1);
        assert_eq!(rescanned.overview.changes.tokens_added, 90);
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
        assert!(snapshot.sessions[0].acknowledged_until.is_some());
        assert!(!snapshot.sessions[0].can_stop);
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

        assert_eq!(stop.result.soft_signaled, vec![100]);
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
    fn notification_health_and_test_record_delivery_state() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        let runtime = Runtime::new(
            test_config(home.path(), Mode::Alert),
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::default())),
        );

        let health = runtime.notification_health().unwrap();
        assert!(health.enabled);
        assert!(health.available);
        assert_eq!(health.status, "ready");

        let tested = runtime.test_notification(now).unwrap();
        assert_eq!(tested.status, "delivered");
        assert_eq!(tested.last_test_at, Some(now));
        assert_eq!(
            runtime.platform.notifications.lock().unwrap().as_slice(),
            &[(
                "Curb notification test".to_string(),
                "Curb can deliver local agent alerts.".to_string()
            )]
        );

        let health = runtime.notification_health().unwrap();
        assert_eq!(health.status, "delivered");
        assert_eq!(health.last_test_at, Some(now));
    }

    #[test]
    fn notification_health_keeps_last_test_but_respects_current_capability() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        let mut runtime = Runtime::new(
            test_config(home.path(), Mode::Alert),
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::default())),
        );
        runtime.test_notification(now).unwrap();

        runtime.platform.capability = platform::NotificationCapability {
            supported: false,
            status: "unavailable".to_string(),
            message: "notify-send not found".to_string(),
        };

        let health = runtime.notification_health().unwrap();
        assert!(!health.available);
        assert_eq!(health.status, "unavailable");
        assert_eq!(health.last_test_at, Some(now));
    }

    #[test]
    fn notification_test_reports_disabled_unavailable_and_delivery_errors() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        let mut disabled_cfg = test_config(home.path(), Mode::Alert);
        disabled_cfg.alerts.local_notifications = false;
        let disabled = Runtime::new(
            disabled_cfg,
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::default())),
        );
        let err = disabled.test_notification(now).unwrap_err();
        assert!(matches!(err, RuntimeError::NotificationsDisabled(_)));

        let unavailable = Runtime::new(
            test_config(home.path(), Mode::Alert),
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::default())).with_notifications_disabled(),
        );
        let err = unavailable.test_notification(now).unwrap_err();
        assert!(matches!(err, RuntimeError::NotificationsUnavailable(_)));

        let failing = Runtime::new(
            test_config(home.path(), Mode::Alert),
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::default())).with_notify_error("denied"),
        );
        let err = failing.test_notification(now).unwrap_err();
        let RuntimeError::NotificationsUnavailable(view) = err else {
            panic!("unexpected error");
        };
        assert_eq!(view.status, "error");
        assert_eq!(
            view.last_error.as_deref(),
            Some("notification failed: denied")
        );
    }

    #[test]
    fn update_config_persists_validated_config_and_clears_snapshot_cache() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 250, 250);
        let config_path = home.path().join("config.yaml");
        let cfg = test_config(home.path(), Mode::Alert);
        fs::write(&config_path, serde_yaml::to_string(&cfg).unwrap()).unwrap();
        let runtime = Runtime::new(
            cfg,
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::new([process(
                now, 100, "codex", "/repo",
            )]))),
        )
        .with_config_path(&config_path);
        let cached = runtime.snapshot(now).unwrap();
        assert_eq!(cached.overview.mode, "watch");

        let view = runtime
            .update_config(ConfigUpdate {
                mode: Some("visibility".to_string()),
                warn_turn_tokens: Some(2_000),
                kill_turn_tokens: Some(4_000),
                usage_window_seconds: Some(120),
                local_notifications: Some(false),
                ..ConfigUpdate::default()
            })
            .unwrap();

        assert_eq!(view.mode, "visibility");
        assert_eq!(view.warn_turn_tokens, 2_000);
        assert!(!view.local_notifications);
        let reloaded = Config::load(&config_path).unwrap();
        assert_eq!(reloaded.mode, Mode::Visibility);
        assert_eq!(reloaded.usage.warn_turn_tokens, 2_000);
        assert_eq!(reloaded.usage.window.as_std().as_secs(), 120);
        assert_eq!(runtime.snapshot(now).unwrap().overview.mode, "watch");
    }

    #[test]
    fn update_config_rejects_invalid_values_without_persisting() {
        let home = temp_home();
        let config_path = home.path().join("config.yaml");
        let cfg = test_config(home.path(), Mode::Alert);
        fs::write(&config_path, serde_yaml::to_string(&cfg).unwrap()).unwrap();
        let runtime = Runtime::new(
            cfg,
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::default())),
        )
        .with_config_path(&config_path);

        let err = runtime
            .update_config(ConfigUpdate {
                warn_turn_tokens: Some(5_000),
                kill_turn_tokens: Some(4_000),
                ..ConfigUpdate::default()
            })
            .unwrap_err();

        assert!(err.to_string().contains("usage.warn_turn_tokens"));
        let reloaded = Config::load(&config_path).unwrap();
        assert_eq!(reloaded.usage.warn_turn_tokens, 100);
        assert_eq!(runtime.config().usage.warn_turn_tokens, 100);
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

        assert_eq!(snapshot.sessions[0].pid, None);
        assert_eq!(
            snapshot.overview.capabilities.process_capture.status,
            "error"
        );
        assert_eq!(
            snapshot.overview.capabilities.process_identity.status,
            "error"
        );
        assert_eq!(snapshot.overview.capabilities.enforcement.status, "blocked");
        assert!(snapshot.overview.sources.iter().any(|source| {
            source.provider == "processes"
                && source
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains("ps unavailable"))
        }));
    }

    #[test]
    fn onboarding_projects_first_run_state_and_completion_marker() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 90, 90);
        let config_path = home.path().join("config.yaml");
        let cfg = test_config(home.path(), Mode::Alert);
        fs::write(&config_path, serde_yaml::to_string(&cfg).unwrap()).unwrap();
        let runtime = Runtime::new(
            cfg,
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::new([process(
                now, 100, "codex", "/repo",
            )]))),
        )
        .with_config_path(&config_path);

        let view = runtime.onboarding(now).unwrap();

        assert!(view.required);
        assert_eq!(
            view.config_path.as_deref(),
            Some(config_path.to_str().unwrap())
        );
        assert_eq!(view.mode, "alert");
        assert!(!view.mode_can_terminate);
        assert_eq!(view.enforceable_agent_types, 1);
        assert!(view.detected_providers.contains(&"codex".to_string()));
        assert!(view.detected_workers.contains(&"Codex CLI".to_string()));
        assert_eq!(view.capabilities.process_capture.status, "ready");
        assert_eq!(view.capabilities.enforcement.status, "disabled");
        assert!(view.steps.iter().any(|step| {
            step.id == "sources" && step.status == "done" && step.message.contains("usage event")
        }));

        let completed = runtime.complete_onboarding(now).unwrap();

        assert!(!completed.required);
        let marker = runtime
            .config()
            .service
            .state_dir
            .join("onboarding.complete");
        assert_eq!(fs::read_to_string(&marker).unwrap(), "complete\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&marker).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn overview_capabilities_report_enforcement_ready_only_when_revalidatable() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 250, 250);
        let runtime = Runtime::new(
            test_config(home.path(), Mode::Enforcement),
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::new([process(
                now, 100, "codex", "/repo",
            )]))),
        );

        let snapshot = runtime.rescan(now).unwrap();

        assert_eq!(snapshot.overview.mode, "enforce");
        assert_eq!(
            snapshot.overview.capabilities.process_capture.status,
            "ready"
        );
        assert_eq!(
            snapshot.overview.capabilities.process_identity.status,
            "ready"
        );
        assert!(snapshot.overview.capabilities.enforcement.available);
        assert_eq!(snapshot.overview.capabilities.enforcement.status, "ready");
    }

    #[test]
    fn runtime_projects_events_and_alerts_from_ledger_with_live_session_affordance() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 150, 150);
        let runtime = Runtime::new(
            test_config(home.path(), Mode::Alert),
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::new([process(
                now, 100, "codex", "/repo",
            )]))),
        );
        let cfg = runtime.config();
        let ledger = crate::ledger::Ledger::open(&cfg.ledger.path).unwrap();
        ledger
            .append(crate::ledger::Event::new("run_started"))
            .unwrap();
        ledger
            .append(
                crate::ledger::Event::new("usage_warning")
                    .with_data(alert_data("codex", "s1", "/repo")),
            )
            .unwrap();
        ledger
            .append(crate::ledger::Event::new("usage_would_terminate"))
            .unwrap();

        let events = runtime.events(2).unwrap();
        let alerts = runtime.alerts(10, now).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, "warning");
        assert_eq!(events[1].kind, "would_stop");
        assert_eq!(alerts.len(), 2);
        assert_eq!(alerts[0].session_key.as_deref(), Some("codex:s1"));
        assert!(alerts[0].can_acknowledge);
        assert_eq!(alerts[1].category, "would_stop");
    }

    #[test]
    fn usage_scan_in_alert_mode_warns_and_would_stop_without_terminating() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 250, 250);
        let runtime = Runtime::new(
            test_config(home.path(), Mode::Alert),
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::new([process(
                now, 100, "codex", "/repo",
            )]))),
        );

        runtime.usage_scan(now).unwrap();

        let events = crate::ledger::read(runtime.config().ledger.path).unwrap();
        assert_eq!(
            event_types(&events),
            ["usage_warning", "usage_would_terminate"]
        );
        assert!(runtime.platform.terminated.lock().unwrap().is_empty());
        assert_eq!(
            notification_titles(&runtime.platform),
            ["Curb usage warning", "Curb would stop agent"]
        );
    }

    #[test]
    fn usage_scan_enforces_only_after_grace_and_revalidated_identity() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 250, 250);
        let mut cfg = test_config(home.path(), Mode::Enforcement);
        cfg.usage.grace_period = HumanDuration::seconds(1);
        let runtime = Runtime::new(
            cfg,
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::new([process(
                now, 100, "codex", "/repo",
            )]))),
        );

        runtime.usage_scan(now).unwrap();
        assert!(runtime.platform.terminated.lock().unwrap().is_empty());
        runtime
            .usage_scan(now + chrono::Duration::seconds(2))
            .unwrap();

        let events = crate::ledger::read(runtime.config().ledger.path).unwrap();
        assert_eq!(
            event_types(&events),
            [
                "usage_warning",
                "usage_grace_started",
                "usage_termination_started",
                "usage_termination_completed"
            ]
        );
        assert_eq!(
            *runtime.platform.terminated.lock().unwrap(),
            vec![vec![100]]
        );
    }

    #[test]
    fn usage_scan_ack_suppresses_then_allows_warning_after_expiry() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 250, 250);
        let cfg = test_config(home.path(), Mode::Enforcement);
        crate::write_path::write_session_ack(
            &cfg.service.state_dir,
            "codex:s1",
            std::time::Duration::from_secs(60),
            "still supervising",
            now,
        )
        .unwrap();
        let runtime = Runtime::new(
            cfg,
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::new([process(
                now, 100, "codex", "/repo",
            )]))),
        );

        runtime.usage_scan(now).unwrap();
        assert!(
            crate::ledger::read(runtime.config().ledger.path.clone())
                .unwrap()
                .is_empty()
        );

        runtime
            .usage_scan(now + chrono::Duration::seconds(61))
            .unwrap();

        let events = crate::ledger::read(runtime.config().ledger.path).unwrap();
        assert_eq!(
            event_types(&events),
            ["usage_warning", "usage_grace_started"]
        );
    }

    #[test]
    fn usage_scan_rejects_pid_reuse_before_automatic_termination() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 250, 250);
        let mut cfg = test_config(home.path(), Mode::Enforcement);
        cfg.usage.grace_period = HumanDuration::seconds(1);
        let original = process(now, 100, "codex", "/repo");
        let mut reused = process(now, 100, "codex", "/repo");
        reused.started_at = Some(now + chrono::Duration::seconds(1));
        let runtime = Runtime::new(
            cfg,
            home.path(),
            FakePlatform::with_captures(vec![
                Ok(platform::Snapshot::new([original.clone()])),
                Ok(platform::Snapshot::new([original])),
                Ok(platform::Snapshot::new([reused.clone()])),
                Ok(platform::Snapshot::new([reused])),
            ]),
        );

        runtime.usage_scan(now).unwrap();
        runtime
            .usage_scan(now + chrono::Duration::seconds(2))
            .unwrap();

        let events = crate::ledger::read(runtime.config().ledger.path).unwrap();
        assert_eq!(
            event_types(&events),
            [
                "usage_warning",
                "usage_grace_started",
                "usage_termination_failed"
            ]
        );
        assert!(runtime.platform.terminated.lock().unwrap().is_empty());
    }

    #[test]
    fn usage_tick_records_scan_failures_without_losing_visibility() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        write_codex_session(home.path(), "s1", "/repo", now, 250, 250);
        let runtime = Runtime::new(
            test_config(home.path(), Mode::Alert),
            home.path(),
            FakePlatform::new(Err(PlatformError::Capture("ps unavailable".to_string()))),
        );

        let snapshot = runtime.usage_tick(now).unwrap();

        assert_eq!(snapshot.sessions[0].pid, None);
        let events = crate::ledger::read(runtime.config().ledger.path).unwrap();
        assert_eq!(event_types(&events), ["usage_scan_failed"]);
        assert!(
            events[0]
                .message
                .as_deref()
                .is_some_and(|message| message.contains("ps unavailable"))
        );
    }

    #[test]
    fn usage_watcher_handle_shuts_down_without_waiting_for_scan_interval() {
        let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
        let home = temp_home();
        let mut cfg = test_config(home.path(), Mode::Alert);
        cfg.usage.scan_interval = HumanDuration::hours(1);
        let runtime = Arc::new(Runtime::new(
            cfg,
            home.path(),
            FakePlatform::new(Ok(platform::Snapshot::new([process(
                now, 100, "codex", "/repo",
            )]))),
        ));

        let watcher = Arc::clone(&runtime).start_usage_watcher();
        watcher.request_shutdown();

        watcher.join().unwrap();
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
            r#"{{"timestamp":"{}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{total},"cached_input_tokens":0,"output_tokens":0,"reasoning_output_tokens":0,"total_tokens":{total}}},"total_token_usage":{{"total_tokens":{cumulative}}},"model_context_window":258400}}}}}}
"#,
            at.to_rfc3339()
        )
    }

    fn alert_data(provider: &str, session_id: &str, cwd: &str) -> Map<String, Value> {
        let mut data = Map::new();
        data.insert("provider".to_string(), Value::String(provider.to_string()));
        data.insert(
            "session_id".to_string(),
            Value::String(session_id.to_string()),
        );
        data.insert("cwd".to_string(), Value::String(cwd.to_string()));
        data
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

    fn event_types(events: &[crate::ledger::Event]) -> Vec<&str> {
        events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect()
    }

    fn notification_titles(platform: &FakePlatform) -> Vec<String> {
        platform
            .notifications
            .lock()
            .unwrap()
            .iter()
            .map(|(title, _)| title.clone())
            .collect()
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
        captures: Mutex<Vec<Result<platform::Snapshot, PlatformError>>>,
        capability: platform::NotificationCapability,
        notifications: Mutex<Vec<(String, String)>>,
        notify_error: Option<String>,
        terminated: Mutex<Vec<Vec<i32>>>,
    }

    impl FakePlatform {
        fn new(capture: Result<platform::Snapshot, PlatformError>) -> Self {
            Self {
                capture,
                captures: Mutex::new(Vec::new()),
                capability: platform::NotificationCapability {
                    supported: true,
                    status: "available".to_string(),
                    message: "available".to_string(),
                },
                notifications: Mutex::new(Vec::new()),
                notify_error: None,
                terminated: Mutex::new(Vec::new()),
            }
        }

        fn with_captures(captures: Vec<Result<platform::Snapshot, PlatformError>>) -> Self {
            Self {
                capture: Ok(platform::Snapshot::default()),
                captures: Mutex::new(captures.into_iter().rev().collect()),
                capability: platform::NotificationCapability {
                    supported: true,
                    status: "available".to_string(),
                    message: "available".to_string(),
                },
                notifications: Mutex::new(Vec::new()),
                notify_error: None,
                terminated: Mutex::new(Vec::new()),
            }
        }

        fn with_notifications_disabled(mut self) -> Self {
            self.capability = platform::NotificationCapability {
                supported: false,
                status: "unavailable".to_string(),
                message: "notify-send not found".to_string(),
            };
            self
        }

        fn with_notify_error(mut self, message: &str) -> Self {
            self.notify_error = Some(message.to_string());
            self
        }
    }

    impl Platform for FakePlatform {
        fn capture(&self) -> Result<platform::Snapshot, PlatformError> {
            if let Some(capture) = self.captures.lock().unwrap().pop() {
                return capture;
            }
            self.capture.clone()
        }

        fn notification_capability(&self) -> platform::NotificationCapability {
            self.capability.clone()
        }

        fn termination_capability(&self) -> platform::TerminationCapability {
            platform::TerminationCapability {
                supported: true,
                status: "available".to_string(),
                message: "test platform can terminate process trees".to_string(),
            }
        }

        fn notify(&self, title: &str, body: &str) -> Result<(), PlatformError> {
            if let Some(error) = &self.notify_error {
                return Err(PlatformError::Notify(error.clone()));
            }
            self.notifications
                .lock()
                .unwrap()
                .push((title.to_string(), body.to_string()));
            Ok(())
        }

        fn terminate(
            &self,
            target: &TerminationTarget,
            _grace: std::time::Duration,
        ) -> platform::TerminationResult {
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
