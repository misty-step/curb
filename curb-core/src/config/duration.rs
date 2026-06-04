use std::time::Duration as StdDuration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

    pub(super) fn default_to(&mut self, value: Self) {
        if self.is_zero() {
            *self = value;
        }
    }

    pub(super) fn override_with(&mut self, value: Self) {
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

pub(super) fn parse_duration(raw: &str) -> Result<StdDuration, String> {
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
