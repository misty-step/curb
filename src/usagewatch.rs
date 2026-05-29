use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use thiserror::Error;

use crate::config::{Config, Mode};
use crate::ledger::{self, Ledger, LedgerEvent};
use crate::platform::{self, Platform};
use crate::service::{self, ServiceError};
use crate::usage::Event as UsageEvent;

#[derive(Debug, Error)]
pub enum UsageWatchError {
    #[error(transparent)]
    Ledger(#[from] ledger::LedgerError),
    #[error(transparent)]
    Service(#[from] ServiceError),
    #[error(transparent)]
    Platform(#[from] platform::PlatformError),
}

#[derive(Clone, Debug, Default)]
pub struct UsageWatch {
    warned: HashSet<String>,
    grace: HashMap<String, DateTime<Utc>>,
    targets: HashMap<String, platform::Process>,
    /// Sessions whose worker Curb has terminated, keyed to the kill time. The
    /// read model drops these rows so a killed agent leaves the dashboard at
    /// once instead of lingering on log recency, and the scan stops re-warning
    /// or re-killing them. Cleared when the session resumes (new activity after
    /// the kill) or ages out of the window.
    terminated: HashMap<String, DateTime<Utc>>,
}

impl UsageWatch {
    pub fn scan<P: Platform>(
        &mut self,
        cfg: &Config,
        events: &[UsageEvent],
        processes: &platform::Snapshot,
        platform: &P,
        now: DateTime<Utc>,
    ) -> Result<(), UsageWatchError> {
        if !cfg.usage.enabled() {
            return Ok(());
        }
        let window_start = now - chrono::Duration::from_std(cfg.usage.window.as_std()).unwrap();
        let sessions = service::build_sessions(events, window_start);
        if sessions.is_empty() {
            self.clear();
            return Ok(());
        }
        // Forget kills that have aged out of the window — beyond it the session
        // is a finished run the read model drops on recency anyway.
        self.terminated
            .retain(|_, killed_at| *killed_at >= window_start);
        let matches = service::process_matches(cfg, processes);
        let mut active_keys = BTreeSet::new();
        for session in sessions {
            if !session.recent_usage(window_start)
                || session.latest_spent_tokens < cfg.usage.warn_turn_tokens
            {
                self.suppress(&session.key);
                continue;
            }
            // A killed session that has logged no new activity is still dead —
            // skip it so Curb does not re-warn, re-kill, or spam "stop blocked".
            // Fresh activity after the kill means it came back: clear and re-arm.
            if let Some(killed_at) = self.terminated.get(&session.key).copied() {
                if session.last_usage.is_some_and(|last| last > killed_at) {
                    self.terminated.remove(&session.key);
                } else {
                    continue;
                }
            }
            active_keys.insert(session.key.clone());
            if service::active_session_ack(&cfg.service.state_dir, &session.key, now)?.is_some() {
                self.suppress(&session.key);
                continue;
            }
            let correlation = service::correlate(&session, &matches);
            self.evaluate(cfg, &session, &correlation, processes, platform, now)?;
        }
        self.retain_active(&active_keys);
        Ok(())
    }

    fn evaluate<P: Platform>(
        &mut self,
        cfg: &Config,
        session: &service::Session,
        correlation: &service::Correlation,
        processes: &platform::Snapshot,
        platform: &P,
        now: DateTime<Utc>,
    ) -> Result<(), UsageWatchError> {
        let key = session.key.as_str();
        let over_stop = session.latest_spent_tokens >= cfg.usage.kill_turn_tokens;
        let message = usage_message(session);

        if self.warned.insert(key.to_string()) {
            notify_user(cfg, platform, "Curb usage warning", &message)?;
            append_event(
                cfg,
                LedgerEvent::UsageWarning,
                session,
                correlation,
                &message,
                None,
            )?;
        }
        if !over_stop {
            self.grace.remove(key);
            self.targets.remove(key);
            return Ok(());
        }
        if !correlation.matched {
            let blocked_key = format!("uncorrelated:{key}");
            if self.warned.insert(blocked_key) {
                notify_user(
                    cfg,
                    platform,
                    "Curb stop blocked",
                    "Usage threshold exceeded, but Curb could not correlate this session to a live worker.",
                )?;
                append_event(
                    cfg,
                    LedgerEvent::UsageKillBlocked,
                    session,
                    correlation,
                    "usage threshold exceeded but no live process correlation was found",
                    None,
                )?;
            }
            return Ok(());
        }
        let Some(agent) = &correlation.agent else {
            return Ok(());
        };
        if !agent.can_terminate(cfg.usage.escalate_supervised) {
            let blocked_key = format!("watch-only:{key}");
            if self.warned.insert(blocked_key) {
                let (title, detail) = if agent.is_supervised() {
                    (
                        "Curb can't stop this agent",
                        "Over the kill line, but a desktop app supervises this task and would respawn it. Enable escalate_supervised to stop it.",
                    )
                } else {
                    (
                        "Curb stop blocked",
                        "Usage threshold exceeded, but the matched process is watch-only.",
                    )
                };
                notify_user(cfg, platform, title, detail)?;
                append_event(
                    cfg,
                    LedgerEvent::UsageKillBlocked,
                    session,
                    correlation,
                    detail,
                    None,
                )?;
            }
            return Ok(());
        }
        if cfg.mode != Mode::Enforcement {
            let would_key = format!("would:{key}");
            if self.warned.insert(would_key) {
                notify_user(cfg, platform, "Curb would stop agent", &message)?;
                append_event(
                    cfg,
                    LedgerEvent::UsageWouldTerminate,
                    session,
                    correlation,
                    &message,
                    None,
                )?;
            }
            return Ok(());
        }

        let Some(process) = &correlation.process else {
            return Ok(());
        };
        let Some(started) = self.grace.get(key).copied() else {
            self.grace.insert(key.to_string(), now);
            self.targets.insert(key.to_string(), process.clone());
            notify_user(cfg, platform, "Curb usage grace period", &message)?;
            append_event(
                cfg,
                LedgerEvent::UsageGraceStarted,
                session,
                correlation,
                &message,
                None,
            )?;
            return Ok(());
        };
        if now.signed_duration_since(started)
            < chrono::Duration::from_std(cfg.usage.grace_period.as_std()).unwrap()
        {
            return Ok(());
        }

        let target_process = self
            .targets
            .get(key)
            .cloned()
            .unwrap_or_else(|| process.clone());
        let mut termination_correlation = correlation.clone();
        termination_correlation.process = Some(target_process.clone());
        // Supervised desktop workers respawn when their leaf is killed; with the
        // escalate opt-in we target the supervisor's whole tree instead.
        let escalate = agent.is_supervised() && cfg.usage.escalate_supervised;
        let resolved_target = if escalate {
            processes
                .supervisor_target(&target_process, &agent.matcher.process_names)
                .or_else(|| processes.termination_target(&target_process))
        } else {
            processes.termination_target(&target_process)
        };
        let Some(target) = resolved_target else {
            notify_user(
                cfg,
                platform,
                "Curb stop failed",
                "Safety guard rejected termination for a stop-pending session.",
            )?;
            append_event(
                cfg,
                LedgerEvent::UsageTerminationFailed,
                session,
                &termination_correlation,
                "safety guard rejected termination",
                None,
            )?;
            return Ok(());
        };
        append_event(
            cfg,
            LedgerEvent::UsageTerminationStarted,
            session,
            &termination_correlation,
            &message,
            None,
        )?;
        let result = platform.terminate(&target, cfg.usage.grace_period.as_std());
        notify_user(cfg, platform, "Curb stopped agent", &message)?;
        append_event(
            cfg,
            LedgerEvent::UsageTerminationCompleted,
            session,
            &termination_correlation,
            &message,
            Some(json!(result)),
        )?;
        // Mark the session killed so the read model drops its row immediately
        // and the next scan stops re-warning or re-killing a dead worker.
        self.terminated.insert(key.to_string(), now);
        self.grace.remove(key);
        self.targets.remove(key);
        Ok(())
    }

    /// Sessions Curb has terminated and that have not resumed — the read model
    /// drops their rows so a killed agent leaves the dashboard at once.
    pub fn terminated_keys(&self) -> BTreeSet<String> {
        self.terminated.keys().cloned().collect()
    }

    fn suppress(&mut self, key: &str) {
        self.warned.remove(key);
        self.warned.remove(&format!("would:{key}"));
        self.warned.remove(&format!("uncorrelated:{key}"));
        self.warned.remove(&format!("watch-only:{key}"));
        self.grace.remove(key);
        self.targets.remove(key);
        self.terminated.remove(key);
    }

    fn retain_active(&mut self, active_keys: &BTreeSet<String>) {
        self.warned.retain(|key| {
            active_keys.contains(
                key.strip_prefix("would:")
                    .or_else(|| key.strip_prefix("uncorrelated:"))
                    .or_else(|| key.strip_prefix("watch-only:"))
                    .unwrap_or(key),
            )
        });
        self.grace.retain(|key, _| active_keys.contains(key));
        self.targets.retain(|key, _| active_keys.contains(key));
    }

    fn clear(&mut self) {
        self.warned.clear();
        self.grace.clear();
        self.targets.clear();
        self.terminated.clear();
    }
}

fn notify_user<P: Platform>(
    cfg: &Config,
    platform: &P,
    title: &str,
    message: &str,
) -> Result<(), UsageWatchError> {
    if !cfg.alerts.local_notifications {
        return Ok(());
    }
    if let Err(error) = platform.notify(title, message) {
        Ledger::open(&cfg.ledger.path)?.append(
            ledger::Event::new(LedgerEvent::NotificationFailed.as_str())
                .with_message(error.to_string())
                .with_mode(cfg.mode.to_string()),
        )?;
    }
    Ok(())
}

fn append_event(
    cfg: &Config,
    event_type: LedgerEvent,
    session: &service::Session,
    correlation: &service::Correlation,
    message: &str,
    result: Option<Value>,
) -> Result<(), UsageWatchError> {
    let mut event = ledger::Event::new(event_type.as_str())
        .with_message(message.to_string())
        .with_data(event_data(session, correlation, result));
    event.agent_id = correlation.agent.as_ref().map(|agent| agent.id.clone());
    event.mode = Some(cfg.mode.to_string());
    Ledger::open(&cfg.ledger.path)?.append(event)?;
    Ok(())
}

fn event_data(
    session: &service::Session,
    correlation: &service::Correlation,
    result: Option<Value>,
) -> Map<String, Value> {
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
    data.insert("calls".to_string(), json!(session.calls));
    data.insert("total_tokens".to_string(), json!(session.total_tokens));
    data.insert("turn_tokens".to_string(), json!(session.latest_turn_tokens));
    data.insert(
        "latest_spent_tokens".to_string(),
        json!(session.latest_spent_tokens),
    );
    data.insert(
        "window_spent_tokens".to_string(),
        json!(session.window_spent_tokens),
    );
    if let Some(last) = session.last {
        data.insert("last".to_string(), Value::String(last.to_rfc3339()));
    }
    if let Some(last_usage) = session.last_usage {
        data.insert(
            "last_usage".to_string(),
            Value::String(last_usage.to_rfc3339()),
        );
    }
    if !session.models.is_empty() {
        data.insert(
            "models".to_string(),
            Value::Array(session.models.iter().cloned().map(Value::String).collect()),
        );
    }
    if correlation.matched {
        if let Some(process) = &correlation.process {
            data.insert("pid".to_string(), json!(process.pid.get()));
        }
        if let Some(agent) = &correlation.agent {
            data.insert("agent_id".to_string(), Value::String(agent.id.clone()));
        }
        data.insert(
            "correlation".to_string(),
            Value::String(correlation.reason.clone()),
        );
        data.insert("correlation_score".to_string(), json!(correlation.score));
    }
    if let Some(result) = result {
        data.insert("result".to_string(), result);
    }
    data
}

fn usage_message(session: &service::Session) -> String {
    format!(
        "{} session {} latest checkpoint spent {} tokens ({} in window, {} calls)",
        session.provider,
        short_id(&session.id),
        format_tokens(session.latest_spent_tokens),
        format_tokens(session.window_spent_tokens),
        session.calls
    )
}

fn short_id(id: &str) -> String {
    if id.len() <= 12 {
        id.to_string()
    } else {
        format!("{}...{}", &id[..8], &id[id.len() - 4..])
    }
}

fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use chrono::TimeZone;
    use tempfile::tempdir;

    use super::*;
    use crate::config::{Config, HumanDuration};
    use crate::platform::{
        NotificationCapability, Pid, Platform, PlatformError, Process, Snapshot,
        TerminationCapability, TerminationResult, TerminationTarget,
    };
    use crate::usage::EventKind;

    /// Records which pids it was asked to terminate.
    #[derive(Default)]
    struct KillPlatform {
        terminated: Mutex<Vec<i32>>,
    }

    impl Platform for KillPlatform {
        fn capture(&self) -> Result<Snapshot, PlatformError> {
            Ok(Snapshot::new([]))
        }
        fn notification_capability(&self) -> NotificationCapability {
            NotificationCapability {
                supported: false,
                status: "off".to_string(),
                message: "off".to_string(),
            }
        }
        fn termination_capability(&self) -> TerminationCapability {
            TerminationCapability {
                supported: true,
                status: "ok".to_string(),
                message: "ok".to_string(),
            }
        }
        fn notify(&self, _: &str, _: &str) -> Result<(), PlatformError> {
            Ok(())
        }
        fn terminate(&self, target: &TerminationTarget, _: Duration) -> TerminationResult {
            self.terminated
                .lock()
                .unwrap()
                .extend(target.scope().iter().map(|pid| pid.get()));
            TerminationResult::default()
        }
    }

    fn codex_process(now: DateTime<Utc>, pid: i32, cwd: &str) -> Process {
        Process {
            pid: Pid::new(pid),
            ppid: None,
            name: "codex".to_string(),
            executable: Some("/usr/local/bin/codex".into()),
            command: "codex".to_string(),
            cwd: Some(cwd.into()),
            started_at: Some(now - chrono::Duration::minutes(5)),
            username: Some("tester".to_string()),
            bundle_id: None,
            team_id: None,
        }
    }

    fn over_kill_event(now: DateTime<Utc>, cwd: &str, spent: i64) -> UsageEvent {
        over_kill_event_for(now, "s1", cwd, spent)
    }

    fn over_kill_event_for(
        now: DateTime<Utc>,
        session_id: &str,
        cwd: &str,
        spent: i64,
    ) -> UsageEvent {
        UsageEvent {
            kind: EventKind::TokenCheckpoint,
            provider: "codex".to_string(),
            source: "test".to_string(),
            source_path: "fixture.jsonl".into(),
            session_id: Some(session_id.to_string()),
            turn_id: None,
            request_id: None,
            model: None,
            cwd: Some(cwd.into()),
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
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        cfg.usage.grace_period = HumanDuration::seconds(30);
        cfg.service.state_dir = state.path().to_path_buf();
        cfg.ledger.path = state.path().join("runs.ndjson");

        let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
        let processes = Snapshot::new([codex_process(now, 4242, "/repo")]);
        let platform = KillPlatform::default();
        let mut watch = UsageWatch::default();

        // Over the kill line: grace starts, nothing is terminated yet.
        watch
            .scan(
                &cfg,
                &[over_kill_event(now, "/repo", 250)],
                &processes,
                &platform,
                now,
            )
            .unwrap();
        assert!(platform.terminated.lock().unwrap().is_empty());

        // After the grace period, still over the line: the worker is terminated.
        let after = now + chrono::Duration::seconds(31);
        watch
            .scan(
                &cfg,
                &[over_kill_event(after, "/repo", 250)],
                &processes,
                &platform,
                after,
            )
            .unwrap();
        assert_eq!(*platform.terminated.lock().unwrap(), vec![4242]);
    }

    #[test]
    fn watch_mode_never_terminates() {
        let state = tempdir().unwrap();
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = Mode::Alert; // watch, not enforce
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        cfg.usage.grace_period = HumanDuration::seconds(0);
        cfg.service.state_dir = state.path().to_path_buf();
        cfg.ledger.path = state.path().join("runs.ndjson");

        let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
        let processes = Snapshot::new([codex_process(now, 4242, "/repo")]);
        let platform = KillPlatform::default();
        let mut watch = UsageWatch::default();

        watch
            .scan(
                &cfg,
                &[over_kill_event(now, "/repo", 250)],
                &processes,
                &platform,
                now,
            )
            .unwrap();
        let later = now + chrono::Duration::seconds(5);
        watch
            .scan(
                &cfg,
                &[over_kill_event(later, "/repo", 250)],
                &processes,
                &platform,
                later,
            )
            .unwrap();
        assert!(platform.terminated.lock().unwrap().is_empty());
    }

    /// A leaf worker spawned by the Codex desktop `app-server` supervisor.
    fn desktop_leaf(now: DateTime<Utc>, pid: i32, ppid: i32, cwd: &str) -> Process {
        Process {
            pid: Pid::new(pid),
            ppid: Some(Pid::new(ppid)),
            name: "codex".to_string(),
            executable: Some("/Applications/Codex.app/Contents/Resources/codex".into()),
            command:
                "/Applications/Codex.app/Contents/Resources/codex app-server --listen stdio://"
                    .to_string(),
            cwd: Some(cwd.into()),
            started_at: Some(now - chrono::Duration::minutes(5)),
            username: Some("tester".to_string()),
            bundle_id: None,
            team_id: None,
        }
    }

    /// The persistent supervisor that respawns leaf workers.
    fn desktop_supervisor(now: DateTime<Utc>, pid: i32) -> Process {
        Process {
            pid: Pid::new(pid),
            ppid: None,
            name: "codex".to_string(),
            executable: Some("/Applications/Codex.app/Contents/Resources/codex".into()),
            command: "/Applications/Codex.app/Contents/Resources/codex app-server --analytics-default-enabled"
                .to_string(),
            cwd: None,
            started_at: Some(now - chrono::Duration::minutes(30)),
            username: Some("tester".to_string()),
            bundle_id: None,
            team_id: None,
        }
    }

    #[test]
    fn supervised_desktop_worker_is_watch_only_by_default() {
        let state = tempdir().unwrap();
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        cfg.usage.grace_period = HumanDuration::seconds(0);
        cfg.service.state_dir = state.path().to_path_buf();
        cfg.ledger.path = state.path().join("runs.ndjson");

        let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
        let processes = Snapshot::new([
            desktop_supervisor(now, 3032),
            desktop_leaf(now, 7731, 3032, "/repo"),
        ]);
        let platform = KillPlatform::default();
        let mut watch = UsageWatch::default();

        for at in [now, now + chrono::Duration::seconds(5)] {
            watch
                .scan(
                    &cfg,
                    &[over_kill_event(at, "/repo", 250)],
                    &processes,
                    &platform,
                    at,
                )
                .unwrap();
        }
        // Killing the leaf is futile (it respawns), so Curb refuses by default.
        assert!(platform.terminated.lock().unwrap().is_empty());
        assert!(watch.terminated_keys().is_empty());
    }

    #[test]
    fn escalate_supervised_kills_the_supervisor() {
        let state = tempdir().unwrap();
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        cfg.usage.grace_period = HumanDuration::seconds(0);
        cfg.usage.escalate_supervised = true;
        cfg.service.state_dir = state.path().to_path_buf();
        cfg.ledger.path = state.path().join("runs.ndjson");

        let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
        let processes = Snapshot::new([
            desktop_supervisor(now, 3032),
            desktop_leaf(now, 7731, 3032, "/repo"),
        ]);
        let platform = KillPlatform::default();
        let mut watch = UsageWatch::default();

        for at in [now, now + chrono::Duration::seconds(5)] {
            watch
                .scan(
                    &cfg,
                    &[over_kill_event(at, "/repo", 250)],
                    &processes,
                    &platform,
                    at,
                )
                .unwrap();
        }
        // With escalation on, Curb targets the supervisor's whole tree.
        let killed = platform.terminated.lock().unwrap().clone();
        assert!(
            killed.contains(&3032),
            "supervisor not terminated: {killed:?}"
        );
        assert!(killed.contains(&7731), "leaf not terminated: {killed:?}");
    }

    #[test]
    fn killed_worker_is_marked_terminated_and_not_rekilled() {
        let state = tempdir().unwrap();
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        cfg.usage.grace_period = HumanDuration::seconds(0);
        cfg.service.state_dir = state.path().to_path_buf();
        cfg.ledger.path = state.path().join("runs.ndjson");

        let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
        let processes = Snapshot::new([codex_process(now, 4242, "/repo")]);
        let platform = KillPlatform::default();
        let mut watch = UsageWatch::default();

        // scan 1: grace starts. scan 2: terminate. scan 3: already dead (no new
        // activity since the kill) — skip, do not kill again.
        let killed_at = now + chrono::Duration::seconds(5);
        watch
            .scan(
                &cfg,
                &[over_kill_event(now, "/repo", 250)],
                &processes,
                &platform,
                now,
            )
            .unwrap();
        watch
            .scan(
                &cfg,
                &[over_kill_event(killed_at, "/repo", 250)],
                &processes,
                &platform,
                killed_at,
            )
            .unwrap();
        watch
            .scan(
                &cfg,
                &[over_kill_event(killed_at, "/repo", 250)],
                &processes,
                &platform,
                killed_at + chrono::Duration::seconds(5),
            )
            .unwrap();
        // Killed exactly once, and remembered so the read model drops its row.
        assert_eq!(*platform.terminated.lock().unwrap(), vec![4242]);
        assert!(watch.terminated_keys().contains("codex:s1"));
    }

    /// Scenario 1: over the kill line, but no live process correlates to the
    /// session. Curb must refuse and say so — `usage_kill_blocked` — never kill.
    #[test]
    fn uncorrelated_over_kill_blocks_and_records_decision() {
        let state = tempdir().unwrap();
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        cfg.usage.grace_period = HumanDuration::seconds(0);
        cfg.service.state_dir = state.path().to_path_buf();
        cfg.ledger.path = state.path().join("runs.ndjson");

        let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
        // The only live worker runs in a different repo, so nothing correlates.
        let processes = Snapshot::new([codex_process(now, 4242, "/other-repo")]);
        let platform = KillPlatform::default();
        let mut watch = UsageWatch::default();

        watch
            .scan(
                &cfg,
                &[over_kill_event(now, "/repo", 250)],
                &processes,
                &platform,
                now,
            )
            .unwrap();

        assert_eq!(
            ledger_event_types(&cfg),
            ["usage_warning", "usage_kill_blocked"]
        );
        assert!(platform.terminated.lock().unwrap().is_empty());
        assert!(watch.terminated_keys().is_empty());
    }

    /// Scenario 2: a supervised desktop worker is over the kill line in enforce
    /// mode with escalation off. Killing the leaf is futile, so Curb refuses —
    /// `usage_kill_blocked` — and nothing dies.
    #[test]
    fn supervised_over_kill_blocks_and_records_decision() {
        let state = tempdir().unwrap();
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        cfg.usage.grace_period = HumanDuration::seconds(0);
        cfg.usage.escalate_supervised = false;
        cfg.service.state_dir = state.path().to_path_buf();
        cfg.ledger.path = state.path().join("runs.ndjson");

        let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
        let processes = Snapshot::new([
            desktop_supervisor(now, 3032),
            desktop_leaf(now, 7731, 3032, "/repo"),
        ]);
        let platform = KillPlatform::default();
        let mut watch = UsageWatch::default();

        watch
            .scan(
                &cfg,
                &[over_kill_event(now, "/repo", 250)],
                &processes,
                &platform,
                now,
            )
            .unwrap();

        assert_eq!(
            ledger_event_types(&cfg),
            ["usage_warning", "usage_kill_blocked"]
        );
        assert!(platform.terminated.lock().unwrap().is_empty());
        assert!(watch.terminated_keys().is_empty());
    }

    /// Scenario 3: a correlated, terminable worker is over the kill line, but
    /// Curb is in watch (alert) mode. It must record `usage_would_terminate`,
    /// not actually terminate.
    #[test]
    fn alert_mode_over_kill_records_would_terminate() {
        let state = tempdir().unwrap();
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = Mode::Alert;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        cfg.usage.grace_period = HumanDuration::seconds(0);
        cfg.service.state_dir = state.path().to_path_buf();
        cfg.ledger.path = state.path().join("runs.ndjson");

        let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
        let processes = Snapshot::new([codex_process(now, 4242, "/repo")]);
        let platform = KillPlatform::default();
        let mut watch = UsageWatch::default();

        watch
            .scan(
                &cfg,
                &[over_kill_event(now, "/repo", 250)],
                &processes,
                &platform,
                now,
            )
            .unwrap();

        assert_eq!(
            ledger_event_types(&cfg),
            ["usage_warning", "usage_would_terminate"]
        );
        assert!(platform.terminated.lock().unwrap().is_empty());
        assert!(watch.terminated_keys().is_empty());
    }

    /// Scenario 4: the worker correlates and grace elapses, but by kill time the
    /// PID belongs to a different process (reused pid). The safety guard rejects
    /// termination — `usage_termination_failed` — and nothing dies.
    #[test]
    fn pid_reuse_at_kill_time_records_termination_failed() {
        let state = tempdir().unwrap();
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        cfg.usage.grace_period = HumanDuration::seconds(1);
        cfg.service.state_dir = state.path().to_path_buf();
        cfg.ledger.path = state.path().join("runs.ndjson");

        let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
        let original = Snapshot::new([codex_process(now, 4242, "/repo")]);
        // Same pid and cwd (still correlates), but a fresh start time means a
        // different process now holds the pid — the safety guard must refuse.
        let mut reused_process = codex_process(now, 4242, "/repo");
        reused_process.started_at = Some(now + chrono::Duration::seconds(2));
        let reused = Snapshot::new([reused_process]);
        let platform = KillPlatform::default();
        let mut watch = UsageWatch::default();

        // scan 1: grace starts against the original process.
        watch
            .scan(
                &cfg,
                &[over_kill_event(now, "/repo", 250)],
                &original,
                &platform,
                now,
            )
            .unwrap();
        // scan 2 (grace elapsed): the pid now belongs to a different process.
        let after = now + chrono::Duration::seconds(2);
        watch
            .scan(
                &cfg,
                &[over_kill_event(after, "/repo", 250)],
                &reused,
                &platform,
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
        assert!(platform.terminated.lock().unwrap().is_empty());
        assert!(watch.terminated_keys().is_empty());
    }

    /// Scenario 5: a killed worker logs fresh usage after the kill — it came
    /// back. The next scans must re-arm and re-kill it, not treat it as dead.
    #[test]
    fn killed_worker_that_resumes_is_rearmed_and_rekilled() {
        let state = tempdir().unwrap();
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        cfg.usage.grace_period = HumanDuration::seconds(0);
        cfg.service.state_dir = state.path().to_path_buf();
        cfg.ledger.path = state.path().join("runs.ndjson");

        let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
        let processes = Snapshot::new([codex_process(now, 4242, "/repo")]);
        let platform = KillPlatform::default();
        let mut watch = UsageWatch::default();

        // scan 1: grace. scan 2: first kill.
        let killed_at = now + chrono::Duration::seconds(5);
        watch
            .scan(
                &cfg,
                &[over_kill_event(now, "/repo", 250)],
                &processes,
                &platform,
                now,
            )
            .unwrap();
        watch
            .scan(
                &cfg,
                &[over_kill_event(killed_at, "/repo", 250)],
                &processes,
                &platform,
                killed_at,
            )
            .unwrap();
        assert_eq!(*platform.terminated.lock().unwrap(), vec![4242]);

        // Fresh usage after the kill: the session resumed. scan 3 re-arms grace,
        // scan 4 re-kills it.
        let resumed = killed_at + chrono::Duration::seconds(5);
        watch
            .scan(
                &cfg,
                &[over_kill_event(resumed, "/repo", 250)],
                &processes,
                &platform,
                resumed,
            )
            .unwrap();
        let rekill = resumed + chrono::Duration::seconds(5);
        watch
            .scan(
                &cfg,
                &[over_kill_event(rekill, "/repo", 250)],
                &processes,
                &platform,
                rekill,
            )
            .unwrap();

        // Re-armed (a second grace) and re-killed (the pid appears twice).
        assert_eq!(*platform.terminated.lock().unwrap(), vec![4242, 4242]);
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
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
        cfg.mode = Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 100;
        cfg.usage.kill_turn_tokens = 200;
        cfg.usage.grace_period = HumanDuration::seconds(0);
        cfg.usage.window = HumanDuration::minutes(5);
        cfg.service.state_dir = state.path().to_path_buf();
        cfg.ledger.path = state.path().join("runs.ndjson");

        let now = Utc.with_ymd_and_hms(2026, 5, 29, 16, 0, 0).unwrap();
        let processes = Snapshot::new([codex_process(now, 4242, "/repo")]);
        let platform = KillPlatform::default();
        let mut watch = UsageWatch::default();

        // scan 1: grace. scan 2: kill, remembered at killed_at.
        let killed_at = now + chrono::Duration::seconds(5);
        watch
            .scan(
                &cfg,
                &[over_kill_event(now, "/repo", 250)],
                &processes,
                &platform,
                now,
            )
            .unwrap();
        watch
            .scan(
                &cfg,
                &[over_kill_event(killed_at, "/repo", 250)],
                &processes,
                &platform,
                killed_at,
            )
            .unwrap();
        assert!(watch.terminated_keys().contains("codex:s1"));

        // scan 3 runs past the window. Only an unrelated session is active, so
        // s1 never enters the loop — the age-out retain is the one thing that
        // can drop its remembered kill.
        let aged_out = killed_at + chrono::Duration::minutes(5) + chrono::Duration::seconds(10);
        let unrelated = Snapshot::new([codex_process(aged_out, 5555, "/elsewhere")]);
        watch
            .scan(
                &cfg,
                &[over_kill_event_for(aged_out, "s2", "/elsewhere", 250)],
                &unrelated,
                &platform,
                aged_out,
            )
            .unwrap();

        // The remembered kill aged out, and the original worker was not killed
        // again (only the first kill stands).
        assert!(!watch.terminated_keys().contains("codex:s1"));
        assert_eq!(*platform.terminated.lock().unwrap(), vec![4242]);
    }
}
