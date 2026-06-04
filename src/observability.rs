use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use curb_core::governor::GovernorReport;
use curb_core::service::Snapshot;
use serde_json::{Map, json};

mod event;
mod registry;

#[cfg(test)]
use event::SCHEMA_VERSION;
use event::{LogEvent, json_logs_enabled, sanitize_reason};
#[cfg(test)]
use registry::registered_events;
use registry::{event_registered, outcome_for_status, path_template, request_event_name};

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

pub fn emit_server_started(url: &str, headless: bool) {
    emit(&server_started_event(url, headless));
}

pub fn emit_config_loaded(path: &Path, mode: &str, agent_count: usize) {
    let mut fields = Map::new();
    fields.insert("path".to_string(), json!(path.display().to_string()));
    fields.insert("mode".to_string(), json!(mode));
    fields.insert("agent_count".to_string(), json!(agent_count));
    emit(&LogEvent::new("info", "config", "config_loaded", "ok").with_fields(fields));
}

pub fn emit_api_request(method: &str, path: &str, status: u16, duration: Duration) {
    emit(&api_request_event(method, path, status, duration));
}

pub fn emit_usage_tick_report(
    report: &curb_core::runtime::UsageTickReport,
    event: &'static str,
    duration: Option<Duration>,
) {
    let log = usage_scan_result_event(&report.snapshot, Some(&report.governor), event, duration);
    emit(&log);
    emit_source_health_errors(&report.snapshot);
}

fn emit_source_health_errors(snapshot: &Snapshot) {
    for source in snapshot
        .overview
        .sources
        .iter()
        .filter(|source| source.error.is_some())
    {
        emit(&source_health_error_event(
            &source.provider,
            source.error.as_deref().unwrap_or("source unavailable"),
        ));
    }
}

fn usage_scan_result_event(
    snapshot: &Snapshot,
    governor: Option<&GovernorReport>,
    event: &'static str,
    duration: Option<Duration>,
) -> LogEvent {
    let mut fields = Map::new();
    fields.insert(
        "status".to_string(),
        json!(snapshot.overview.status.as_str()),
    );
    fields.insert("working".to_string(), json!(snapshot.overview.working));
    fields.insert("warn".to_string(), json!(snapshot.overview.warn));
    fields.insert("kill".to_string(), json!(snapshot.overview.kill));
    fields.insert(
        "source_errors".to_string(),
        json!(snapshot.overview.changes.source_errors),
    );
    fields.insert("event_count".to_string(), json!(snapshot.turns.len()));
    fields.insert("process_count".to_string(), json!(snapshot.agents.len()));
    if let Some(governor) = governor {
        fields.insert(
            "observed_sessions".to_string(),
            json!(governor.observed_sessions),
        );
        fields.insert(
            "policy_warnings".to_string(),
            json!(governor.policy.warnings),
        );
        fields.insert("would_stop".to_string(), json!(governor.policy.would_stop));
        fields.insert(
            "stop_blocked".to_string(),
            json!(governor.policy.stop_blocked),
        );
        fields.insert(
            "grace_started".to_string(),
            json!(governor.policy.grace_started),
        );
        fields.insert(
            "grace_pending".to_string(),
            json!(governor.policy.grace_pending),
        );
        fields.insert(
            "stop_attempted".to_string(),
            json!(governor.policy.stop_attempted),
        );
        fields.insert(
            "stop_completed".to_string(),
            json!(governor.policy.stop_completed),
        );
        fields.insert(
            "stop_rejected".to_string(),
            json!(governor.policy.stop_rejected),
        );
        fields.insert(
            "resumed_sessions".to_string(),
            json!(governor.policy.resumed_sessions),
        );
        fields.insert(
            "terminated_sessions".to_string(),
            json!(governor.policy.terminated_sessions),
        );
    }
    let mut log = LogEvent::new(
        "info",
        "runtime",
        event,
        if snapshot.overview.changes.source_errors == 0 {
            "ok"
        } else {
            "degraded"
        },
    )
    .with_fields(fields);
    if let Some(duration) = duration {
        log = log.with_duration(duration);
    }
    log
}

pub fn emit_usage_scan_failure(event: &'static str, error: &str, duration: Duration) {
    emit(&usage_scan_failure_event(event, error, duration));
}

fn usage_scan_failure_event(event: &'static str, error: &str, duration: Duration) -> LogEvent {
    LogEvent::new("error", "runtime", event, "error")
        .with_duration(duration)
        .with_reason(sanitize_reason(error))
}

pub fn emit_notification_attempt(status: u16) {
    let mut fields = Map::new();
    fields.insert("status".to_string(), json!(status));
    emit(
        &LogEvent::new(
            "info",
            "runtime",
            "notification_attempt",
            outcome_for_status(status),
        )
        .with_fields(fields),
    );
}

pub fn emit_stop_outcome(status: u16) {
    emit(&stop_outcome_event(status));
}

fn stop_outcome_event(status: u16) -> LogEvent {
    let mut fields = Map::new();
    fields.insert("status".to_string(), json!(status));
    let event = if status < 400 {
        "stop_decision"
    } else {
        "stop_rejection"
    };
    LogEvent::new("info", "policy", event, outcome_for_status(status)).with_fields(fields)
}

pub fn emit_shutdown(component: &'static str, reason: &str) {
    emit(&shutdown_event(component, reason));
}

fn shutdown_event(component: &'static str, reason: &str) -> LogEvent {
    LogEvent::new("info", component, "shutdown", "ok").with_reason(sanitize_reason(reason))
}

pub fn write_event(mut writer: impl Write, event: &LogEvent) -> io::Result<()> {
    if !event_registered(event.event, event.component) {
        return Err(io::Error::other(format!(
            "unregistered observability event {}:{}",
            event.component, event.event
        )));
    }
    serde_json::to_writer(&mut writer, event).map_err(io::Error::other)?;
    writer.write_all(b"\n")
}

fn emit(event: &LogEvent) {
    if json_logs_enabled() {
        let _ = write_event(io::stderr().lock(), event);
    }
}

fn server_started_event(url: &str, headless: bool) -> LogEvent {
    let mut fields = Map::new();
    fields.insert("url".to_string(), json!(url));
    fields.insert(
        "mode".to_string(),
        json!(if headless { "headless" } else { "ui" }),
    );
    fields.insert("ui_enabled".to_string(), json!(!headless));
    LogEvent::new("info", "server", "server_started", "ok").with_fields(fields)
}

fn api_request_event(method: &str, path: &str, status: u16, duration: Duration) -> LogEvent {
    let mut fields = Map::new();
    fields.insert("method".to_string(), json!(method));
    fields.insert("path_template".to_string(), json!(path_template(path)));
    fields.insert("status".to_string(), json!(status));
    LogEvent::new(
        "info",
        "http",
        request_event_name(path),
        outcome_for_status(status),
    )
    .with_duration(duration)
    .with_request_id(next_request_id())
    .with_fields(fields)
}

fn source_health_error_event(provider: &str, reason: &str) -> LogEvent {
    let mut fields = Map::new();
    fields.insert("provider".to_string(), json!(provider));
    LogEvent::new("warn", "runtime", "source_health_error", "error")
        .with_reason(sanitize_reason(reason))
        .with_fields(fields)
}

fn next_request_id() -> String {
    format!("req-{}", NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::Utc;
    use curb_core::governor::GovernorReport;
    use curb_core::onboarding::PlatformCapabilities;
    use curb_core::service::{AgentView, Overview, OverviewDelta, Snapshot, TurnView};
    use curb_core::usage::SourceReport;
    use curb_core::usagewatch::PolicyScanReport;
    use serde_json::Value;

    use super::*;

    #[test]
    fn json_event_has_required_schema_fields() {
        let event = server_started_event("http://127.0.0.1:8765/", true);
        let mut out = Vec::new();

        write_event(&mut out, &event).unwrap();
        let parsed: Value = serde_json::from_slice(&out).unwrap();

        assert_eq!(parsed["schema_version"], SCHEMA_VERSION);
        assert!(parsed["timestamp"].as_str().unwrap().ends_with('Z'));
        assert_eq!(parsed["level"], "info");
        assert_eq!(parsed["component"], "server");
        assert_eq!(parsed["event"], "server_started");
        assert_eq!(parsed["outcome"], "ok");
        assert_eq!(parsed["fields"]["ui_enabled"], false);
    }

    #[test]
    fn request_events_redact_queries_and_session_keys() {
        let event = api_request_event(
            "POST",
            "/v1/sessions/codex:secret-session/stop?token=secret",
            409,
            Duration::from_millis(12),
        );
        let encoded = serde_json::to_string(&event).unwrap();

        assert_eq!(
            event.fields["path_template"],
            "/v1/sessions/{session_key}/stop"
        );
        assert_eq!(event.outcome, "rejected");
        assert!(event.request_id.as_deref().unwrap().starts_with("req-"));
        assert!(!encoded.contains("secret-session"));
        assert!(!encoded.contains("token=secret"));
    }

    #[test]
    fn emitted_events_must_be_registered() {
        let mut event = LogEvent::new("info", "test", "unlisted", "ok");

        let error = write_event(Vec::new(), &event).unwrap_err();
        assert!(error.to_string().contains("unregistered"));

        event.component = "http";
        event.event = "api_request";
        write_event(Vec::new(), &event).unwrap();
    }

    #[test]
    fn registry_covers_every_emitted_event() {
        let emitted = [
            server_started_event("http://127.0.0.1:8765/", true),
            LogEvent::new("info", "config", "config_loaded", "ok"),
            api_request_event("GET", "/v1/snapshot", 200, Duration::from_millis(1)),
            api_request_event("GET", "/v1/health", 200, Duration::from_millis(1)),
            api_request_event("GET", "/v1/ready", 200, Duration::from_millis(1)),
            usage_scan_result_event(&test_snapshot(), None, "usage_scan", None),
            usage_scan_result_event(&test_snapshot(), None, "watcher_tick", None),
            LogEvent::new("info", "runtime", "notification_attempt", "ok"),
            stop_outcome_event(200),
            stop_outcome_event(409),
            source_health_error_event("codex", "failed"),
            shutdown_event("server", "done"),
        ];
        let registry = registered_events();

        for event in emitted {
            assert!(
                registry
                    .iter()
                    .any(|entry| entry.event == event.event && entry.component == event.component),
                "missing registry entry for {}:{}",
                event.component,
                event.event
            );
        }
    }

    #[test]
    fn source_health_errors_are_registered_and_sanitized() {
        let event = source_health_error_event("codex", "bad token\nsecret");
        let encoded = serde_json::to_string(&event).unwrap();

        assert_eq!(event.event, "source_health_error");
        assert_eq!(event.component, "runtime");
        assert_eq!(event.reason.as_deref(), Some("bad tokensecret"));
        assert!(!encoded.contains('\n'));
        write_event(Vec::new(), &event).unwrap();
    }

    #[test]
    fn stop_and_notification_events_are_registered_without_operator_reasons() {
        let stop_success = stop_outcome_event(200);
        let stop_rejection = stop_outcome_event(409);
        let notification = LogEvent::new("info", "runtime", "notification_attempt", "ok");

        let encoded = serde_json::to_string(&stop_rejection).unwrap();
        assert_eq!(stop_success.event, "stop_decision");
        assert_eq!(stop_success.outcome, "ok");
        assert_eq!(stop_rejection.event, "stop_rejection");
        assert_eq!(stop_rejection.outcome, "rejected");
        assert!(!encoded.contains("session_key"));
        assert!(!encoded.contains("Manual stop"));
        write_event(Vec::new(), &stop_success).unwrap();
        write_event(Vec::new(), &stop_rejection).unwrap();
        write_event(Vec::new(), &notification).unwrap();
    }

    #[test]
    fn runtime_scan_events_include_counts_duration_and_sanitized_failures() {
        let mut snapshot = test_snapshot();
        snapshot.overview.changes.source_errors = 1;
        snapshot.turns.truncate(1);
        snapshot.agents.truncate(1);

        let event = usage_scan_result_event(
            &snapshot,
            None,
            "usage_scan",
            Some(Duration::from_millis(34)),
        );
        assert_eq!(event.outcome, "degraded");
        assert_eq!(event.duration_ms, Some(34));
        assert_eq!(event.fields["source_errors"], 1);
        assert_eq!(event.fields["event_count"], 1);
        assert_eq!(event.fields["process_count"], 1);
        write_event(Vec::new(), &event).unwrap();

        let failure = usage_scan_failure_event(
            "watcher_tick",
            "failed on token=secret\nAuthorization: Bearer nope",
            Duration::from_millis(5),
        );
        let encoded = serde_json::to_string(&failure).unwrap();
        assert_eq!(failure.level, "error");
        assert_eq!(failure.outcome, "error");
        assert_eq!(failure.duration_ms, Some(5));
        assert!(!encoded.contains('\n'));
        write_event(Vec::new(), &failure).unwrap();
    }

    #[test]
    fn runtime_scan_events_include_policy_outcomes_when_reported() {
        let snapshot = test_snapshot();
        let governor = GovernorReport {
            observed_sessions: 2,
            policy: PolicyScanReport {
                warnings: 1,
                would_stop: 2,
                stop_blocked: 3,
                grace_started: 4,
                grace_pending: 5,
                stop_attempted: 6,
                stop_completed: 7,
                stop_rejected: 8,
                resumed_sessions: 9,
                terminated_sessions: 10,
                ..PolicyScanReport::default()
            },
            ..GovernorReport::default()
        };

        let event = usage_scan_result_event(
            &snapshot,
            Some(&governor),
            "watcher_tick",
            Some(Duration::from_millis(7)),
        );

        assert_eq!(event.fields["observed_sessions"], 2);
        assert_eq!(event.fields["policy_warnings"], 1);
        assert_eq!(event.fields["would_stop"], 2);
        assert_eq!(event.fields["stop_blocked"], 3);
        assert_eq!(event.fields["grace_started"], 4);
        assert_eq!(event.fields["grace_pending"], 5);
        assert_eq!(event.fields["stop_attempted"], 6);
        assert_eq!(event.fields["stop_completed"], 7);
        assert_eq!(event.fields["stop_rejected"], 8);
        assert_eq!(event.fields["resumed_sessions"], 9);
        assert_eq!(event.fields["terminated_sessions"], 10);
        write_event(Vec::new(), &event).unwrap();
    }

    #[test]
    fn health_and_shutdown_events_are_registered() {
        let health = api_request_event("GET", "/v1/health", 200, Duration::from_millis(1));
        let shutdown = shutdown_event("server", "Ctrl-C");

        assert_eq!(health.event, "health_check");
        assert_eq!(health.fields["path_template"], "/v1/health");
        assert_eq!(shutdown.event, "shutdown");
        assert_eq!(shutdown.component, "server");
        write_event(Vec::new(), &health).unwrap();
        write_event(Vec::new(), &shutdown).unwrap();
    }

    fn test_snapshot() -> Snapshot {
        Snapshot {
            overview: Overview {
                mode: "alert".to_string(),
                status: "WATCH".to_string(),
                message: "1 agent needs attention".to_string(),
                working: 1,
                warn: 1,
                kill: 0,
                busiest_turn_tokens: 1000,
                last_scan: Utc::now(),
                sources: vec![SourceReport {
                    provider: "codex".to_string(),
                    files: 1,
                    events: 1,
                    error: Some("bad provider source".to_string()),
                }],
                changes: OverviewDelta {
                    source_errors: 1,
                    ..OverviewDelta::default()
                },
                capabilities: PlatformCapabilities::default(),
            },
            agents: vec![AgentView {
                id: "codex-worker".to_string(),
                provider: "codex".to_string(),
                label: "Codex".to_string(),
                status: "working".to_string(),
                pid: 123,
                turn_tokens: 1000,
                explanation: "matched worker".to_string(),
                ..AgentView::default()
            }],
            sessions: Vec::new(),
            turns: vec![TurnView {
                id: None,
                request_id: None,
                session_key: "codex:session".to_string(),
                session_id: Some("session".to_string()),
                provider: "codex".to_string(),
                at: Some(Utc::now()),
                model: Some("model".to_string()),
                input_tokens: 1,
                cached_input_tokens: 0,
                output_tokens: 1,
                cache_creation_input_tokens: 0,
                reasoning_output_tokens: 0,
                total_tokens: 2,
                spent_tokens: 2,
                cumulative_tokens: 2,
                source: "test".to_string(),
            }],
        }
    }
}
