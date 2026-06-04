use std::env;
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Map, Value};

pub const SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Serialize)]
pub struct LogEvent {
    pub schema_version: u8,
    pub timestamp: String,
    pub level: &'static str,
    pub component: &'static str,
    pub event: &'static str,
    pub outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub fields: Map<String, Value>,
}

impl LogEvent {
    pub(crate) fn new(
        level: &'static str,
        component: &'static str,
        event: &'static str,
        outcome: &'static str,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            level,
            component,
            event,
            outcome,
            duration_ms: None,
            request_id: None,
            session_key: None,
            reason: None,
            fields: Map::new(),
        }
    }

    pub(crate) fn with_duration(mut self, duration: Duration) -> Self {
        self.duration_ms = Some(duration.as_millis());
        self
    }

    pub(crate) fn with_request_id(mut self, request_id: String) -> Self {
        self.request_id = Some(request_id);
        self
    }

    pub(crate) fn with_fields(mut self, fields: Map<String, Value>) -> Self {
        self.fields = fields;
        self
    }

    pub(crate) fn with_reason(mut self, reason: String) -> Self {
        self.reason = Some(reason);
        self
    }
}

pub fn json_logs_enabled() -> bool {
    env::var("CURB_LOG_FORMAT").is_ok_and(|value| {
        value.eq_ignore_ascii_case("json") || value.eq_ignore_ascii_case("ndjson")
    })
}

pub fn sanitize_reason(reason: &str) -> String {
    reason
        .chars()
        .filter(|ch| !ch.is_control())
        .take(240)
        .collect()
}
