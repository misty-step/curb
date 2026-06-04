//! Write-path persistence for sessions.
//!
//! The read-model boundary in [`crate::service`] derives the pure snapshot and
//! its view transforms. This module owns the side-effecting half: the
//! [`Service`] entry points that acknowledge or stop a session, the
//! session-ack files those operations write and roll back, and the ledger
//! appends they emit. Keeping disk and ledger mutation here means a reader of
//! the snapshot-derivation code path never encounters write-path I/O.

use chrono::{DateTime, Utc};

use crate::config::{Config, Mode};
use crate::ledger::{self, Ledger};
use crate::platform::{self, Platform};
use crate::service::{
    AckRequest, AckView, Correlation, ServiceError, Session, SessionAck, StopRequest, StopView,
    active_session_ack, build_session_view, correlate, find_session, process_matches,
    read_session_ack, usage_activity_start,
};
use crate::usage::Event;

mod ack_store;
mod ledger_events;
mod stop_identity;

use ack_store::rollback_session_ack;
pub use ack_store::write_session_ack;
use stop_identity::{validate_expected_stop_identity, validate_stop_expectation};

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
        self.append_ledger_event(ledger_events::session_ack_event(ack, extend))
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
        let event = ledger_events::manual_stop_event(
            event_type,
            session,
            correlation,
            target,
            result,
            reason,
            self.cfg.mode,
        );
        self.append_ledger_event(event)
    }

    fn append_ledger_event(&self, event: ledger::Event) -> Result<(), ServiceError> {
        Ledger::open(&self.cfg.ledger.path)?.append(event)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
