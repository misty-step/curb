use std::collections::HashSet;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration as StdDuration;

use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

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
        cfg.apply_policy_to_agents();
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

        let mut seen = HashSet::new();
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
        let mut policy = self.defaults.clone();
        if let Some(override_policy) = &agent.policy {
            policy.merge_override(override_policy);
        }
        policy
    }

    pub fn apply_preset(&mut self, preset: Preset) {
        self.keep_process_agents();
        self.service.min_confidence = 50;
        match preset {
            Preset::Aggressive => {
                self.mode = Mode::Enforcement;
                self.service.scan_interval = HumanDuration::seconds(1);
                self.service.heartbeat_interval = HumanDuration::seconds(5);
                self.usage.enabled = Some(true);
                self.usage.scan_interval = HumanDuration::seconds(1);
                self.usage.window = HumanDuration::minutes(1);
                self.usage.warn_turn_tokens = 250_000;
                self.usage.kill_turn_tokens = 750_000;
                self.usage.grace_period = HumanDuration::seconds(10);
                self.defaults.warn_after = HumanDuration::seconds(30);
                self.defaults.kill_after = HumanDuration::seconds(60);
                self.defaults.kill_grace_period = HumanDuration::seconds(10);
                self.defaults.ack_extension = HumanDuration::seconds(30);
                self.defaults.max_extensions = 1;
                self.defaults.min_lifetime = HumanDuration::seconds(1);
                self.defaults.max_run_gap = HumanDuration::seconds(2);
            }
            Preset::Reasonable => {
                self.mode = Mode::Alert;
                self.service.scan_interval = HumanDuration::seconds(15);
                self.service.heartbeat_interval = HumanDuration::minutes(1);
                self.usage.enabled = Some(true);
                self.usage.scan_interval = HumanDuration::seconds(5);
                self.usage.window = HumanDuration::minutes(15);
                self.usage.warn_turn_tokens = 1_000_000;
                self.usage.kill_turn_tokens = 3_000_000;
                self.usage.grace_period = HumanDuration::minutes(1);
                self.defaults.warn_after = HumanDuration::minutes(90);
                self.defaults.kill_after = HumanDuration::hours(2);
                self.defaults.kill_grace_period = HumanDuration::minutes(1);
                self.defaults.ack_extension = HumanDuration::minutes(30);
                self.defaults.max_extensions = 2;
            }
            Preset::Observe => {
                self.mode = Mode::Visibility;
                self.service.scan_interval = HumanDuration::seconds(15);
                self.usage.enabled = Some(true);
                self.usage.scan_interval = HumanDuration::seconds(10);
                self.usage.window = HumanDuration::minutes(15);
                self.usage.warn_turn_tokens = 5_000_000;
                self.usage.kill_turn_tokens = 10_000_000;
                self.usage.grace_period = HumanDuration::minutes(1);
                self.defaults.warn_after = HumanDuration::hours(24);
                self.defaults.kill_after = HumanDuration::hours(48);
                self.defaults.kill_grace_period = HumanDuration::minutes(1);
                self.defaults.ack_extension = HumanDuration::minutes(30);
                self.defaults.max_extensions = 2;
            }
        }
        self.apply_policy_to_agents();
    }

    pub fn refresh_agent_policies(&mut self) {
        self.apply_policy_to_agents();
    }

    fn apply_policy_to_agents(&mut self) {
        for agent in &mut self.agents {
            let mut policy = self.defaults.clone();
            policy.allow_app_root_kill = false;
            agent.policy = Some(policy);
        }
    }

    fn keep_process_agents(&mut self) {
        let mut agents = default_process_agents();
        let mut seen = agents
            .iter()
            .map(|agent| agent.id.clone())
            .collect::<HashSet<_>>();
        for agent in &self.agents {
            if seen.contains(&agent.id) || !agent.termination_allowed() {
                continue;
            }
            let mut agent = agent.clone();
            if agent.kind == AgentKind::Unspecified {
                agent.kind = AgentKind::Process;
            }
            seen.insert(agent.id.clone());
            agents.push(agent);
        }
        self.agents = agents;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Preset {
    Aggressive,
    Reasonable,
    Observe,
}

impl FromStr for Preset {
    type Err = ConfigError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "aggressive" => Ok(Self::Aggressive),
            "reasonable" => Ok(Self::Reasonable),
            "observe" => Ok(Self::Observe),
            other => Err(ConfigError::Preset(other.to_string())),
        }
    }
}

impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Preset::Aggressive => "aggressive",
            Preset::Reasonable => "reasonable",
            Preset::Observe => "observe",
        })
    }
}

fn write_private_file(path: &Path, content: &[u8]) -> Result<(), ConfigError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    use std::io::Write;
    file.write_all(content)
        .map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source,
        })
}

fn set_dir_private(path: &Path) -> Result<(), ConfigError> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
            ConfigError::Write {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    Ok(())
}

fn set_file_private(path: &Path) -> Result<(), ConfigError> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            ConfigError::Write {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    Ok(())
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct HumanDuration(StdDuration);

impl HumanDuration {
    pub const ZERO: Self = Self(StdDuration::ZERO);

    pub const fn seconds(seconds: u64) -> Self {
        Self(StdDuration::from_secs(seconds))
    }

    pub const fn minutes(minutes: u64) -> Self {
        Self::seconds(minutes * 60)
    }

    pub const fn hours(hours: u64) -> Self {
        Self::minutes(hours * 60)
    }

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    pub fn as_std(self) -> StdDuration {
        self.0
    }

    pub fn from_std(duration: StdDuration) -> Self {
        Self(duration)
    }

    fn default_to(&mut self, value: Self) {
        if self.is_zero() {
            *self = value;
        }
    }

    fn override_with(&mut self, value: Self) {
        if !value.is_zero() {
            *self = value;
        }
    }
}

impl Default for HumanDuration {
    fn default() -> Self {
        Self::ZERO
    }
}

impl Serialize for HumanDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format_duration(self.0))
    }
}

impl<'de> Deserialize<'de> for HumanDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        parse_duration(&String::deserialize(deserializer)?)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

fn parse_duration(raw: &str) -> Result<StdDuration, String> {
    if raw.is_empty() {
        return Ok(StdDuration::ZERO);
    }
    let mut rest = raw;
    let mut total = 0u64;
    while !rest.is_empty() {
        let digits = rest
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>();
        if digits.is_empty() {
            return Err(format!("invalid duration {raw:?}"));
        }
        let value = digits
            .parse::<u64>()
            .map_err(|_| format!("invalid duration {raw:?}"))?;
        rest = &rest[digits.len()..];
        let unit = if let Some(stripped) = rest.strip_prefix("ms") {
            rest = stripped;
            total = total.saturating_add(value / 1000);
            continue;
        } else if let Some(stripped) = rest.strip_prefix('h') {
            rest = stripped;
            3600
        } else if let Some(stripped) = rest.strip_prefix('m') {
            rest = stripped;
            60
        } else if let Some(stripped) = rest.strip_prefix('s') {
            rest = stripped;
            1
        } else {
            return Err(format!("invalid duration {raw:?}"));
        };
        total = total.saturating_add(value.saturating_mul(unit));
    }
    Ok(StdDuration::from_secs(total))
}

pub fn parse_duration_for_cli(raw: &str) -> Result<StdDuration, String> {
    parse_duration(raw)
}

fn format_duration(duration: StdDuration) -> String {
    let seconds = duration.as_secs();
    if seconds != 0 && seconds.is_multiple_of(3600) {
        format!("{}h", seconds / 3600)
    } else if seconds != 0 && seconds.is_multiple_of(60) {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
}

/// The user's home directory, derived from the environment.
///
/// Prefers `HOME` (Unix) and falls back to `USERPROFILE` (Windows). Returns
/// `None` when neither is set. Used by path-compaction rendering and by the
/// binary's CLI to resolve default config/state locations.
pub fn default_home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
}

fn default_state_dir() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_STATE_HOME") {
        return PathBuf::from(xdg).join("curb");
    }
    if let Ok(local) = env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("Curb");
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("curb");
    }
    PathBuf::from(".curb")
}

fn default_process_agents() -> Vec<Agent> {
    vec![
        Agent {
            id: "codex-desktop-worker".to_string(),
            label: "Codex Desktop Worker".to_string(),
            family: "codex".to_string(),
            kind: AgentKind::Process,
            matcher: Match {
                process_names: vec!["codex".to_string()],
                require_command_regex: vec![
                    "\\bapp-server\\b".to_string(),
                    "--listen\\s+stdio://".to_string(),
                ],
                command_regex: vec![
                    "\\bapp-server\\b".to_string(),
                    "--listen\\s+stdio://".to_string(),
                ],
                ..Match::default()
            },
            policy: None,
        },
        Agent {
            id: "codex-cli".to_string(),
            label: "Codex CLI".to_string(),
            family: "codex".to_string(),
            kind: AgentKind::Process,
            matcher: Match {
                process_names: vec!["codex".to_string()],
                command_regex: vec!["(^|/|\\\\)codex(\\.js|\\.cmd|\\.exe)?(\\s|$)".to_string()],
                exclude_command_regex: vec!["/Applications/Codex.app".to_string()],
                ..Match::default()
            },
            policy: None,
        },
        Agent {
            id: "claude-code".to_string(),
            label: "Claude Code".to_string(),
            family: "claude".to_string(),
            kind: AgentKind::Process,
            matcher: Match {
                process_names: vec!["claude".to_string(), "claude-code".to_string()],
                command_regex: vec!["(^|/|\\\\)claude(-code)?(\\.cmd|\\.exe)?(\\s|$)".to_string()],
                exclude_command_regex: vec!["/Applications/Claude.app".to_string()],
                exclude_parent_regex: vec![
                    "/Applications/Codex\\.app/.+\\bapp-server\\b".to_string(),
                ],
                ..Match::default()
            },
            policy: None,
        },
        Agent {
            id: "antigravity-cli".to_string(),
            label: "Anti-Gravity CLI".to_string(),
            family: "antigravity".to_string(),
            kind: AgentKind::Process,
            matcher: Match {
                process_names: vec!["agy".to_string()],
                command_regex: vec!["(^|/|\\\\)agy(\\.cmd|\\.exe)?(\\s|$)".to_string()],
                ..Match::default()
            },
            policy: None,
        },
    ]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_example_config_with_defaults() {
        let cfg = Config::load(example_config_path()).unwrap();

        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.mode, Mode::Visibility);
        assert_eq!(cfg.usage.warn_turn_tokens, 1_000_000);
        assert_eq!(cfg.usage.kill_turn_tokens, 3_000_000);
        assert_eq!(cfg.agents.len(), 6);
        assert!(!cfg.ledger.include_prompt_content);
    }

    #[test]
    fn save_round_trips_yaml_with_lowercase_enums() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("curb.yaml");
        let mut cfg = Config::load(example_config_path()).unwrap();
        cfg.mode = Mode::Enforcement;
        cfg.usage.warn_turn_tokens = 2_000;
        cfg.usage.kill_turn_tokens = 4_000;

        cfg.save(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let reloaded = Config::load(&path).unwrap();

        assert!(raw.contains("mode: enforcement"));
        assert!(raw.contains("kind: process"));
        assert_eq!(reloaded.mode, Mode::Enforcement);
        assert_eq!(reloaded.usage.warn_turn_tokens, 2_000);
        assert_eq!(reloaded.usage.kill_turn_tokens, 4_000);
        assert_eq!(reloaded.agents.len(), cfg.agents.len());
        assert_eq!(reloaded.ledger.path, cfg.ledger.path);
    }

    #[test]
    fn local_default_builds_private_process_agent_config() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::local_default(Mode::Alert, dir.path().join("state"));

        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.profile, "local-default");
        assert_eq!(cfg.mode, Mode::Alert);
        assert_eq!(cfg.agents.len(), 4);
        assert!(cfg.agents.iter().all(Agent::termination_allowed));
        assert_eq!(
            cfg.ledger.path,
            dir.path().join("state").join("runs.ndjson")
        );
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn presets_keep_custom_process_agents_and_drop_watch_only_apps() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::local_default(Mode::Visibility, dir.path().join("state"));
        cfg.agents.push(Agent {
            id: "custom-worker".to_string(),
            label: "Custom Worker".to_string(),
            kind: AgentKind::Process,
            matcher: Match {
                process_names: vec!["custom".to_string()],
                ..Match::default()
            },
            ..Agent::default()
        });
        cfg.agents.push(Agent {
            id: "custom-app".to_string(),
            label: "Custom App".to_string(),
            kind: AgentKind::App,
            matcher: Match {
                app_paths: vec!["/Applications/Custom.app".to_string()],
                ..Match::default()
            },
            ..Agent::default()
        });

        cfg.apply_preset(Preset::Aggressive);

        assert_eq!(cfg.mode, Mode::Enforcement);
        assert_eq!(cfg.usage.warn_turn_tokens, 250_000);
        assert!(cfg.agents.iter().any(|agent| agent.id == "custom-worker"));
        assert!(!cfg.agents.iter().any(|agent| agent.id == "custom-app"));
        assert!(cfg.agents.iter().all(|agent| {
            agent
                .policy
                .as_ref()
                .is_some_and(|policy| !policy.allow_app_root_kill)
        }));
    }

    #[test]
    fn save_validates_before_replacing_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("curb.yaml");
        let mut cfg = Config::load(example_config_path()).unwrap();
        cfg.save(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        cfg.usage.warn_turn_tokens = cfg.usage.kill_turn_tokens + 1;

        let err = cfg.save(&path).unwrap_err();

        assert!(matches!(err, ConfigError::InvalidUsageThresholds));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            std::fs::read_dir(dir.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
                .count(),
            0
        );
    }

    #[test]
    fn rejects_prompt_capture() {
        let err = load_from_str(
            r#"
version: 1
ledger:
  include_prompt_content: true
agents:
  - id: codex
    label: Codex
    match:
      process_names: [codex]
"#,
        )
        .unwrap_err();

        assert!(matches!(err, ConfigError::PromptCaptureUnsupported));
    }

    #[test]
    fn rejects_unimplemented_egress_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("curb.yaml");
        std::fs::write(
            &path,
            r#"
version: 1
alerts:
  webhook_url: https://example.invalid/curb/alerts
  slack_webhook_url: https://example.invalid/slack
ledger:
  forward_url: https://example.invalid/curb/events
agents:
  - id: codex
    label: Codex
    match:
      process_names: [codex]
"#,
        )
        .unwrap();

        let err = Config::load(&path).unwrap_err();

        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn rejects_duplicate_agent_ids() {
        let err = load_from_str(
            r#"
version: 1
agents:
  - id: codex
    label: Codex
    match:
      process_names: [codex]
  - id: codex
    label: Codex Again
    match:
      process_names: [codex]
"#,
        )
        .unwrap_err();

        assert!(matches!(err, ConfigError::DuplicateAgentId(id) if id == "codex"));
    }

    #[test]
    fn desktop_app_roots_are_not_termination_targets_by_default() {
        let agent = Agent {
            id: "codex-desktop".to_string(),
            label: "Codex Desktop".to_string(),
            matcher: Match {
                bundle_ids: vec!["com.openai.codex".to_string()],
                ..Match::default()
            },
            ..Agent::default()
        };

        assert!(!agent.termination_allowed());
    }

    #[test]
    fn supervised_desktop_worker_is_watch_only_unless_escalated() {
        let worker = default_process_agents()
            .into_iter()
            .find(|agent| agent.id == "codex-desktop-worker")
            .expect("codex-desktop-worker is a default agent");
        // It is a real process, but supervised — futile to kill the leaf.
        assert!(worker.termination_allowed());
        assert!(worker.is_supervised());
        assert!(!worker.can_terminate(false));
        assert!(worker.can_terminate(true));

        let cli = default_process_agents()
            .into_iter()
            .find(|agent| agent.id == "codex-cli")
            .expect("codex-cli is a default agent");
        assert!(!cli.is_supervised());
        assert!(cli.can_terminate(false));
    }

    #[test]
    fn parses_composite_duration_like_go() {
        assert_eq!(
            parse_duration("1h30m").unwrap(),
            StdDuration::from_secs(90 * 60)
        );
        assert_eq!(
            parse_duration("15m10s").unwrap(),
            StdDuration::from_secs(15 * 60 + 10)
        );
    }

    fn load_from_str(raw: &str) -> Result<Config, ConfigError> {
        let mut cfg: Config = serde_yaml::from_str(raw).unwrap();
        cfg.set_defaults();
        cfg.validate()?;
        Ok(cfg)
    }
}
