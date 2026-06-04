use std::fmt;
use std::str::FromStr;

use super::{Config, ConfigError, HumanDuration, Mode, policy_merge};

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
            Self::Aggressive => "aggressive",
            Self::Reasonable => "reasonable",
            Self::Observe => "observe",
        })
    }
}

pub(super) fn apply(config: &mut Config, preset: Preset) {
    policy_merge::keep_process_agents(config);
    config.service.min_confidence = 50;
    match preset {
        Preset::Aggressive => apply_aggressive(config),
        Preset::Reasonable => apply_reasonable(config),
        Preset::Observe => apply_observe(config),
    }
    policy_merge::refresh_agent_policies(config);
}

fn apply_aggressive(config: &mut Config) {
    config.mode = Mode::Enforcement;
    config.service.scan_interval = HumanDuration::seconds(1);
    config.service.heartbeat_interval = HumanDuration::seconds(5);
    config.usage.enabled = Some(true);
    config.usage.scan_interval = HumanDuration::seconds(1);
    config.usage.window = HumanDuration::minutes(1);
    config.usage.warn_turn_tokens = 250_000;
    config.usage.kill_turn_tokens = 750_000;
    config.usage.grace_period = HumanDuration::seconds(10);
    config.defaults.warn_after = HumanDuration::seconds(30);
    config.defaults.kill_after = HumanDuration::seconds(60);
    config.defaults.kill_grace_period = HumanDuration::seconds(10);
    config.defaults.ack_extension = HumanDuration::seconds(30);
    config.defaults.max_extensions = 1;
    config.defaults.min_lifetime = HumanDuration::seconds(1);
    config.defaults.max_run_gap = HumanDuration::seconds(2);
}

fn apply_reasonable(config: &mut Config) {
    config.mode = Mode::Alert;
    config.service.scan_interval = HumanDuration::seconds(15);
    config.service.heartbeat_interval = HumanDuration::minutes(1);
    config.usage.enabled = Some(true);
    config.usage.scan_interval = HumanDuration::seconds(5);
    config.usage.window = HumanDuration::minutes(15);
    config.usage.warn_turn_tokens = 1_000_000;
    config.usage.kill_turn_tokens = 3_000_000;
    config.usage.grace_period = HumanDuration::minutes(1);
    config.defaults.warn_after = HumanDuration::minutes(90);
    config.defaults.kill_after = HumanDuration::hours(2);
    config.defaults.kill_grace_period = HumanDuration::minutes(1);
    config.defaults.ack_extension = HumanDuration::minutes(30);
    config.defaults.max_extensions = 2;
}

fn apply_observe(config: &mut Config) {
    config.mode = Mode::Visibility;
    config.service.scan_interval = HumanDuration::seconds(15);
    config.usage.enabled = Some(true);
    config.usage.scan_interval = HumanDuration::seconds(10);
    config.usage.window = HumanDuration::minutes(15);
    config.usage.warn_turn_tokens = 5_000_000;
    config.usage.kill_turn_tokens = 10_000_000;
    config.usage.grace_period = HumanDuration::minutes(1);
    config.defaults.warn_after = HumanDuration::hours(24);
    config.defaults.kill_after = HumanDuration::hours(48);
    config.defaults.kill_grace_period = HumanDuration::minutes(1);
    config.defaults.ack_extension = HumanDuration::minutes(30);
    config.defaults.max_extensions = 2;
}
