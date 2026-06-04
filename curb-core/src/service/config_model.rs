use std::path::Path;

use serde::{Deserialize, Serialize};

use super::ServiceError;
use crate::config::{Agent, Config, HumanDuration, Mode};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub mode: String,
    pub usage_enabled: bool,
    pub warn_turn_tokens: i64,
    pub kill_turn_tokens: i64,
    pub usage_window_seconds: i64,
    pub usage_scan_seconds: i64,
    pub lookback_seconds: i64,
    pub process_warn_seconds: i64,
    pub process_kill_seconds: i64,
    pub ack_extension_seconds: i64,
    pub local_notifications: bool,
    pub escalate_supervised: bool,
    pub agents: Vec<ConfigAgentView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigAgentView {
    pub id: String,
    pub label: String,
    pub family: String,
    pub kind: String,
    pub terminates: bool,
    pub description: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(default)]
pub struct ConfigUpdate {
    pub mode: Option<String>,
    pub usage_enabled: Option<bool>,
    pub warn_turn_tokens: Option<i64>,
    pub kill_turn_tokens: Option<i64>,
    pub usage_window_seconds: Option<i64>,
    pub usage_scan_seconds: Option<i64>,
    pub lookback_seconds: Option<i64>,
    pub process_warn_seconds: Option<i64>,
    pub process_kill_seconds: Option<i64>,
    pub local_notifications: Option<bool>,
    pub escalate_supervised: Option<bool>,
}

pub fn config_view(path: Option<&Path>, cfg: &Config) -> ConfigView {
    ConfigView {
        path: path.map(|path| path.display().to_string()),
        mode: cfg.mode.to_string(),
        usage_enabled: cfg.usage.enabled(),
        warn_turn_tokens: cfg.usage.warn_turn_tokens,
        kill_turn_tokens: cfg.usage.kill_turn_tokens,
        usage_window_seconds: seconds(cfg.usage.window),
        usage_scan_seconds: seconds(cfg.usage.scan_interval),
        lookback_seconds: seconds(cfg.usage.lookback),
        process_warn_seconds: seconds(cfg.defaults.warn_after),
        process_kill_seconds: seconds(cfg.defaults.kill_after),
        ack_extension_seconds: seconds(cfg.defaults.ack_extension),
        local_notifications: cfg.alerts.local_notifications,
        escalate_supervised: cfg.usage.escalate_supervised,
        agents: cfg
            .agents
            .iter()
            .map(|agent| {
                let terminates = agent.termination_allowed();
                ConfigAgentView {
                    id: agent.id.clone(),
                    label: agent.label.clone(),
                    family: agent.family.clone(),
                    kind: agent_kind(agent, terminates).to_string(),
                    terminates,
                    description: if terminates {
                        "worker process; eligible for enforcement".to_string()
                    } else {
                        "app or shell process; visibility only".to_string()
                    },
                }
            })
            .collect(),
    }
}

pub fn apply_config_update(cfg: &mut Config, update: ConfigUpdate) -> Result<(), ServiceError> {
    if let Some(mode) = update.mode {
        cfg.mode = parse_mode(&mode)?;
    }
    if let Some(enabled) = update.usage_enabled {
        cfg.usage.enabled = Some(enabled);
    }
    if let Some(value) = update.warn_turn_tokens {
        cfg.usage.warn_turn_tokens = value;
    }
    if let Some(value) = update.kill_turn_tokens {
        cfg.usage.kill_turn_tokens = value;
    }
    if let Some(value) = update.usage_window_seconds {
        cfg.usage.window = positive_seconds("usage_window_seconds", value)?;
    }
    if let Some(value) = update.usage_scan_seconds {
        cfg.usage.scan_interval = positive_seconds("usage_scan_seconds", value)?;
    }
    if let Some(value) = update.lookback_seconds {
        cfg.usage.lookback = positive_seconds("lookback_seconds", value)?;
    }
    if let Some(value) = update.process_warn_seconds {
        cfg.defaults.warn_after = positive_seconds("process_warn_seconds", value)?;
    }
    if let Some(value) = update.process_kill_seconds {
        cfg.defaults.kill_after = positive_seconds("process_kill_seconds", value)?;
    }
    if let Some(enabled) = update.local_notifications {
        cfg.alerts.local_notifications = enabled;
    }
    if let Some(enabled) = update.escalate_supervised {
        cfg.usage.escalate_supervised = enabled;
    }
    cfg.validate()
        .map_err(|error| ServiceError::InvalidConfig(error.to_string()))
}

fn seconds(duration: HumanDuration) -> i64 {
    i64::try_from(duration.as_std().as_secs()).unwrap_or(i64::MAX)
}

fn positive_seconds(field: &'static str, value: i64) -> Result<HumanDuration, ServiceError> {
    if value <= 0 {
        return Err(ServiceError::InvalidConfig(format!(
            "{field} must be positive"
        )));
    }
    Ok(HumanDuration::seconds(value as u64))
}

fn parse_mode(mode: &str) -> Result<Mode, ServiceError> {
    match mode {
        "visibility" => Ok(Mode::Visibility),
        "alert" => Ok(Mode::Alert),
        "enforcement" => Ok(Mode::Enforcement),
        other => Err(ServiceError::InvalidConfig(format!(
            "invalid mode {other:?}"
        ))),
    }
}

fn agent_kind(agent: &Agent, terminates: bool) -> &'static str {
    match agent.kind {
        crate::config::AgentKind::Process => "process",
        crate::config::AgentKind::App => "app",
        crate::config::AgentKind::Unspecified if terminates => "process",
        crate::config::AgentKind::Unspecified => "app",
    }
}
