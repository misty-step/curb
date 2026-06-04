use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::config::Config;
use crate::governor::GovernorEngine;
use crate::onboarding::{self, NotificationView, OnboardingView};
use crate::platform::{Platform, PlatformError};
use crate::service::{
    self, AckRequest, AckView, AlertView, ConfigUpdate, ConfigView, EventView, ReadinessView,
    ServiceError, SessionView, Snapshot, StopRequest, StopView, TurnView,
};
use crate::usage::{Reader, UsageError};
use crate::usagewatch::UsageWatchError;
use crate::write_path::Service;

mod cache;
mod config_store;
mod readiness;
mod usage_tick;
mod watcher;

use cache::SnapshotCache;
use config_store::ConfigStore;
use readiness::SnapshotCacheStatus;
pub use usage_tick::UsageTickReport;
pub use watcher::WatcherHandle;

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

pub struct Runtime<P: Platform> {
    config: ConfigStore,
    reader: Reader,
    platform: P,
    cache: SnapshotCache,
    notification: Mutex<Option<NotificationView>>,
    governor: Mutex<GovernorEngine>,
}

impl<P> Runtime<P>
where
    P: Platform + Send + Sync + 'static,
{
    pub fn start_usage_watcher(self: Arc<Self>) -> WatcherHandle {
        self.start_usage_watcher_with_observer(|_, _| {})
    }

    pub fn start_usage_watcher_with_observer(
        self: Arc<Self>,
        observe: impl FnMut(Result<&Snapshot, &RuntimeError>, std::time::Duration) + Send + 'static,
    ) -> WatcherHandle {
        self.start_usage_watcher_with_report_observer(snapshot_observer(observe))
    }

    pub fn start_usage_watcher_with_report_observer(
        self: Arc<Self>,
        observe: impl FnMut(Result<&UsageTickReport, &RuntimeError>, std::time::Duration)
        + Send
        + 'static,
    ) -> WatcherHandle {
        let interval_runtime = Arc::clone(&self);
        watcher::start_usage_watcher(
            move || interval_runtime.config().usage.scan_interval.as_std(),
            move |now| self.usage_tick_report(now),
            observe,
        )
    }
}

fn snapshot_observer(
    mut observe: impl FnMut(Result<&Snapshot, &RuntimeError>, std::time::Duration) + Send + 'static,
) -> impl FnMut(Result<&UsageTickReport, &RuntimeError>, std::time::Duration) + Send + 'static {
    move |result, duration| match result {
        Ok(report) => observe(Ok(&report.snapshot), duration),
        Err(error) => observe(Err(error), duration),
    }
}

impl<P: Platform> Runtime<P> {
    pub fn new(cfg: Config, home: impl Into<PathBuf>, platform: P) -> Self {
        let state_dir = cfg.service.state_dir.join("usage");
        Self {
            config: ConfigStore::new(cfg),
            reader: Reader::with_state(home, state_dir),
            platform,
            cache: SnapshotCache::new(),
            notification: Mutex::new(None),
            governor: Mutex::new(GovernorEngine::default()),
        }
    }

    pub fn with_reader(cfg: Config, reader: Reader, platform: P) -> Self {
        Self {
            config: ConfigStore::new(cfg),
            reader,
            platform,
            cache: SnapshotCache::new(),
            notification: Mutex::new(None),
            governor: Mutex::new(GovernorEngine::default()),
        }
    }

    pub fn with_config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config = self.config.with_path(path);
        self
    }

    pub fn config(&self) -> Config {
        self.config.get()
    }

    pub fn config_view(&self) -> Result<ConfigView, RuntimeError> {
        Ok(self.config.view())
    }

    pub fn readiness(&self) -> ReadinessView {
        let cfg = self.config();
        let notifications = self.notification_health();
        let termination = self.platform.termination_capability();
        readiness::readiness_view(
            &cfg,
            notifications,
            termination,
            self.snapshot_cache_status(),
        )
    }

    pub fn update_config(&self, update: ConfigUpdate) -> Result<ConfigView, RuntimeError> {
        let view = self.config.update(update)?;
        self.cache.clear();
        Ok(view)
    }

    pub fn onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, RuntimeError> {
        let cfg = self.config();
        let config = self.config.view();
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
        self.cache.refresh(|| self.build_snapshot(now))
    }

    pub fn usage_scan(&self, now: DateTime<Utc>) -> Result<Snapshot, RuntimeError> {
        let cfg = self.config();
        usage_tick::usage_scan(
            &cfg,
            &self.reader,
            &self.platform,
            &self.governor,
            now,
            || self.rescan(now),
        )
    }

    pub fn usage_tick(&self, now: DateTime<Utc>) -> Result<Snapshot, RuntimeError> {
        let cfg = self.config();
        usage_tick::usage_tick(
            &cfg,
            &self.reader,
            &self.platform,
            &self.governor,
            now,
            || self.rescan(now),
        )
    }

    pub fn usage_tick_report(&self, now: DateTime<Utc>) -> Result<UsageTickReport, RuntimeError> {
        let cfg = self.config();
        usage_tick::usage_tick_report(
            &cfg,
            &self.reader,
            &self.platform,
            &self.governor,
            now,
            || self.rescan(now),
        )
    }

    pub fn snapshot(&self, now: DateTime<Utc>) -> Result<Snapshot, RuntimeError> {
        if let Some(snapshot) = self.cache.get() {
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
        let lookback_start = usage_tick::lookback_start(&cfg, now);
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
        let cfg = self.config();
        let scan = self
            .reader
            .scan_since(Some(usage_tick::lookback_start(&cfg, now)))?;
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
            .governor
            .lock()
            .expect("governor mutex poisoned")
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
        let cfg = self.config();
        Ok(self
            .reader
            .scan_since(Some(usage_tick::lookback_start(&cfg, now)))?
            .events)
    }

    fn snapshot_cache_status(&self) -> SnapshotCacheStatus {
        self.cache.status()
    }

    fn record_notification(&self, view: NotificationView) {
        *self
            .notification
            .lock()
            .expect("notification mutex poisoned") = Some(view);
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
mod tests;
