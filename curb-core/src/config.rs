use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

mod defaults;
mod duration;
mod policy_merge;
mod preset;
mod storage;
#[cfg(test)]
mod tests;

pub use defaults::default_home_dir;
use defaults::{default_process_agents, default_state_dir};
pub use duration::{HumanDuration, parse_duration_for_cli};
pub use preset::Preset;
use storage::{set_dir_private, set_file_private, write_private_file};

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("read config {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("parse config {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_yaml::Error,
    },
    #[error("serialize config {path}: {source}")]
    Serialize {
        path: PathBuf,
        source: serde_yaml::Error,
    },
    #[error("write config {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("version must be 1, got {0}")]
    Version(i64),
    #[error("invalid mode {0:?}")]
    Mode(String),
    #[error("invalid preset {0:?}")]
    Preset(String),
    #[error("defaults.warn_after must be less than defaults.kill_after")]
    InvalidDefaultThresholds,
    #[error("ledger.include_prompt_content is not supported by launch implementation")]
    PromptCaptureUnsupported,
    #[error("usage.warn_turn_tokens must be less than usage.kill_turn_tokens")]
    InvalidUsageThresholds,
    #[error("usage intervals must be positive")]
    InvalidUsageIntervals,
    #[error("agent id is required")]
    MissingAgentId,
    #[error("duplicate agent id {0:?}")]
    DuplicateAgentId(String),
    #[error("agent {0:?} label is required")]
    MissingAgentLabel(String),
    #[error("agent {0:?} kind must be process or app")]
    InvalidAgentKind(String),
    #[error("agent {0:?} must define at least one matcher")]
    MissingAgentMatcher(String),
    #[error("agent {agent:?} {field} {pattern:?}: {source}")]
    InvalidRegex {
        agent: String,
        field: &'static str,
        pattern: String,
        source: regex::Error,
    },
    #[error("agent {0:?} warn_after must be less than kill_after")]
    InvalidAgentThresholds(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: i64,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub service: ServiceConfig,
    #[serde(default)]
    pub usage: UsageConfig,
    #[serde(default)]
    pub defaults: Policy,
    #[serde(default)]
    pub agents: Vec<Agent>,
    #[serde(default)]
    pub alerts: AlertConfig,
    #[serde(default)]
    pub ledger: LedgerConfig,
}

impl Config {
    pub fn local_default(mode: Mode, state_dir: PathBuf) -> Self {
        let mut cfg = Self {
            version: 1,
            profile: "local-default".to_string(),
            mode,
            service: ServiceConfig {
                state_dir: state_dir.clone(),
                min_confidence: 50,
                ..ServiceConfig::default()
            },
            usage: UsageConfig {
                enabled: Some(true),
                ..UsageConfig::default()
            },
            defaults: Policy::default(),
            agents: default_process_agents(),
            alerts: AlertConfig {
                local_notifications: true,
            },
            ledger: LedgerConfig {
                path: state_dir.join("runs.ndjson"),
                ..LedgerConfig::default()
            },
        };
        cfg.set_defaults();
        cfg.refresh_agent_policies();
        cfg
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let mut cfg: Self = serde_yaml::from_str(&raw).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        cfg.set_defaults();
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        let path = path.as_ref();
        self.validate()?;
        let raw = serde_yaml::to_string(self).map_err(|source| ConfigError::Serialize {
            path: path.to_path_buf(),
            source,
        })?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
                path: parent.to_path_buf(),
                source,
            })?;
            set_dir_private(parent)?;
        }
        let tmp = path.with_extension(format!(
            "{}.tmp",
            path.extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("yaml")
        ));
        write_private_file(&tmp, raw.as_bytes())?;
        fs::rename(&tmp, path).map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source,
        })?;
        set_file_private(path)
    }

    pub fn set_defaults(&mut self) {
        if self.mode == Mode::Unspecified {
            self.mode = Mode::Visibility;
        }
        self.service.set_defaults();
        self.usage.set_defaults();
        self.defaults.set_defaults();
        if self.ledger.path.as_os_str().is_empty() {
            self.ledger.path = self.service.state_dir.join("runs.ndjson");
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.version != 1 {
            return Err(ConfigError::Version(self.version));
        }
        if self.mode == Mode::Unspecified {
            return Err(ConfigError::Mode(String::new()));
        }
        if self.defaults.warn_after >= self.defaults.kill_after {
            return Err(ConfigError::InvalidDefaultThresholds);
        }
        if self.ledger.include_prompt_content {
            return Err(ConfigError::PromptCaptureUnsupported);
        }
        if self.usage.enabled() {
            if self.usage.warn_turn_tokens >= self.usage.kill_turn_tokens {
                return Err(ConfigError::InvalidUsageThresholds);
            }
            if self.usage.scan_interval.is_zero()
                || self.usage.lookback.is_zero()
                || self.usage.window.is_zero()
            {
                return Err(ConfigError::InvalidUsageIntervals);
            }
        }

        let mut seen = std::collections::HashSet::new();
        for agent in &self.agents {
            if agent.id.is_empty() {
                return Err(ConfigError::MissingAgentId);
            }
            if !seen.insert(agent.id.clone()) {
                return Err(ConfigError::DuplicateAgentId(agent.id.clone()));
            }
            if agent.label.is_empty() {
                return Err(ConfigError::MissingAgentLabel(agent.id.clone()));
            }
            if !matches!(
                agent.kind,
                AgentKind::Unspecified | AgentKind::Process | AgentKind::App
            ) {
                return Err(ConfigError::InvalidAgentKind(agent.id.clone()));
            }
            if agent.matcher.is_empty() {
                return Err(ConfigError::MissingAgentMatcher(agent.id.clone()));
            }
            validate_regexes(&agent.id, "command_regex", &agent.matcher.command_regex)?;
            validate_regexes(
                &agent.id,
                "require_command_regex",
                &agent.matcher.require_command_regex,
            )?;
            validate_regexes(
                &agent.id,
                "exclude_command_regex",
                &agent.matcher.exclude_command_regex,
            )?;
            validate_regexes(
                &agent.id,
                "exclude_parent_command_regex",
                &agent.matcher.exclude_parent_regex,
            )?;
            if self.policy_for(agent).warn_after >= self.policy_for(agent).kill_after {
                return Err(ConfigError::InvalidAgentThresholds(agent.id.clone()));
            }
        }
        Ok(())
    }

    pub fn policy_for(&self, agent: &Agent) -> Policy {
        policy_merge::policy_for(self, agent)
    }

    pub fn apply_preset(&mut self, preset: Preset) {
        preset::apply(self, preset);
    }

    pub fn refresh_agent_policies(&mut self) {
        policy_merge::refresh_agent_policies(self);
    }
}

fn validate_regexes(
    agent: &str,
    field: &'static str,
    patterns: &[String],
) -> Result<(), ConfigError> {
    for pattern in patterns {
        Regex::new(pattern).map_err(|source| ConfigError::InvalidRegex {
            agent: agent.to_string(),
            field,
            pattern: pattern.clone(),
            source,
        })?;
    }
    Ok(())
}

/// The runtime makes one decision from mode: enforce (stop runaways) or not.
/// `Visibility` and `Alert` are both "watch" — they never terminate; they
/// differ only in config defaults and whether notifications are wired
/// (`alerts.local_notifications`), not in any branch the policy engine takes.
/// The product surfaces just two modes: Watch (Visibility/Alert) and Enforce.
/// See `service::mode_label`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
    #[default]
    Unspecified,
    Visibility,
    Alert,
    Enforcement,
}

impl Serialize for Mode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Mode::Unspecified => "",
            Mode::Visibility => "visibility",
            Mode::Alert => "alert",
            Mode::Enforcement => "enforcement",
        })
    }
}

impl<'de> Deserialize<'de> for Mode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match String::deserialize(deserializer)?.as_str() {
            "" => Ok(Self::Unspecified),
            "visibility" => Ok(Self::Visibility),
            "alert" => Ok(Self::Alert),
            "enforcement" => Ok(Self::Enforcement),
            other => Err(serde::de::Error::custom(format!("invalid mode {other:?}"))),
        }
    }
}

impl FromStr for Mode {
    type Err = ConfigError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "visibility" => Ok(Self::Visibility),
            "alert" => Ok(Self::Alert),
            "enforcement" => Ok(Self::Enforcement),
            other => Err(ConfigError::Mode(other.to_string())),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServiceConfig {
    pub scan_interval: HumanDuration,
    pub policy_interval: HumanDuration,
    pub state_dir: PathBuf,
    pub min_confidence: i64,
    pub heartbeat_interval: HumanDuration,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            scan_interval: HumanDuration::ZERO,
            policy_interval: HumanDuration::ZERO,
            state_dir: PathBuf::new(),
            min_confidence: 0,
            heartbeat_interval: HumanDuration::ZERO,
        }
    }
}

impl ServiceConfig {
    fn set_defaults(&mut self) {
        self.scan_interval.default_to(HumanDuration::seconds(15));
        self.policy_interval.default_to(HumanDuration::seconds(5));
        self.heartbeat_interval
            .default_to(HumanDuration::seconds(60));
        if self.min_confidence == 0 {
            self.min_confidence = 50;
        }
        if self.state_dir.as_os_str().is_empty() {
            self.state_dir = default_state_dir();
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct UsageConfig {
    pub enabled: Option<bool>,
    pub scan_interval: HumanDuration,
    pub lookback: HumanDuration,
    pub window: HumanDuration,
    pub warn_turn_tokens: i64,
    pub kill_turn_tokens: i64,
    pub grace_period: HumanDuration,
    /// Opt-in: when a supervised desktop worker blows past the kill line, kill
    /// the supervisor process instead of the leaf (which would just respawn).
    /// Off by default — it stops every concurrent task under that supervisor.
    pub escalate_supervised: bool,
}

impl Default for UsageConfig {
    fn default() -> Self {
        Self {
            enabled: None,
            scan_interval: HumanDuration::ZERO,
            lookback: HumanDuration::ZERO,
            window: HumanDuration::ZERO,
            warn_turn_tokens: 0,
            kill_turn_tokens: 0,
            grace_period: HumanDuration::ZERO,
            escalate_supervised: false,
        }
    }
}

impl UsageConfig {
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    fn set_defaults(&mut self) {
        self.scan_interval.default_to(HumanDuration::seconds(5));
        self.lookback.default_to(HumanDuration::hours(24));
        self.window.default_to(HumanDuration::minutes(5));
        if self.warn_turn_tokens == 0 {
            self.warn_turn_tokens = 1_000_000;
        }
        if self.kill_turn_tokens == 0 {
            self.kill_turn_tokens = 3_000_000;
        }
        self.grace_period.default_to(HumanDuration::seconds(10));
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Agent {
    pub id: String,
    pub label: String,
    pub family: String,
    pub kind: AgentKind,
    #[serde(rename = "match")]
    pub matcher: Match,
    pub policy: Option<Policy>,
}

impl Default for Agent {
    fn default() -> Self {
        Self {
            id: String::new(),
            label: String::new(),
            family: String::new(),
            kind: AgentKind::Unspecified,
            matcher: Match::default(),
            policy: None,
        }
    }
}

impl Agent {
    pub fn termination_allowed(&self) -> bool {
        match self.kind {
            AgentKind::Process => true,
            AgentKind::App => false,
            AgentKind::Unspecified => {
                !self.id.to_lowercase().contains("desktop")
                    && self.matcher.bundle_ids.is_empty()
                    && self.matcher.app_paths.is_empty()
            }
        }
    }

    /// A real (terminable) worker spawned — and respawned — by a long-lived
    /// desktop supervisor (e.g. the Codex desktop app's `app-server`). Killing
    /// the leaf is futile because the supervisor restarts it, so these are
    /// watch-only by default. Detected by the established "desktop" id
    /// convention; excludes App-kind GUI agents, which are watch-only outright.
    pub fn is_supervised(&self) -> bool {
        self.termination_allowed() && self.id.to_lowercase().contains("desktop")
    }

    /// Whether Curb may terminate a matched worker for this agent. Supervised
    /// desktop workers are watch-only unless the operator opts into escalation,
    /// which targets the supervisor process instead of the respawning leaf.
    pub fn can_terminate(&self, escalate_supervised: bool) -> bool {
        self.termination_allowed() && (escalate_supervised || !self.is_supervised())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AgentKind {
    #[default]
    Unspecified,
    Process,
    App,
}

impl Serialize for AgentKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match self {
            AgentKind::Unspecified => "",
            AgentKind::Process => "process",
            AgentKind::App => "app",
        })
    }
}

impl<'de> Deserialize<'de> for AgentKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match String::deserialize(deserializer)?.as_str() {
            "" => Ok(Self::Unspecified),
            "process" => Ok(Self::Process),
            "app" => Ok(Self::App),
            other => Err(serde::de::Error::custom(format!(
                "agent kind must be process or app, got {other:?}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Match {
    pub bundle_ids: Vec<String>,
    pub code_signatures: Vec<CodeSignature>,
    pub app_paths: Vec<String>,
    pub windows_paths: Vec<String>,
    pub linux_paths: Vec<String>,
    pub executable_paths: Vec<String>,
    pub process_names: Vec<String>,
    pub parent_process_names: Vec<String>,
    pub command_regex: Vec<String>,
    pub require_command_regex: Vec<String>,
    #[serde(rename = "exclude_process_names")]
    pub exclude_names: Vec<String>,
    pub exclude_command_regex: Vec<String>,
    #[serde(rename = "exclude_parent_command_regex")]
    pub exclude_parent_regex: Vec<String>,
}

impl Match {
    /// Only positive matchers count. An exclude-only matcher matches nothing,
    /// so it is "empty" by design and rejected as a misconfiguration.
    pub fn is_empty(&self) -> bool {
        self.bundle_ids.is_empty()
            && self.code_signatures.is_empty()
            && self.app_paths.is_empty()
            && self.windows_paths.is_empty()
            && self.linux_paths.is_empty()
            && self.executable_paths.is_empty()
            && self.process_names.is_empty()
            && self.parent_process_names.is_empty()
            && self.command_regex.is_empty()
            && self.require_command_regex.is_empty()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CodeSignature {
    pub identifier: String,
    pub team_id: String,
}

/// Per-run policy. Curb now enforces on token turn-spend, not wall-clock, so
/// only `ack_extension` is consumed by the runtime (it bounds an acknowledgement
/// window). The duration fields below (`warn_after`, `kill_after`,
/// `kill_grace_period`, `cooldown_after_kill`, `min_lifetime`, `max_run_gap`)
/// and `allow_app_root_kill` are accepted for config-file compatibility and
/// validated for sanity, but no runtime path reads them; token thresholds live
/// in `UsageConfig`. Treat them as a deletion candidate, not active controls.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub warn_after: HumanDuration,
    pub kill_after: HumanDuration,
    pub ack_extension: HumanDuration,
    pub max_extensions: i64,
    pub kill_grace_period: HumanDuration,
    pub cooldown_after_kill: HumanDuration,
    pub min_lifetime: HumanDuration,
    pub max_run_gap: HumanDuration,
    pub allow_app_root_kill: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            warn_after: HumanDuration::ZERO,
            kill_after: HumanDuration::ZERO,
            ack_extension: HumanDuration::ZERO,
            max_extensions: 0,
            kill_grace_period: HumanDuration::ZERO,
            cooldown_after_kill: HumanDuration::ZERO,
            min_lifetime: HumanDuration::ZERO,
            max_run_gap: HumanDuration::ZERO,
            allow_app_root_kill: false,
        }
    }
}

impl Policy {
    fn set_defaults(&mut self) {
        self.warn_after.default_to(HumanDuration::minutes(90));
        self.kill_after.default_to(HumanDuration::hours(2));
        self.ack_extension.default_to(HumanDuration::minutes(30));
        self.kill_grace_period
            .default_to(HumanDuration::seconds(60));
        self.min_lifetime.default_to(HumanDuration::seconds(10));
        self.max_run_gap.default_to(HumanDuration::seconds(20));
    }

    fn merge_override(&mut self, override_policy: &Policy) {
        self.warn_after.override_with(override_policy.warn_after);
        self.kill_after.override_with(override_policy.kill_after);
        self.ack_extension
            .override_with(override_policy.ack_extension);
        if override_policy.max_extensions != 0 {
            self.max_extensions = override_policy.max_extensions;
        }
        self.kill_grace_period
            .override_with(override_policy.kill_grace_period);
        self.cooldown_after_kill
            .override_with(override_policy.cooldown_after_kill);
        self.min_lifetime
            .override_with(override_policy.min_lifetime);
        self.max_run_gap.override_with(override_policy.max_run_gap);
        if override_policy.allow_app_root_kill {
            self.allow_app_root_kill = true;
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AlertConfig {
    pub local_notifications: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LedgerConfig {
    pub path: PathBuf,
    pub include_prompt_content: bool,
}

/// Absolute path to the committed example config, used by unit tests across the
/// crate. Resolved relative to the workspace root (one level above this crate's
/// manifest dir) so tests pass regardless of the harness working directory.
#[cfg(test)]
pub(crate) fn example_config_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("configs")
        .join("curb.example.yaml")
}
