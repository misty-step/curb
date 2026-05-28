use std::collections::HashSet;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration as StdDuration;

use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use url::Url;

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
    #[error("defaults.warn_after must be less than defaults.kill_after")]
    InvalidDefaultThresholds,
    #[error("ledger.include_prompt_content is not supported by launch implementation")]
    PromptCaptureUnsupported,
    #[error("ledger.forward_url must use http or https")]
    ForwardUrlScheme,
    #[error("ledger.forward_url must include a host")]
    ForwardUrlHost,
    #[error("ledger.forward_url: {0}")]
    ForwardUrl(url::ParseError),
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
        if !self.ledger.forward_url.is_empty() {
            validate_forward_url(&self.ledger.forward_url)?;
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

fn validate_forward_url(raw: &str) -> Result<(), ConfigError> {
    let parsed = Url::parse(raw).map_err(ConfigError::ForwardUrl)?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(ConfigError::ForwardUrlScheme);
    }
    if parsed.host_str().is_none() {
        return Err(ConfigError::ForwardUrlHost);
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
        self.window.default_to(HumanDuration::minutes(15));
        if self.warn_turn_tokens == 0 {
            self.warn_turn_tokens = 1_000_000;
        }
        if self.kill_turn_tokens == 0 {
            self.kill_turn_tokens = 3_000_000;
        }
        self.grace_period.default_to(HumanDuration::seconds(60));
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
    pub webhook_url: String,
    pub slack_webhook_url: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LedgerConfig {
    pub path: PathBuf,
    pub include_prompt_content: bool,
    pub forward_url: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_example_config_with_defaults() {
        let cfg = Config::load("configs/curb.example.yaml").unwrap();

        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.mode, Mode::Visibility);
        assert_eq!(cfg.usage.warn_turn_tokens, 1_000_000);
        assert_eq!(cfg.usage.kill_turn_tokens, 3_000_000);
        assert_eq!(cfg.agents.len(), 5);
        assert!(!cfg.ledger.include_prompt_content);
    }

    #[test]
    fn save_round_trips_yaml_with_lowercase_enums() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("curb.yaml");
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
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
    fn save_validates_before_replacing_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("curb.yaml");
        let mut cfg = Config::load("configs/curb.example.yaml").unwrap();
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
    fn rejects_invalid_forward_url() {
        let err = load_from_str(
            r#"
version: 1
ledger:
  forward_url: file:///tmp/events
agents:
  - id: codex
    label: Codex
    match:
      process_names: [codex]
"#,
        )
        .unwrap_err();

        assert!(matches!(err, ConfigError::ForwardUrlScheme));
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
