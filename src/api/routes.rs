use chrono::{DateTime, Utc};
use url::Url;

use super::Request;
use curb_core::config::parse_duration_for_cli;
use curb_core::runtime::TurnQuery;

pub(super) enum PublicRoute {
    Live,
    Ready,
    MethodNotAllowed,
    NotFound,
}

pub(super) enum Route {
    Health,
    Snapshot,
    Overview,
    Agents,
    Sessions,
    Rescan,
    Events { limit: usize },
    Alerts { limit: usize },
    NotificationHealth,
    NotificationTest,
    Config,
    UpdateConfig,
    Onboarding,
    CompleteOnboarding,
    Session { key: String },
    SessionTurns { key: String, query: TurnQuery },
    Ack { key: String },
    Stop { key: String },
    InvalidSessionKey,
    MethodNotAllowed,
    NotFound,
}

pub(super) fn split_target(target: &str) -> (String, String) {
    let target = if let Ok(url) = Url::parse(target) {
        let mut out = url.path().to_string();
        if out.is_empty() {
            out = "/".to_string();
        }
        return (out, url.query().unwrap_or_default().to_string());
    } else {
        target.to_string()
    };
    match target.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (target, String::new()),
    }
}

pub(super) fn is_public(path: &str) -> bool {
    matches!(path, "/v1/live" | "/v1/ready")
}

pub(super) fn public(request: &Request) -> PublicRoute {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/v1/live") => PublicRoute::Live,
        (_, "/v1/live") => PublicRoute::MethodNotAllowed,
        ("GET", "/v1/ready") => PublicRoute::Ready,
        (_, "/v1/ready") => PublicRoute::MethodNotAllowed,
        _ => PublicRoute::NotFound,
    }
}

pub(super) fn protected(request: &Request, now: DateTime<Utc>) -> Route {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/v1/health") => Route::Health,
        ("GET", "/v1/snapshot") => Route::Snapshot,
        ("GET", "/v1/overview") => Route::Overview,
        ("GET", "/v1/agents") => Route::Agents,
        ("GET", "/v1/sessions") => Route::Sessions,
        ("POST", "/v1/service/rescan") => Route::Rescan,
        (_, "/v1/service/rescan") => Route::MethodNotAllowed,
        _ if request.path.starts_with("/v1/sessions/") => session(request, now),
        ("GET", "/v1/events") => Route::Events {
            limit: limit_query(request, 200),
        },
        (_, "/v1/events") => Route::MethodNotAllowed,
        ("GET", "/v1/alerts") => Route::Alerts {
            limit: limit_query(request, 50),
        },
        (_, "/v1/alerts") => Route::MethodNotAllowed,
        ("GET", "/v1/notifications/health") => Route::NotificationHealth,
        (_, "/v1/notifications/health") => Route::MethodNotAllowed,
        ("POST", "/v1/notifications/test") => Route::NotificationTest,
        (_, "/v1/notifications/test") => Route::MethodNotAllowed,
        ("GET", "/v1/config") => Route::Config,
        ("PUT", "/v1/config") => Route::UpdateConfig,
        (_, "/v1/config") => Route::MethodNotAllowed,
        ("GET", "/v1/onboarding") => Route::Onboarding,
        (_, "/v1/onboarding") => Route::MethodNotAllowed,
        ("POST", "/v1/onboarding/complete") => Route::CompleteOnboarding,
        (_, "/v1/onboarding/complete") => Route::MethodNotAllowed,
        ("GET", _) => Route::NotFound,
        _ => Route::MethodNotAllowed,
    }
}

fn session(request: &Request, now: DateTime<Utc>) -> Route {
    let route = match session_path(&request.path) {
        Ok(Some(route)) => route,
        Ok(None) => return Route::NotFound,
        Err(()) => return Route::InvalidSessionKey,
    };
    match (request.method.as_str(), route.action) {
        ("GET", None) => Route::Session { key: route.key },
        ("GET", Some(SessionAction::Turns)) => Route::SessionTurns {
            key: route.key,
            query: turn_query(request, now),
        },
        ("POST", Some(SessionAction::Ack)) => Route::Ack { key: route.key },
        ("POST", Some(SessionAction::Stop)) => Route::Stop { key: route.key },
        (_, Some(SessionAction::Ack | SessionAction::Stop | SessionAction::Turns)) => {
            Route::MethodNotAllowed
        }
        _ => Route::NotFound,
    }
}

struct SessionPath {
    key: String,
    action: Option<SessionAction>,
}

enum SessionAction {
    Ack,
    Stop,
    Turns,
    Unknown,
}

fn session_path(path: &str) -> Result<Option<SessionPath>, ()> {
    let Some(rest) = path.strip_prefix("/v1/sessions/") else {
        return Ok(None);
    };
    let rest = rest.trim_matches('/');
    if rest.is_empty() {
        return Ok(None);
    }
    let parts = rest.split('/').collect::<Vec<_>>();
    if parts.len() > 2 {
        return Ok(None);
    }
    let key = percent_decode(parts[0]).ok_or(())?;
    let action = parts.get(1).map(|part| match *part {
        "ack" => SessionAction::Ack,
        "stop" => SessionAction::Stop,
        "turns" => SessionAction::Turns,
        _ => SessionAction::Unknown,
    });
    if matches!(action, Some(SessionAction::Unknown)) {
        return Ok(None);
    }
    Ok(Some(SessionPath { key, action }))
}

fn percent_decode(raw: &str) -> Option<String> {
    let mut out = Vec::new();
    let mut bytes = raw.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes.next()?;
            let low = bytes.next()?;
            let value = (hex_value(high)? << 4) | hex_value(low)?;
            out.push(value);
        } else {
            out.push(byte);
        }
    }
    String::from_utf8(out).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn turn_query(request: &Request, now: DateTime<Utc>) -> TurnQuery {
    TurnQuery {
        since: query_param(&request.query, "since").and_then(|value| since_param(&value, now)),
        limit: limit_query(request, 200),
    }
}

fn limit_query(request: &Request, default: usize) -> usize {
    query_param(&request.query, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .map(|value| value.min(1000))
        .unwrap_or(default)
}

fn query_param(query: &str, name: &str) -> Option<String> {
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

fn since_param(raw: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if let Ok(duration) = parse_duration_for_cli(raw) {
        return chrono::Duration::from_std(duration)
            .ok()
            .map(|duration| now - duration);
    }
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}
