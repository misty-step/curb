#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventRegistration {
    pub event: &'static str,
    pub component: &'static str,
}

const EVENT_REGISTRY: &[EventRegistration] = &[
    EventRegistration {
        event: "server_started",
        component: "server",
    },
    EventRegistration {
        event: "config_loaded",
        component: "config",
    },
    EventRegistration {
        event: "api_request",
        component: "http",
    },
    EventRegistration {
        event: "health_check",
        component: "http",
    },
    EventRegistration {
        event: "readiness_check",
        component: "http",
    },
    EventRegistration {
        event: "usage_scan",
        component: "runtime",
    },
    EventRegistration {
        event: "watcher_tick",
        component: "runtime",
    },
    EventRegistration {
        event: "notification_attempt",
        component: "runtime",
    },
    EventRegistration {
        event: "stop_decision",
        component: "policy",
    },
    EventRegistration {
        event: "stop_rejection",
        component: "policy",
    },
    EventRegistration {
        event: "source_health_error",
        component: "runtime",
    },
    EventRegistration {
        event: "shutdown",
        component: "server",
    },
];

pub fn registered_events() -> &'static [EventRegistration] {
    EVENT_REGISTRY
}

pub fn event_registered(event: &str, component: &str) -> bool {
    registered_events()
        .iter()
        .any(|entry| entry.event == event && entry.component == component)
}

pub fn request_event_name(path: &str) -> &'static str {
    match path {
        "/v1/health" => "health_check",
        "/v1/ready" => "readiness_check",
        _ => "api_request",
    }
}

pub fn outcome_for_status(status: u16) -> &'static str {
    if status < 400 {
        "ok"
    } else if status < 500 {
        "rejected"
    } else {
        "error"
    }
}

pub fn path_template(path: &str) -> String {
    let path = path.split('?').next().unwrap_or(path);
    if path.starts_with("/v1/sessions/") {
        let action = path.rsplit('/').next().unwrap_or_default();
        return match action {
            "ack" | "stop" | "turns" => format!("/v1/sessions/{{session_key}}/{action}"),
            _ => "/v1/sessions/{session_key}".to_string(),
        };
    }
    path.to_string()
}
