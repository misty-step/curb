use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use thiserror::Error;

use crate::config::{Config, Mode};
use crate::ledger::{self, Ledger, LedgerEvent};

#[derive(Debug, Error)]
pub enum UsageWatchError {
    #[error(transparent)]
    Ledger(#[from] ledger::LedgerError),
}

/// A correlated session the policy evaluates, free of OS process facts. The
/// caller (runtime/e2e) builds these from raw usage events; the policy core
/// never sees `usage::Event`, `platform::Snapshot`, or `platform::Process`.
#[derive(Clone, Debug)]
pub struct PolicySession {
    pub key: String,
    pub id: String,
    pub provider: String,
    pub cwd: Option<PathBuf>,
    pub models: BTreeSet<String>,
    pub last: Option<DateTime<Utc>>,
    pub last_usage: Option<DateTime<Utc>>,
    pub calls: usize,
    pub latest_turn_tokens: i64,
    pub latest_spent_tokens: i64,
    pub window_spent_tokens: i64,
    pub total_tokens: i64,
    /// The correlation the caller resolved for this session.
    pub target: AgentTarget,
    /// Whether the operator has an active acknowledgement suppressing this
    /// session. The caller reads the ack store; the policy stays I/O-free.
    pub acknowledged: bool,
}

impl PolicySession {
    fn recent_usage(&self, window_start: DateTime<Utc>) -> bool {
        self.last_usage
            .is_some_and(|last_usage| last_usage >= window_start)
    }
}

/// An opaque token the enforcer can revalidate and stop. The policy stores it
/// across scans (grace lifecycle) without ever inspecting its contents; only
/// the enforcer that produced it knows how to resolve it back to a live target.
/// The OS seal (pid + start + owner + executable) lives inside the concrete
/// token an enforcer downcasts to — never in the policy core.
pub trait StopToken: std::any::Any + std::fmt::Debug + Send {
    fn clone_token(&self) -> Box<dyn StopToken>;
    fn as_any(&self) -> &dyn std::any::Any;
}

impl Clone for Box<dyn StopToken> {
    fn clone(&self) -> Self {
        self.clone_token()
    }
}

/// The pre-correlated, environment-agnostic view of a session's worker. Carries
/// only what the policy needs to decide — never an OS `Process`.
#[derive(Clone, Debug, Default)]
pub struct AgentTarget {
    pub matched: bool,
    pub agent_id: Option<String>,
    /// `true` when an agent matched and Curb may terminate it under the active
    /// escalation setting. Resolved by the caller from `Agent::can_terminate`.
    pub can_terminate: bool,
    /// `true` when the matched agent is a supervised desktop worker. Drives the
    /// escalation decision and the watch-only messaging.
    pub supervised: bool,
    /// The live worker pid, for ledger projection. `None` when uncorrelated.
    pub pid: Option<i64>,
    pub score: i64,
    pub reason: String,
    /// The token the enforcer uses to revalidate and stop the worker, captured
    /// at correlation time. `None` when uncorrelated.
    pub stop_token: Option<Box<dyn StopToken>>,
}

/// The outcome of an [`Enforcer::stop`] attempt, projected into the ledger.
pub enum StopResolution {
    /// The safety guard resolved a live target and the stop ran. Carries the
    /// already-serialized termination result for the completed ledger event.
    Stopped(Value),
    /// The safety guard rejected the stop (e.g. pid reuse). Nothing died.
    Rejected,
}

/// The side-effecting actions the policy delegates. The local implementation
/// owns the OS specifics (the sealed termination target, supervisor escalation,
/// the kill primitive); a remote implementation governs its own world.
pub trait Enforcer {
    /// Deliver an operator notification. Failures are the enforcer's concern.
    fn notify(&self, title: &str, message: &str);
    /// Revalidate and stop the worker behind `token`. `escalate` requests the
    /// supervisor's tree instead of the leaf for supervised desktop workers.
    fn stop(&self, token: &dyn StopToken, escalate: bool) -> StopResolution;
}

#[derive(Clone, Debug, Default)]
pub struct UsageWatch {
    warned: HashSet<String>,
    grace: HashMap<String, DateTime<Utc>>,
    targets: HashMap<String, Box<dyn StopToken>>,
    /// Sessions whose worker Curb has terminated, keyed to the kill time. The
    /// read model drops these rows so a killed agent leaves the dashboard at
    /// once instead of lingering on log recency, and the scan stops re-warning
    /// or re-killing them. Cleared when the session resumes (new activity after
    /// the kill) or ages out of the window.
    terminated: HashMap<String, DateTime<Utc>>,
}

impl UsageWatch {
    /// Evaluate the correlated `sessions` against config and drive the enforcer.
    /// Pure policy: the caller has already built sessions, resolved correlation
    /// and acks, and supplied the `enforcer`; this method owns only thresholds,
    /// the grace/terminated state machine, and the ledger projection.
    pub fn scan<E: Enforcer>(
        &mut self,
        cfg: &Config,
        sessions: &[PolicySession],
        enforcer: &E,
        window_start: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<(), UsageWatchError> {
        if !cfg.usage.enabled() {
            return Ok(());
        }
        if sessions.is_empty() {
            self.clear();
            return Ok(());
        }
        // Forget kills that have aged out of the window — beyond it the session
        // is a finished run the read model drops on recency anyway.
        self.terminated
            .retain(|_, killed_at| *killed_at >= window_start);
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
            if session.acknowledged {
                self.suppress(&session.key);
                continue;
            }
            self.evaluate(cfg, session, enforcer, now)?;
        }
        self.retain_active(&active_keys);
        Ok(())
    }

    fn evaluate<E: Enforcer>(
        &mut self,
        cfg: &Config,
        session: &PolicySession,
        enforcer: &E,
        now: DateTime<Utc>,
    ) -> Result<(), UsageWatchError> {
        let key = session.key.as_str();
        let target = &session.target;
        let over_stop = session.latest_spent_tokens >= cfg.usage.kill_turn_tokens;
        let message = usage_message(session);

        if self.warned.insert(key.to_string()) {
            notify_user(cfg, enforcer, "Curb usage warning", &message);
            append_event(cfg, LedgerEvent::UsageWarning, session, &message, None)?;
        }
        if !over_stop {
            self.grace.remove(key);
            self.targets.remove(key);
            return Ok(());
        }
        if !target.matched {
            let blocked_key = format!("uncorrelated:{key}");
            if self.warned.insert(blocked_key) {
                notify_user(
                    cfg,
                    enforcer,
                    "Curb stop blocked",
                    "Usage threshold exceeded, but Curb could not correlate this session to a live worker.",
                );
                append_event(
                    cfg,
                    LedgerEvent::UsageKillBlocked,
                    session,
                    "usage threshold exceeded but no live process correlation was found",
                    None,
                )?;
            }
            return Ok(());
        }
        if target.agent_id.is_none() {
            return Ok(());
        }
        if !target.can_terminate {
            let blocked_key = format!("watch-only:{key}");
            if self.warned.insert(blocked_key) {
                let (title, detail) = if target.supervised {
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
                notify_user(cfg, enforcer, title, detail);
                append_event(cfg, LedgerEvent::UsageKillBlocked, session, detail, None)?;
            }
            return Ok(());
        }
        if cfg.mode != Mode::Enforcement {
            let would_key = format!("would:{key}");
            if self.warned.insert(would_key) {
                notify_user(cfg, enforcer, "Curb would stop agent", &message);
                append_event(
                    cfg,
                    LedgerEvent::UsageWouldTerminate,
                    session,
                    &message,
                    None,
                )?;
            }
            return Ok(());
        }

        let Some(stop_token) = &target.stop_token else {
            return Ok(());
        };
        if !self.grace.contains_key(key) {
            self.grace.insert(key.to_string(), now);
            self.targets.insert(key.to_string(), stop_token.clone());
            notify_user(cfg, enforcer, "Curb usage grace period", &message);
            append_event(cfg, LedgerEvent::UsageGraceStarted, session, &message, None)?;
            return Ok(());
        }
        let started = self.grace[key];
        if now.signed_duration_since(started)
            < chrono::Duration::from_std(cfg.usage.grace_period.as_std()).unwrap()
        {
            return Ok(());
        }

        // Stop the grace-time target, falling back to the current correlation if
        // none was stored. Supervised desktop workers respawn when their leaf is
        // killed; with the escalate opt-in we target the supervisor's tree.
        let stored = self.targets.get(key).cloned();
        let stop_target: &dyn StopToken = match &stored {
            Some(token) => token.as_ref(),
            None => stop_token.as_ref(),
        };
        let escalate = target.supervised && cfg.usage.escalate_supervised;
        // The identity seal and the kill are bundled inside the env-agnostic
        // `Enforcer::stop`, so the termination_started/completed ledger pair is
        // written once the stop resolves. The event *sequence* (started ->
        // completed, or a lone failed) matches the old inline platform path; the
        // policy just no longer holds OS concepts to resolve the target itself.
        match enforcer.stop(stop_target, escalate) {
            StopResolution::Rejected => {
                notify_user(
                    cfg,
                    enforcer,
                    "Curb stop failed",
                    "Safety guard rejected termination for a stop-pending session.",
                );
                append_event(
                    cfg,
                    LedgerEvent::UsageTerminationFailed,
                    session,
                    "safety guard rejected termination",
                    None,
                )?;
            }
            StopResolution::Stopped(result) => {
                append_event(
                    cfg,
                    LedgerEvent::UsageTerminationStarted,
                    session,
                    &message,
                    None,
                )?;
                notify_user(cfg, enforcer, "Curb stopped agent", &message);
                append_event(
                    cfg,
                    LedgerEvent::UsageTerminationCompleted,
                    session,
                    &message,
                    Some(result),
                )?;
                // Mark the session killed so the read model drops its row
                // immediately and the next scan stops re-warning or re-killing.
                self.terminated.insert(key.to_string(), now);
                self.grace.remove(key);
                self.targets.remove(key);
            }
        }
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

fn notify_user<E: Enforcer>(cfg: &Config, enforcer: &E, title: &str, message: &str) {
    if !cfg.alerts.local_notifications {
        return;
    }
    enforcer.notify(title, message);
}

fn append_event(
    cfg: &Config,
    event_type: LedgerEvent,
    session: &PolicySession,
    message: &str,
    result: Option<Value>,
) -> Result<(), UsageWatchError> {
    let mut event = ledger::Event::new(event_type.as_str())
        .with_message(message.to_string())
        .with_data(event_data(session, result));
    event.agent_id = session.target.agent_id.clone();
    event.mode = Some(cfg.mode.to_string());
    Ledger::open(&cfg.ledger.path)?.append(event)?;
    Ok(())
}

fn event_data(session: &PolicySession, result: Option<Value>) -> Map<String, Value> {
    let target = &session.target;
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
    if target.matched {
        if let Some(pid) = target.pid {
            data.insert("pid".to_string(), json!(pid));
        }
        if let Some(agent_id) = &target.agent_id {
            data.insert("agent_id".to_string(), Value::String(agent_id.clone()));
        }
        data.insert(
            "correlation".to_string(),
            Value::String(target.reason.clone()),
        );
        data.insert("correlation_score".to_string(), json!(target.score));
    }
    if let Some(result) = result {
        data.insert("result".to_string(), result);
    }
    data
}

fn usage_message(session: &PolicySession) -> String {
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

    use chrono::TimeZone;
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
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
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
        watch
            .scan(&cfg, &[session], &enforcer, window_start(&cfg, now), now)
            .unwrap();
        assert!(enforcer.terminated.lock().unwrap().is_empty());

        // After the grace period, still over the line: the worker is terminated.
        let after = now + chrono::Duration::seconds(31);
        let session = policy_session(after, "codex:s1", "s1", 250, terminable_target(4242));
        watch
            .scan(
                &cfg,
                &[session],
                &enforcer,
                window_start(&cfg, after),
                after,
            )
            .unwrap();
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
        watch
            .scan(&cfg, &[session], &enforcer, window_start(&cfg, now), now)
            .unwrap();
        let later = now + chrono::Duration::seconds(5);
        let session = policy_session(later, "codex:s1", "s1", 250, terminable_target(4242));
        watch
            .scan(
                &cfg,
                &[session],
                &enforcer,
                window_start(&cfg, later),
                later,
            )
            .unwrap();
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
}
