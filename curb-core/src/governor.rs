//! Embeddable policy driver for orchestrators that already know their world.
//!
//! Curb's local runtime observes provider logs and OS processes before it calls
//! this module. Olympus will observe SQLite runs, lane health, Sprite readiness,
//! and job telemetry before it calls this module. The governor deliberately does
//! not own observation or correlation; callers submit pre-correlated
//! [`PolicySession`] values and an environment-specific [`Enforcer`].

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};

use crate::config::Config;
use crate::usagewatch::{Enforcer, PolicyScanReport, PolicySession, UsageWatch, UsageWatchError};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GovernorReport {
    pub observed_sessions: usize,
    pub terminated_keys: BTreeSet<String>,
    pub policy: PolicyScanReport,
}

/// Stateful governor for one environment.
///
/// The caller owns the environment: local OS, Olympus lanes, containers, or a
/// future remote runner. The governor owns only the policy state machine:
/// warning suppression, grace periods, killed-session memory, and ledger
/// projection.
#[derive(Clone, Debug, Default)]
pub struct GovernorEngine {
    watch: UsageWatch,
}

impl GovernorEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn scan<E: Enforcer>(
        &mut self,
        cfg: &Config,
        sessions: &[PolicySession],
        enforcer: &E,
        now: DateTime<Utc>,
    ) -> Result<GovernorReport, UsageWatchError> {
        let window_start = now - chrono::Duration::from_std(cfg.usage.window.as_std()).unwrap();
        let policy = self
            .watch
            .scan(cfg, sessions, enforcer, window_start, now)?;
        Ok(GovernorReport {
            observed_sessions: sessions.len(),
            terminated_keys: self.watch.terminated_keys(),
            policy,
        })
    }

    pub fn terminated_keys(&self) -> BTreeSet<String> {
        self.watch.terminated_keys()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::config::{Config, HumanDuration, Mode};
    use crate::usagewatch::{AgentTarget, StopResolution, StopToken};

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct OlympusRunToken {
        run_id: i64,
        workflow: String,
        trace_id: String,
        sprite_name: String,
    }

    impl StopToken for OlympusRunToken {
        fn clone_token(&self) -> Box<dyn StopToken> {
            Box::new(self.clone())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[derive(Default)]
    struct OlympusEnforcer {
        stopped: RefCell<Vec<OlympusRunToken>>,
        notifications: RefCell<Vec<String>>,
    }

    impl Enforcer for OlympusEnforcer {
        fn notify(&self, title: &str, _message: &str) {
            self.notifications.borrow_mut().push(title.to_string());
        }

        fn stop(&self, token: &dyn StopToken, _escalate: bool) -> StopResolution {
            let Some(token) = token.as_any().downcast_ref::<OlympusRunToken>() else {
                return StopResolution::Rejected;
            };
            self.stopped.borrow_mut().push(token.clone());
            StopResolution::Stopped(json!({
                "run_id": token.run_id,
                "workflow": token.workflow,
                "trace_id": token.trace_id,
                "sprite_name": token.sprite_name,
                "action": "request_lane_stop"
            }))
        }
    }

    #[test]
    fn governor_stops_olympus_like_run_after_grace_without_pid_identity() {
        let state = tempdir().unwrap();
        let mut cfg = Config::local_default(Mode::Enforcement, state.path().to_path_buf());
        cfg.usage.warn_turn_tokens = 10;
        cfg.usage.kill_turn_tokens = 20;
        cfg.usage.grace_period = HumanDuration::seconds(5);
        cfg.usage.window = HumanDuration::seconds(300);
        cfg.alerts.local_notifications = false;
        let now = Utc.with_ymd_and_hms(2026, 6, 2, 12, 0, 0).unwrap();
        let token = OlympusRunToken {
            run_id: 42,
            workflow: "nemesis".to_string(),
            trace_id: "trace-abc".to_string(),
            sprite_name: "nemesis-sprite".to_string(),
        };
        let session = PolicySession {
            key: "olympus:nemesis:42".to_string(),
            id: "42".to_string(),
            provider: "olympus".to_string(),
            cwd: Some(PathBuf::from("/workspace/repo")),
            models: BTreeSet::from(["gpt-5.5".to_string()]),
            last: Some(now),
            last_usage: Some(now),
            calls: 3,
            latest_turn_tokens: 30,
            latest_spent_tokens: 30,
            window_spent_tokens: 30,
            total_tokens: 90,
            acknowledged: false,
            target: AgentTarget {
                matched: true,
                agent_id: Some("nemesis".to_string()),
                can_terminate: true,
                supervised: false,
                pid: None,
                score: 100,
                reason: "olympus-run-trace".to_string(),
                stop_token: Some(Box::new(token.clone())),
            },
        };
        let enforcer = OlympusEnforcer::default();
        let mut governor = GovernorEngine::new();

        let first = governor.scan(&cfg, std::slice::from_ref(&session), &enforcer, now);
        assert!(first.is_ok());
        assert!(enforcer.stopped.borrow().is_empty());

        let second = governor
            .scan(
                &cfg,
                std::slice::from_ref(&session),
                &enforcer,
                now + chrono::Duration::seconds(6),
            )
            .unwrap();

        assert_eq!(enforcer.stopped.borrow().as_slice(), &[token]);
        assert_eq!(second.observed_sessions, 1);
        assert!(second.terminated_keys.contains("olympus:nemesis:42"));
    }
}
