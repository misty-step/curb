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
        (Some(agent), Some(process)) => Some(Box::new(LocalToken {
            process: process.clone(),
            supervisor_names: agent.matcher.process_names.clone(),
        }) as Box<dyn StopToken>),
        _ => None,
    };
    AgentTarget {
        matched: correlation.matched,
        agent_id: agent.map(|agent| agent.id.clone()),
        can_terminate: agent
            .is_some_and(|agent| agent.can_terminate(cfg.usage.escalate_supervised)),
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
                ledger::Event::new(LedgerEvent::NotificationFailed.as_str())
                    .with_message(error.to_string())
                    .with_mode(self.cfg.mode.to_string()),
            );
        }
    }

    fn stop(&self, token: &dyn StopToken, escalate: bool) -> StopResolution {
        let Some(token) = token.as_any().downcast_ref::<LocalToken>() else {
            return StopResolution::Rejected;
        };
        let resolved = if escalate {
            self.processes
                .supervisor_target(&token.process, &token.supervisor_names)
                .or_else(|| self.processes.termination_target(&token.process))
        } else {
            self.processes.termination_target(&token.process)
        };
        let Some(target) = resolved else {
            return StopResolution::Rejected;
        };
        let result = self
            .platform
            .terminate(&target, self.cfg.usage.grace_period.as_std());
        StopResolution::Stopped(serde_json::json!(result))
    }
}
