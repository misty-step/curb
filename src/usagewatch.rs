use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use thiserror::Error;

use crate::config::{Config, Mode};
use crate::ledger::{self, Ledger};
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
        let matches = service::process_matches(cfg, processes);
        let mut active_keys = BTreeSet::new();
        for session in sessions {
            if !session.recent_usage(window_start)
                || session.latest_turn_tokens < cfg.usage.warn_turn_tokens
            {
                self.suppress(&session.key);
                continue;
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
        let over_stop = session.latest_turn_tokens >= cfg.usage.kill_turn_tokens;
        let message = usage_message(session);

        if self.warned.insert(key.to_string()) {
            notify_user(cfg, platform, "Curb usage warning", &message)?;
            append_event(cfg, "usage_warning", session, correlation, &message, None)?;
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
                    "usage_kill_blocked",
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
        if !agent.termination_allowed() {
            let blocked_key = format!("watch-only:{key}");
            if self.warned.insert(blocked_key) {
                notify_user(
                    cfg,
                    platform,
                    "Curb stop blocked",
                    "Usage threshold exceeded, but the matched process is watch-only.",
                )?;
                append_event(
                    cfg,
                    "usage_kill_blocked",
                    session,
                    correlation,
                    "usage threshold exceeded but matched agent is watch-only",
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
                    "usage_would_terminate",
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
                "usage_grace_started",
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
        let Some(target) = processes.termination_target(&target_process) else {
            notify_user(
                cfg,
                platform,
                "Curb stop failed",
                "Safety guard rejected termination for a stop-pending session.",
            )?;
            append_event(
                cfg,
                "usage_termination_failed",
                session,
                &termination_correlation,
                "safety guard rejected termination",
                None,
            )?;
            return Ok(());
        };
        append_event(
            cfg,
            "usage_termination_started",
            session,
            &termination_correlation,
            &message,
            None,
        )?;
        let result = platform.terminate(&target, cfg.usage.grace_period.as_std());
        notify_user(cfg, platform, "Curb stopped agent", &message)?;
        append_event(
            cfg,
            "usage_termination_completed",
            session,
            &termination_correlation,
            &message,
            Some(json!(result)),
        )?;
        Ok(())
    }

    fn suppress(&mut self, key: &str) {
        self.warned.remove(key);
        self.warned.remove(&format!("would:{key}"));
        self.warned.remove(&format!("uncorrelated:{key}"));
        self.warned.remove(&format!("watch-only:{key}"));
        self.grace.remove(key);
        self.targets.remove(key);
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
            ledger::Event::new("notification_failed")
                .with_message(error.to_string())
                .with_mode(cfg.mode.to_string()),
        )?;
    }
    Ok(())
}

fn append_event(
    cfg: &Config,
    event_type: &str,
    session: &service::Session,
    correlation: &service::Correlation,
    message: &str,
    result: Option<Value>,
) -> Result<(), UsageWatchError> {
    let mut event = ledger::Event::new(event_type)
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
        "{} session {} latest turn used {} tokens (total {} in {} calls)",
        session.provider,
        short_id(&session.id),
        format_tokens(session.latest_turn_tokens),
        format_tokens(session.total_tokens),
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

trait EventMode {
    fn with_mode(self, mode: String) -> Self;
}

impl EventMode for ledger::Event {
    fn with_mode(mut self, mode: String) -> Self {
        self.mode = Some(mode);
        self
    }
}
