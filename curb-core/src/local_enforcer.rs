//! The local-OS adapter between the pure policy core (`usagewatch`) and the
//! environment: it correlates raw usage events against the live process table,
//! builds the policy's [`PolicySession`] inputs, and executes stops against the
//! sealed OS termination target.
//!
//! This is the seam Phase 0 inverts. `usagewatch` no longer imports `service`
//! or `platform`; the caller (runtime, e2e) uses this adapter to bridge them.

use chrono::{DateTime, Utc};

use crate::config::Config;
use crate::ledger::{self, Ledger, LedgerEvent};
use crate::platform::{self, Platform};
use crate::service;
use crate::usage::Event as UsageEvent;
use crate::usagewatch::{AgentTarget, Enforcer, PolicySession, StopResolution, StopToken};

/// The opaque token the policy holds across scans. Carries the grace-time
/// process identity (the seal) and the agent's process-name family for
/// supervisor escalation — both OS facts the policy never inspects.
#[derive(Clone, Debug)]
pub struct LocalToken {
    process: platform::Process,
    supervisor_names: Vec<String>,
}

impl StopToken for LocalToken {
    fn clone_token(&self) -> Box<dyn StopToken> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Build the policy's correlated session inputs from raw usage events and the
/// live process snapshot. Correlation and ack resolution — formerly inside
/// `UsageWatch::scan` — happen here, in the local adapter.
pub fn build_policy_sessions(
    cfg: &Config,
    events: &[UsageEvent],
    processes: &platform::Snapshot,
    now: DateTime<Utc>,
) -> Result<Vec<PolicySession>, service::ServiceError> {
    let window_start = now - chrono::Duration::from_std(cfg.usage.window.as_std()).unwrap();
    let sessions = service::build_sessions(events, window_start);
    let matches = service::process_matches(cfg, processes);
    let mut out = Vec::with_capacity(sessions.len());
    for session in sessions {
        let correlation = service::correlate(&session, &matches);
        let acknowledged =
            service::active_session_ack(&cfg.service.state_dir, &session.key, now)?.is_some();
        out.push(PolicySession {
            target: agent_target(cfg, &correlation),
            acknowledged,
            key: session.key,
            id: session.id,
            provider: session.provider,
            cwd: session.cwd,
            models: session.models,
            last: session.last,
            last_usage: session.last_usage,
            calls: session.calls,
            latest_turn_tokens: session.latest_turn_tokens,
            latest_spent_tokens: session.latest_spent_tokens,
            window_spent_tokens: session.window_spent_tokens,
            total_tokens: session.total_tokens,
        });
    }
    Ok(out)
}

fn agent_target(cfg: &Config, correlation: &service::Correlation) -> AgentTarget {
    let agent = correlation.agent.as_ref();
    let stop_token = match (agent, correlation.process.as_ref()) {
        (Some(agent), Some(process))
            if process.has_termination_identity()
                || (agent.is_supervised() && cfg.usage.escalate_supervised) =>
        {
            Some(Box::new(LocalToken {
                process: process.clone(),
                supervisor_names: agent.matcher.process_names.clone(),
            }) as Box<dyn StopToken>)
        }
        _ => None,
    };
    let can_stop = agent.is_some_and(|agent| agent.can_terminate(cfg.usage.escalate_supervised))
        && stop_token.is_some();
    AgentTarget {
        matched: correlation.matched,
        agent_id: agent.map(|agent| agent.id.clone()),
        can_terminate: can_stop,
        supervised: agent.is_some_and(|agent| agent.is_supervised()),
        pid: correlation
            .process
            .as_ref()
            .map(|process| process.pid.get() as i64),
        score: correlation.score,
        reason: correlation.reason.clone(),
        stop_token,
    }
}

/// Executes notifications and sealed termination against the local OS for the
/// pure policy. Owns the safety contract (the `TerminationTarget` seal and the
/// supervisor escalation) so it never leaks into the policy core.
pub struct LocalEnforcer<'a, P: Platform> {
    cfg: &'a Config,
    platform: &'a P,
    processes: &'a platform::Snapshot,
}

impl<'a, P: Platform> LocalEnforcer<'a, P> {
    pub fn new(cfg: &'a Config, platform: &'a P, processes: &'a platform::Snapshot) -> Self {
        Self {
            cfg,
            platform,
            processes,
        }
    }
}

impl<P: Platform> Enforcer for LocalEnforcer<'_, P> {
    fn notify(&self, title: &str, message: &str) {
        if let Err(error) = self.platform.notify(title, message)
            && let Ok(ledger) = Ledger::open(&self.cfg.ledger.path)
        {
            let _ = ledger.append(
                ledger::Event::new(LedgerEvent::NotificationFailed)
                    .with_message(error.to_string())
                    .with_mode(self.cfg.mode.to_string()),
            );
        }
    }

    fn stop(&self, token: &dyn StopToken, escalate: bool) -> StopResolution {
        let Some(token) = token.as_any().downcast_ref::<LocalToken>() else {
            return StopResolution::Rejected("stop token did not belong to local enforcer".into());
        };
        let resolved = if escalate {
            self.processes
                .supervisor_target(&token.process, &token.supervisor_names)
                .or_else(|| self.processes.termination_target(&token.process))
        } else {
            self.processes.termination_target(&token.process)
        };
        let Some(target) = resolved else {
            let reason = if token.process.has_termination_identity() {
                format!(
                    "process identity could not be revalidated for pid {}",
                    token.process.pid.get()
                )
            } else {
                format!(
                    "grace-time process identity was incomplete for pid {}",
                    token.process.pid.get()
                )
            };
            return StopResolution::Rejected(reason);
        };
        let result = self
            .platform
            .terminate(&target, self.cfg.usage.grace_period.as_std());
        StopResolution::Stopped(serde_json::json!(result))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::TimeZone;
    use tempfile::tempdir;

    use super::*;
    use crate::config::{Agent, AgentKind, Match, Mode};
    use crate::platform::{Pid, Process, Snapshot};
    use crate::usage::{Event as UsageEvent, EventKind};

    #[test]
    fn local_policy_session_requires_sealable_identity_before_stop_token() {
        let temp = tempdir().unwrap();
        let mut cfg = Config::local_default(Mode::Enforcement, temp.path().join("state"));
        cfg.agents = vec![Agent {
            id: "synthetic-worker".to_string(),
            label: "Synthetic Worker".to_string(),
            family: "codex".to_string(),
            kind: AgentKind::Process,
            matcher: Match {
                process_names: vec!["sh".to_string()],
                command_regex: vec!["curb-e2e-worker".to_string()],
                require_command_regex: vec!["curb-e2e-worker".to_string()],
                ..Match::default()
            },
            policy: None,
        }];
        cfg.refresh_agent_policies();

        let now = Utc.with_ymd_and_hms(2026, 6, 11, 12, 0, 0).unwrap();
        let cwd = temp.path().join("work");
        let event = usage_event(now, &cwd);
        let incomplete = Process {
            executable: None,
            ..process(&cwd)
        };
        let sessions = build_policy_sessions(
            &cfg,
            std::slice::from_ref(&event),
            &Snapshot::new([incomplete]),
            now,
        )
        .unwrap();

        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].target.matched);
        assert!(!sessions[0].target.can_terminate);
        assert!(sessions[0].target.stop_token.is_none());

        let complete = process(&cwd);
        let sessions =
            build_policy_sessions(&cfg, &[event], &Snapshot::new([complete]), now).unwrap();

        assert!(sessions[0].target.can_terminate);
        assert!(sessions[0].target.stop_token.is_some());
    }

    fn process(cwd: &std::path::Path) -> Process {
        Process {
            pid: Pid::new(4242),
            ppid: None,
            name: "sh".to_string(),
            executable: Some(PathBuf::from("/bin/sh")),
            command: "sh -c 'while :; do sleep 1; done # curb-e2e-worker'".to_string(),
            cwd: Some(cwd.to_path_buf()),
            started_at: Some(Utc.with_ymd_and_hms(2026, 6, 11, 12, 0, 0).unwrap()),
            username: Some("tester".to_string()),
            bundle_id: None,
            team_id: None,
        }
    }

    fn usage_event(now: DateTime<Utc>, cwd: &std::path::Path) -> UsageEvent {
        UsageEvent {
            kind: EventKind::TokenCheckpoint,
            provider: "codex".to_string(),
            source: "test".to_string(),
            source_path: PathBuf::from("test.jsonl"),
            session_id: Some("session".to_string()),
            turn_id: None,
            request_id: None,
            model: None,
            cwd: Some(cwd.to_path_buf()),
            timestamp: Some(now),
            input_tokens: 250,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            total_tokens: 250,
            spent_tokens: 250,
            cumulative_tokens: 250,
            model_context_window: 0,
        }
    }
}
