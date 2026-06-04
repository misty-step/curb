use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{ApiError, Request};
use curb_core::service::{AckRequest, ConfigUpdate, StopExpectedIdentity, StopRequest};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct WireAckRequest {
    extend_seconds: i64,
    reason: String,
}

impl From<WireAckRequest> for AckRequest {
    fn from(request: WireAckRequest) -> Self {
        Self {
            extend_seconds: request.extend_seconds,
            reason: request.reason,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct WireStopRequest {
    confirm: bool,
    scope: String,
    reason: String,
    expected: WireStopExpectedIdentity,
}

impl From<WireStopRequest> for StopRequest {
    fn from(request: WireStopRequest) -> Self {
        Self {
            confirm: request.confirm,
            scope: request.scope,
            reason: request.reason,
            expected: request.expected.into(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct WireStopExpectedIdentity {
    pid: i32,
    started_at: Option<DateTime<Utc>>,
    owner: String,
    executable: Option<PathBuf>,
    bundle_id: Option<String>,
    team_id: Option<String>,
}

impl From<WireStopExpectedIdentity> for StopExpectedIdentity {
    fn from(expected: WireStopExpectedIdentity) -> Self {
        Self {
            pid: expected.pid,
            started_at: expected.started_at,
            owner: expected.owner,
            executable: expected.executable,
            bundle_id: expected.bundle_id,
            team_id: expected.team_id,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct WireConfigUpdate {
    mode: Option<WireConfigMode>,
    usage_enabled: Option<bool>,
    warn_turn_tokens: Option<i64>,
    kill_turn_tokens: Option<i64>,
    usage_window_seconds: Option<i64>,
    usage_scan_seconds: Option<i64>,
    lookback_seconds: Option<i64>,
    process_warn_seconds: Option<i64>,
    process_kill_seconds: Option<i64>,
    local_notifications: Option<bool>,
    escalate_supervised: Option<bool>,
}

impl From<WireConfigUpdate> for ConfigUpdate {
    fn from(update: WireConfigUpdate) -> Self {
        Self {
            mode: update.mode.map(WireConfigMode::into_config_value),
            usage_enabled: update.usage_enabled,
            warn_turn_tokens: update.warn_turn_tokens,
            kill_turn_tokens: update.kill_turn_tokens,
            usage_window_seconds: update.usage_window_seconds,
            usage_scan_seconds: update.usage_scan_seconds,
            lookback_seconds: update.lookback_seconds,
            process_warn_seconds: update.process_warn_seconds,
            process_kill_seconds: update.process_kill_seconds,
            local_notifications: update.local_notifications,
            escalate_supervised: update.escalate_supervised,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum WireConfigMode {
    Visibility,
    Alert,
    Enforcement,
}

impl WireConfigMode {
    fn into_config_value(self) -> String {
        match self {
            Self::Visibility => "visibility",
            Self::Alert => "alert",
            Self::Enforcement => "enforcement",
        }
        .to_string()
    }
}

pub(super) fn decode_ack(request: &Request) -> Result<AckRequest, ApiError> {
    serde_json::from_slice::<WireAckRequest>(&request.body)
        .map(Into::into)
        .map_err(|error| ApiError::InvalidAck(error.to_string()))
}

pub(super) fn decode_stop(request: &Request) -> Result<StopRequest, ApiError> {
    serde_json::from_slice::<WireStopRequest>(&request.body)
        .map(Into::into)
        .map_err(|error| ApiError::InvalidStop(error.to_string()))
}

pub(super) fn decode_config_update(request: &Request) -> Result<ConfigUpdate, ApiError> {
    serde_json::from_slice::<WireConfigUpdate>(&request.body)
        .map(Into::into)
        .map_err(|error| ApiError::InvalidConfig(error.to_string()))
}
