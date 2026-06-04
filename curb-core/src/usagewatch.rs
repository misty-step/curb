use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::Value;
use thiserror::Error;

use crate::config::{Config, Mode};
use crate::ledger::{self, Ledger, LedgerEvent};

mod events;
#[cfg(test)]
mod tests;

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

/// Machine-readable summary of one policy scan.
///
/// The ledger remains the durable audit trail. This report is the compact
/// per-tick shape runtime observability can log without scraping the ledger or
/// inferring grace/stop outcomes from UI prose.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PolicyScanReport {
    pub observed_sessions: usize,
    pub warnings: usize,
    pub would_stop: usize,
    pub stop_blocked: usize,
    pub grace_started: usize,
    pub grace_pending: usize,
    pub stop_attempted: usize,
    pub stop_completed: usize,
    pub stop_rejected: usize,
    pub resumed_sessions: usize,
    pub terminated_sessions: usize,
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
    ) -> Result<PolicyScanReport, UsageWatchError> {
        let mut report = PolicyScanReport {
            observed_sessions: sessions.len(),
            ..PolicyScanReport::default()
        };
        if !cfg.usage.enabled() {
            return Ok(report);
        }
        if sessions.is_empty() {
            self.clear();
            return Ok(report);
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
                    report.resumed_sessions += 1;
                } else {
                    continue;
                }
            }
            active_keys.insert(session.key.clone());
            if session.acknowledged {
                self.suppress(&session.key);
                continue;
            }
            self.evaluate(cfg, session, enforcer, now, &mut report)?;
        }
        self.retain_active(&active_keys);
        report.terminated_sessions = self.terminated.len();
        Ok(report)
    }

    fn evaluate<E: Enforcer>(
        &mut self,
        cfg: &Config,
        session: &PolicySession,
        enforcer: &E,
        now: DateTime<Utc>,
        report: &mut PolicyScanReport,
    ) -> Result<(), UsageWatchError> {
        let key = session.key.as_str();
        let target = &session.target;
        let over_stop = session.latest_spent_tokens >= cfg.usage.kill_turn_tokens;
        let message = events::usage_message(session);

        if self.warned.insert(key.to_string()) {
            notify_user(cfg, enforcer, "Curb usage warning", &message);
            append_event(cfg, LedgerEvent::UsageWarning, session, &message, None)?;
            report.warnings += 1;
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
                report.stop_blocked += 1;
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
                report.stop_blocked += 1;
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
                report.would_stop += 1;
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
            report.grace_started += 1;
            return Ok(());
        }
        let started = self.grace[key];
        if now.signed_duration_since(started)
            < chrono::Duration::from_std(cfg.usage.grace_period.as_std()).unwrap()
        {
            report.grace_pending += 1;
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
        report.stop_attempted += 1;
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
                report.stop_rejected += 1;
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
                report.stop_completed += 1;
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
        .with_data(events::event_data(session, result));
    event.agent_id = session.target.agent_id.clone();
    event.mode = Some(cfg.mode.to_string());
    Ledger::open(&cfg.ledger.path)?.append(event)?;
    Ok(())
}
