use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;
use url::Url;

use crate::config::parse_duration_for_cli;
use crate::onboarding::{NotificationView, OnboardingView};
use crate::platform::Platform;
use crate::runtime::{Runtime, RuntimeError, TurnQuery};
use crate::service::{
    AckRequest, AckView, AlertView, ConfigUpdate, ConfigView, EventView, ServiceError, SessionView,
    Snapshot, StopRequest, StopView, TurnView,
};

pub const TOKEN_COOKIE: &str = "curb_token";

pub fn load_or_create_token(state_dir: impl AsRef<Path>) -> Result<(String, PathBuf), ApiError> {
    let state_dir = state_dir.as_ref();
    let path = state_dir.join("api.token");
    match fs::read_to_string(&path) {
        Ok(content) => {
            set_file_private(&path)?;
            let token = content.trim().to_string();
            if token.is_empty() {
                return Err(ApiError::Config("api token file is empty".to_string()));
            }
            Ok((token, path))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(state_dir)
                .map_err(|source| ApiError::Internal(format!("create state dir: {source}")))?;
            set_dir_private(state_dir)?;
            let mut raw = [0u8; 32];
            getrandom::fill(&mut raw)
                .map_err(|source| ApiError::Internal(format!("generate api token: {source}")))?;
            let token = hex::encode(raw);
            write_new_private_file(&path, format!("{token}\n").as_bytes())?;
            Ok((token, path))
        }
        Err(error) => Err(ApiError::Internal(format!("read api token: {error}"))),
    }
}

pub trait Backend {
    fn snapshot(&self, now: DateTime<Utc>) -> Result<Snapshot, ApiError>;
    fn rescan(&self, now: DateTime<Utc>) -> Result<Snapshot, ApiError>;
    fn session(&self, key: &str, now: DateTime<Utc>) -> Result<SessionView, ApiError>;
    fn turns(
        &self,
        key: &str,
        query: TurnQuery,
        now: DateTime<Utc>,
    ) -> Result<Vec<TurnView>, ApiError>;
    fn events(&self, limit: usize) -> Result<Vec<EventView>, ApiError>;
    fn alerts(&self, limit: usize, now: DateTime<Utc>) -> Result<Vec<AlertView>, ApiError>;
    fn acknowledge_session(
        &self,
        key: &str,
        request: AckRequest,
        now: DateTime<Utc>,
    ) -> Result<AckView, ApiError>;
    fn stop_session(
        &self,
        key: &str,
        request: StopRequest,
        now: DateTime<Utc>,
    ) -> Result<StopView, ApiError>;
    fn config(&self) -> Result<ConfigView, ApiError>;
    fn update_config(&self, update: ConfigUpdate) -> Result<ConfigView, ApiError>;
    fn onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, ApiError>;
    fn complete_onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, ApiError>;
    fn notification_health(&self) -> Result<NotificationView, ApiError>;
    fn test_notification(&self, now: DateTime<Utc>) -> Result<NotificationView, ApiError>;
}

impl<P: Platform> Backend for Runtime<P> {
    fn snapshot(&self, now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
        self.snapshot(now).map_err(ApiError::from)
    }

    fn rescan(&self, now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
        self.rescan(now).map_err(ApiError::from)
    }

    fn session(&self, key: &str, now: DateTime<Utc>) -> Result<SessionView, ApiError> {
        self.session(key, now).map_err(ApiError::from)
    }

    fn turns(
        &self,
        key: &str,
        query: TurnQuery,
        now: DateTime<Utc>,
    ) -> Result<Vec<TurnView>, ApiError> {
        self.turns(key, query, now).map_err(ApiError::from)
    }

    fn events(&self, limit: usize) -> Result<Vec<EventView>, ApiError> {
        self.events(limit).map_err(ApiError::from)
    }

    fn alerts(&self, limit: usize, now: DateTime<Utc>) -> Result<Vec<AlertView>, ApiError> {
        self.alerts(limit, now).map_err(ApiError::from)
    }

    fn acknowledge_session(
        &self,
        key: &str,
        request: AckRequest,
        now: DateTime<Utc>,
    ) -> Result<AckView, ApiError> {
        self.acknowledge_session(key, request, now)
            .map_err(ApiError::from)
    }

    fn stop_session(
        &self,
        key: &str,
        request: StopRequest,
        now: DateTime<Utc>,
    ) -> Result<StopView, ApiError> {
        self.stop_session(key, request, now).map_err(ApiError::from)
    }

    fn config(&self) -> Result<ConfigView, ApiError> {
        self.config_view().map_err(ApiError::from)
    }

    fn update_config(&self, update: ConfigUpdate) -> Result<ConfigView, ApiError> {
        self.update_config(update).map_err(ApiError::from)
    }

    fn onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, ApiError> {
        self.onboarding(now).map_err(ApiError::from)
    }

    fn complete_onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, ApiError> {
        self.complete_onboarding(now).map_err(ApiError::from)
    }

    fn notification_health(&self) -> Result<NotificationView, ApiError> {
        self.notification_health().map_err(ApiError::from)
    }

    fn test_notification(&self, now: DateTime<Utc>) -> Result<NotificationView, ApiError> {
        self.test_notification(now).map_err(ApiError::from)
    }
}

impl<B: Backend> Backend for Arc<B> {
    fn snapshot(&self, now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
        (**self).snapshot(now)
    }

    fn rescan(&self, now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
        (**self).rescan(now)
    }

    fn session(&self, key: &str, now: DateTime<Utc>) -> Result<SessionView, ApiError> {
        (**self).session(key, now)
    }

    fn turns(
        &self,
        key: &str,
        query: TurnQuery,
        now: DateTime<Utc>,
    ) -> Result<Vec<TurnView>, ApiError> {
        (**self).turns(key, query, now)
    }

    fn events(&self, limit: usize) -> Result<Vec<EventView>, ApiError> {
        (**self).events(limit)
    }

    fn alerts(&self, limit: usize, now: DateTime<Utc>) -> Result<Vec<AlertView>, ApiError> {
        (**self).alerts(limit, now)
    }

    fn acknowledge_session(
        &self,
        key: &str,
        request: AckRequest,
        now: DateTime<Utc>,
    ) -> Result<AckView, ApiError> {
        (**self).acknowledge_session(key, request, now)
    }

    fn stop_session(
        &self,
        key: &str,
        request: StopRequest,
        now: DateTime<Utc>,
    ) -> Result<StopView, ApiError> {
        (**self).stop_session(key, request, now)
    }

    fn config(&self) -> Result<ConfigView, ApiError> {
        (**self).config()
    }

    fn update_config(&self, update: ConfigUpdate) -> Result<ConfigView, ApiError> {
        (**self).update_config(update)
    }

    fn onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, ApiError> {
        (**self).onboarding(now)
    }

    fn complete_onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, ApiError> {
        (**self).complete_onboarding(now)
    }

    fn notification_health(&self) -> Result<NotificationView, ApiError> {
        (**self).notification_health()
    }

    fn test_notification(&self, now: DateTime<Utc>) -> Result<NotificationView, ApiError> {
        (**self).test_notification(now)
    }
}

pub struct Server<B: Backend> {
    token: String,
    backend: B,
    ui: bool,
}

impl<B: Backend> Server<B> {
    pub fn new(token: impl Into<String>, backend: B) -> Result<Self, ApiError> {
        let token = token.into();
        if token.trim().is_empty() {
            return Err(ApiError::Config("api token is required".to_string()));
        }
        Ok(Self {
            token,
            backend,
            ui: false,
        })
    }

    pub fn serve_ui(&mut self) {
        self.ui = true;
    }

    pub fn handle(&self, request: Request, now: DateTime<Utc>) -> Response {
        if !request.path.starts_with("/v1/") {
            if self.ui
                && let Some(mut response) = crate::web::handle(&request)
            {
                response.headers.insert(
                    "set-cookie",
                    token_cookie(&self.token, request.scheme == "https"),
                );
                return response;
            }
            return Response::empty(404);
        }
        let mut cors_headers = cors_headers(&request);
        if request.method == "OPTIONS" {
            return Response::empty(204).with_headers(cors_headers);
        }
        if !self.authorized(&request) {
            return error_response(401, "unauthorized").with_headers(cors_headers);
        }
        if self.uses_cookie_auth(&request)
            && unsafe_method(&request.method)
            && !same_origin(&request)
        {
            return error_response(403, "forbidden").with_headers(cors_headers);
        }
        let mut response = self.route(request, now);
        response.headers.append(&mut cors_headers);
        response
    }

    fn route(&self, request: Request, now: DateTime<Utc>) -> Response {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/v1/health") => json_response(
                200,
                json!({
                    "ok": true,
                    "app": "curb",
                    "api_version": 1,
                }),
            ),
            ("GET", "/v1/snapshot") => self
                .backend
                .snapshot(now)
                .map(json_ok)
                .unwrap_or_else(api_error_response),
            ("GET", "/v1/overview") => self
                .backend
                .snapshot(now)
                .map(|snapshot| json_ok(snapshot.overview))
                .unwrap_or_else(api_error_response),
            ("GET", "/v1/agents") => self
                .backend
                .snapshot(now)
                .map(|snapshot| json_ok(snapshot.agents))
                .unwrap_or_else(api_error_response),
            ("GET", "/v1/sessions") => self
                .backend
                .snapshot(now)
                .map(|snapshot| json_ok(snapshot.sessions))
                .unwrap_or_else(api_error_response),
            ("POST", "/v1/service/rescan") => self
                .backend
                .rescan(now)
                .map(json_ok)
                .unwrap_or_else(api_error_response),
            (_, "/v1/service/rescan") => error_response(405, "method not allowed"),
            _ if request.path.starts_with("/v1/sessions/") => self.handle_session(request, now),
            ("GET", "/v1/events") => self
                .backend
                .events(limit_query(&request, 200))
                .map(json_ok)
                .unwrap_or_else(api_error_response),
            (_, "/v1/events") => error_response(405, "method not allowed"),
            ("GET", "/v1/alerts") => self
                .backend
                .alerts(limit_query(&request, 50), now)
                .map(json_ok)
                .unwrap_or_else(api_error_response),
            (_, "/v1/alerts") => error_response(405, "method not allowed"),
            ("GET", "/v1/notifications/health") => self
                .backend
                .notification_health()
                .map(json_ok)
                .unwrap_or_else(api_error_response),
            (_, "/v1/notifications/health") => error_response(405, "method not allowed"),
            ("POST", "/v1/notifications/test") => self
                .backend
                .test_notification(now)
                .map(json_ok)
                .unwrap_or_else(api_error_response),
            (_, "/v1/notifications/test") => error_response(405, "method not allowed"),
            ("GET", "/v1/config") => self
                .backend
                .config()
                .map(json_ok)
                .unwrap_or_else(api_error_response),
            ("PUT", "/v1/config") => decode_config_update(&request)
                .and_then(|update| self.backend.update_config(update))
                .map(json_ok)
                .unwrap_or_else(api_error_response),
            (_, "/v1/config") => error_response(405, "method not allowed"),
            ("GET", "/v1/onboarding") => self
                .backend
                .onboarding(now)
                .map(json_ok)
                .unwrap_or_else(api_error_response),
            (_, "/v1/onboarding") => error_response(405, "method not allowed"),
            ("POST", "/v1/onboarding/complete") => self
                .backend
                .complete_onboarding(now)
                .map(json_ok)
                .unwrap_or_else(api_error_response),
            (_, "/v1/onboarding/complete") => error_response(405, "method not allowed"),
            ("GET", _) => error_response(404, "not found"),
            _ => error_response(405, "method not allowed"),
        }
    }

    fn handle_session(&self, request: Request, now: DateTime<Utc>) -> Response {
        let (key, action) = match session_route(&request.path) {
            Ok(Some(route)) => route,
            Ok(None) => return error_response(404, "not found"),
            Err(()) => return error_response(400, "invalid session key"),
        };
        match (request.method.as_str(), action.as_deref()) {
            ("GET", None) => self
                .backend
                .session(&key, now)
                .map(json_ok)
                .unwrap_or_else(api_error_response),
            ("GET", Some("turns")) => self
                .backend
                .turns(&key, turn_query(&request, now), now)
                .map(json_ok)
                .unwrap_or_else(api_error_response),
            ("POST", Some("ack")) => decode_ack(&request)
                .and_then(|ack| self.backend.acknowledge_session(&key, ack, now))
                .map(json_ok)
                .unwrap_or_else(api_error_response),
            ("POST", Some("stop")) => decode_stop(&request)
                .and_then(|stop| self.backend.stop_session(&key, stop, now))
                .map(json_ok)
                .unwrap_or_else(api_error_response),
            (_, Some("ack" | "stop")) => error_response(405, "method not allowed"),
            (_, Some("turns")) => error_response(405, "method not allowed"),
            _ => error_response(404, "not found"),
        }
    }

    fn authorized(&self, request: &Request) -> bool {
        constant_time_eq(
            bearer_token(request.header_value("authorization")),
            &self.token,
        ) || constant_time_eq(
            request.header_value("x-curb-token").unwrap_or_default(),
            &self.token,
        ) || request
            .cookie_value(TOKEN_COOKIE)
            .is_some_and(|token| constant_time_eq(&token, &self.token))
    }

    fn uses_cookie_auth(&self, request: &Request) -> bool {
        !constant_time_eq(
            bearer_token(request.header_value("authorization")),
            &self.token,
        ) && !constant_time_eq(
            request.header_value("x-curb-token").unwrap_or_default(),
            &self.token,
        ) && request
            .cookie_value(TOKEN_COOKIE)
            .is_some_and(|token| constant_time_eq(&token, &self.token))
    }
}

fn token_cookie(token: &str, secure: bool) -> String {
    let mut cookie = format!("{TOKEN_COOKIE}={token}; Path=/v1/; HttpOnly; SameSite=Strict");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub scheme: String,
    pub host: String,
}

impl Request {
    pub fn new(method: impl Into<String>, target: impl Into<String>) -> Self {
        let target = target.into();
        let (path, query) = split_target(&target);
        Self {
            method: method.into().to_ascii_uppercase(),
            path,
            query,
            headers: HeaderMap::default(),
            body: Vec::new(),
            scheme: "http".to_string(),
            host: "127.0.0.1:8765".to_string(),
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn origin(mut self, origin: impl Into<String>) -> Self {
        self.headers.insert("origin", origin);
        self
    }

    pub fn cookie(mut self, cookie: impl Into<String>) -> Self {
        self.headers.insert("cookie", cookie);
        self
    }

    pub fn endpoint(mut self, scheme: impl Into<String>, host: impl Into<String>) -> Self {
        self.scheme = scheme.into();
        self.host = host.into();
        self
    }

    fn header_value(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }

    fn cookie_value(&self, name: &str) -> Option<String> {
        self.header_value("cookie").and_then(|raw| {
            raw.split(';').find_map(|part| {
                let (key, value) = part.trim().split_once('=')?;
                (key == name).then(|| value.to_string())
            })
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Response {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Response {
    fn empty(status: u16) -> Self {
        Self {
            status,
            headers: HeaderMap::default(),
            body: Vec::new(),
        }
    }

    fn with_headers(mut self, mut headers: HeaderMap) -> Self {
        self.headers.append(&mut headers);
        self
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    #[cfg(test)]
    pub(crate) fn empty_for_test(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: HeaderMap::default(),
            body,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HeaderMap(BTreeMap<String, String>);

impl HeaderMap {
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.0
            .insert(name.into().to_ascii_lowercase(), value.into());
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(&name.to_ascii_lowercase()).map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    fn append(&mut self, other: &mut HeaderMap) {
        self.0.append(&mut other.0);
    }
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("api config: {0}")]
    Config(String),
    #[error("session not found")]
    SessionNotFound,
    #[error("invalid acknowledgement: {0}")]
    InvalidAck(String),
    #[error("invalid stop request: {0}")]
    InvalidStop(String),
    #[error("invalid config update: {0}")]
    InvalidConfig(String),
    #[error("session cannot be stopped safely: {0}")]
    StopConflict(String),
    #[error("local notifications are disabled")]
    NotificationsDisabled(NotificationView),
    #[error("local notifications are unavailable")]
    NotificationsUnavailable(NotificationView),
    #[error("{0}")]
    Internal(String),
}

impl From<RuntimeError> for ApiError {
    fn from(error: RuntimeError) -> Self {
        match error {
            RuntimeError::Service(ServiceError::SessionNotFound) => Self::SessionNotFound,
            RuntimeError::Service(ServiceError::InvalidAck(message)) => Self::InvalidAck(message),
            RuntimeError::Service(ServiceError::InvalidStop(message)) => Self::InvalidStop(message),
            RuntimeError::Service(ServiceError::InvalidConfig(message)) => {
                Self::InvalidConfig(message)
            }
            RuntimeError::Service(ServiceError::StopConflict(message)) => {
                Self::StopConflict(message)
            }
            RuntimeError::NotificationsDisabled(view) => Self::NotificationsDisabled(view),
            RuntimeError::NotificationsUnavailable(view) => Self::NotificationsUnavailable(view),
            other => Self::Internal(other.to_string()),
        }
    }
}

fn decode_ack(request: &Request) -> Result<AckRequest, ApiError> {
    serde_json::from_slice(&request.body).map_err(|error| ApiError::InvalidAck(error.to_string()))
}

fn decode_stop(request: &Request) -> Result<StopRequest, ApiError> {
    serde_json::from_slice(&request.body).map_err(|error| ApiError::InvalidStop(error.to_string()))
}

fn decode_config_update(request: &Request) -> Result<ConfigUpdate, ApiError> {
    serde_json::from_slice(&request.body)
        .map_err(|error| ApiError::InvalidConfig(error.to_string()))
}

fn json_ok(value: impl Serialize) -> Response {
    json_response(
        200,
        serde_json::to_value(value).expect("serialize api response"),
    )
}

fn json_response(status: u16, value: Value) -> Response {
    let mut headers = HeaderMap::default();
    headers.insert("content-type", "application/json");
    Response {
        status,
        headers,
        body: serde_json::to_vec(&value).expect("serialize json"),
    }
}

fn error_response(status: u16, message: &str) -> Response {
    json_response(status, json!({ "error": message }))
}

fn api_error_response(error: ApiError) -> Response {
    match error {
        ApiError::SessionNotFound => error_response(404, "session not found"),
        ApiError::InvalidAck(message) => error_response(400, &message),
        ApiError::InvalidStop(message) => error_response(400, &message),
        ApiError::InvalidConfig(message) => error_response(400, &message),
        ApiError::StopConflict(message) => error_response(409, &message),
        ApiError::NotificationsDisabled(view) => {
            json_response(409, serde_json::to_value(view).unwrap())
        }
        ApiError::NotificationsUnavailable(view) => {
            json_response(503, serde_json::to_value(view).unwrap())
        }
        ApiError::Config(message) => error_response(500, &message),
        ApiError::Internal(message) => error_response(500, &message),
    }
}

fn cors_headers(request: &Request) -> HeaderMap {
    let mut headers = HeaderMap::default();
    let Some(origin) = request.header_value("origin") else {
        return headers;
    };
    if !local_origin(origin) {
        return headers;
    }
    headers.insert("access-control-allow-origin", origin);
    headers.insert("vary", "Origin");
    headers.insert(
        "access-control-allow-headers",
        "Authorization, Content-Type, X-Curb-Token",
    );
    headers.insert("access-control-allow-methods", "GET, POST, PUT, OPTIONS");
    headers
}

fn local_origin(origin: &str) -> bool {
    let Ok(parsed) = Url::parse(origin) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https" | "tauri") {
        return false;
    }
    parsed.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    })
}

fn same_origin(request: &Request) -> bool {
    let Some(origin) = request.header_value("origin") else {
        return false;
    };
    let Ok(parsed) = Url::parse(origin) else {
        return false;
    };
    parsed.scheme().eq_ignore_ascii_case(&request.scheme)
        && parsed
            .host_str()
            .zip(parsed.port_or_known_default())
            .map(|(host, port)| format!("{host}:{port}"))
            .is_some_and(|origin_host| origin_host.eq_ignore_ascii_case(&request.host))
}

fn unsafe_method(method: &str) -> bool {
    !matches!(method, "GET" | "HEAD" | "OPTIONS")
}

fn bearer_token(value: Option<&str>) -> &str {
    value
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or_default()
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let max = left.len().max(right.len());
    let mut diff = left.len() ^ right.len();
    for index in 0..max {
        let left_byte = left.get(index).copied().unwrap_or_default();
        let right_byte = right.get(index).copied().unwrap_or_default();
        diff |= usize::from(left_byte ^ right_byte);
    }
    diff == 0
}

fn split_target(target: &str) -> (String, String) {
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

fn session_route(path: &str) -> Result<Option<(String, Option<String>)>, ()> {
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
    let action = parts.get(1).map(|part| (*part).to_string());
    Ok(Some((key, action)))
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

fn set_dir_private(path: &Path) -> Result<(), ApiError> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|source| ApiError::Internal(format!("chmod state dir: {source}")))?;
    }
    Ok(())
}

fn set_file_private(path: &Path) -> Result<(), ApiError> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|source| ApiError::Internal(format!("chmod api token: {source}")))?;
    }
    Ok(())
}

fn write_new_private_file(path: &Path, content: &[u8]) -> Result<(), ApiError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|source| ApiError::Internal(format!("create api token: {source}")))?;
    file.write_all(content)
        .map_err(|source| ApiError::Internal(format!("write api token: {source}")))?;
    set_file_private(path)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use chrono::TimeZone;

    use super::*;
    use crate::onboarding::{CapabilityView, PlatformCapabilities};
    use crate::service::{AgentView, Overview};

    #[test]
    fn requires_auth_for_api_routes_and_allows_local_preflight() {
        let server = Server::new("test-token", FakeBackend::default()).unwrap();
        let now = fixed_now();

        let unauthorized = server.handle(Request::new("GET", "/v1/overview"), now);
        assert_eq!(unauthorized.status, 401);

        let preflight = server.handle(
            Request::new("OPTIONS", "/v1/overview").origin("http://127.0.0.1:5173"),
            now,
        );
        assert_eq!(preflight.status, 204);
        assert_eq!(
            preflight.headers.get("access-control-allow-origin"),
            Some("http://127.0.0.1:5173")
        );
    }

    #[test]
    fn non_api_routes_serve_embedded_ui_only_when_enabled() {
        let mut server = Server::new("test-token", FakeBackend::default()).unwrap();
        let disabled = server.handle(Request::new("GET", "/"), fixed_now());
        assert_eq!(disabled.status, 404);

        server.serve_ui();
        let index = server.handle(Request::new("GET", "/"), fixed_now());
        assert_eq!(index.status, 200);
        assert_eq!(
            index.headers.get("content-type"),
            Some("text/html; charset=utf-8")
        );
        assert!(index.text().contains("<div id=\"root\"></div>"));
        assert_eq!(
            index.headers.get("set-cookie"),
            Some("curb_token=test-token; Path=/v1/; HttpOnly; SameSite=Strict")
        );

        let secure = server.handle(
            Request::new("GET", "/").endpoint("https", "127.0.0.1:8765"),
            fixed_now(),
        );
        assert_eq!(
            secure.headers.get("set-cookie"),
            Some("curb_token=test-token; Path=/v1/; HttpOnly; SameSite=Strict; Secure")
        );

        let spa = server.handle(Request::new("GET", "/sessions/codex:s1"), fixed_now());
        assert_eq!(spa.status, 200);
        assert!(spa.text().contains("<div id=\"root\"></div>"));

        let blocked_method = server.handle(Request::new("POST", "/"), fixed_now());
        assert_eq!(blocked_method.status, 404);
    }

    #[test]
    fn api_routes_remain_protected_when_ui_is_enabled() {
        let mut server = Server::new("test-token", FakeBackend::default()).unwrap();
        server.serve_ui();

        let health = server.handle(Request::new("GET", "/v1/health"), fixed_now());
        assert_eq!(health.status, 401);

        let authed = server.handle(authed("GET", "/v1/health"), fixed_now());
        assert_eq!(authed.status, 200);
    }

    #[test]
    fn supports_bearer_header_token_and_cookie_auth() {
        let server = Server::new("test-token", FakeBackend::default()).unwrap();
        let now = fixed_now();

        assert_eq!(
            server
                .handle(
                    Request::new("GET", "/v1/health").header("Authorization", "Bearer test-token"),
                    now,
                )
                .status,
            200
        );
        assert_eq!(
            server
                .handle(
                    Request::new("GET", "/v1/health").header("X-Curb-Token", "test-token"),
                    now,
                )
                .status,
            200
        );
        assert_eq!(
            server
                .handle(
                    Request::new("GET", "/v1/health").cookie("curb_token=test-token"),
                    now,
                )
                .status,
            200
        );
    }

    #[test]
    fn cookie_auth_requires_same_origin_for_unsafe_methods() {
        let server = Server::new("test-token", FakeBackend::default()).unwrap();
        let now = fixed_now();

        let missing_origin = server.handle(
            Request::new("POST", "/v1/service/rescan").cookie("curb_token=test-token"),
            now,
        );
        assert_eq!(missing_origin.status, 403);

        let cross_origin = server.handle(
            Request::new("POST", "/v1/service/rescan")
                .cookie("curb_token=test-token")
                .origin("http://evil.example"),
            now,
        );
        assert_eq!(cross_origin.status, 403);

        let same_origin = server.handle(
            Request::new("POST", "/v1/service/rescan")
                .cookie("curb_token=test-token")
                .origin("http://127.0.0.1:8765")
                .endpoint("http", "127.0.0.1:8765"),
            now,
        );
        assert_eq!(same_origin.status, 200);
    }

    #[test]
    fn returns_snapshot_slices() {
        let server = Server::new("test-token", FakeBackend::default()).unwrap();
        let now = fixed_now();

        let overview = server.handle(authed("GET", "/v1/overview"), now);
        assert_eq!(overview.status, 200);
        assert!(overview.text().contains("\"status\":\"WATCH\""));
        assert!(overview.text().contains("\"changes\""));
        assert!(overview.text().contains("\"capabilities\""));
        assert!(overview.text().contains("\"mode\":\"watch\""));

        let agents = server.handle(authed("GET", "/v1/agents"), now);
        assert_eq!(agents.status, 200);
        assert!(agents.text().contains("codex-worker"));
        assert!(agents.text().contains("\"project\":\"repo\""));
        assert!(agents.text().contains("\"running_for_seconds\":60"));

        let sessions = server.handle(authed("GET", "/v1/sessions"), now);
        assert_eq!(sessions.status, 200);
        assert!(sessions.text().contains("codex:session/one"));
        assert!(sessions.text().contains("\"alert\":\"warn\""));
        assert!(sessions.text().contains("\"project\":\"repo\""));
    }

    #[test]
    fn server_accepts_shared_backend_for_daemon_side_loops() {
        let backend = Arc::new(SharedBackend);
        let server = Server::new("test-token", Arc::clone(&backend)).unwrap();

        let response = server.handle(authed("GET", "/v1/overview"), fixed_now());

        assert_eq!(response.status, 200);
        assert!(response.text().contains("\"status\":\"WATCH\""));
    }

    #[test]
    fn returns_events_and_alerts_with_limit_and_method_semantics() {
        let server = Server::new("test-token", FakeBackend::default()).unwrap();
        let now = fixed_now();

        let events = server.handle(authed("GET", "/v1/events?limit=1"), now);
        assert_eq!(events.status, 200);
        assert!(events.text().contains("\"category\":\"alert\""));
        assert!(events.text().contains("\"kind\":\"warning\""));
        assert!(!events.text().contains("\"kind\":\"completed\""));

        let alerts = server.handle(authed("GET", "/v1/alerts?limit=1"), now);
        assert_eq!(alerts.status, 200);
        assert!(alerts.text().contains("\"category\":\"warning\""));
        assert!(alerts.text().contains("\"can_acknowledge\":true"));
        assert!(
            alerts
                .text()
                .contains("\"session_key\":\"codex:session/one\"")
        );

        assert_eq!(server.handle(authed("POST", "/v1/events"), now).status, 405);
        assert_eq!(server.handle(authed("POST", "/v1/alerts"), now).status, 405);
    }

    #[test]
    fn decodes_session_key_and_filters_turns() {
        let backend = FakeBackend::default();
        let server = Server::new("test-token", backend).unwrap();
        let now = fixed_now();

        let session = server.handle(authed("GET", "/v1/sessions/codex:session%2Fone"), now);
        assert_eq!(session.status, 200);
        assert!(session.text().contains("\"id\":\"session/one\""));

        let turns = server.handle(
            authed(
                "GET",
                "/v1/sessions/codex:session%2Fone/turns?limit=1&since=24h",
            ),
            now,
        );
        assert_eq!(turns.status, 200);
        assert!(turns.text().contains("\"total_tokens\":789"));
    }

    #[test]
    fn rescan_requires_post_and_auth() {
        let server = Server::new("test-token", FakeBackend::default()).unwrap();
        let now = fixed_now();

        assert_eq!(
            server
                .handle(authed("GET", "/v1/service/rescan"), now)
                .status,
            405
        );
        assert_eq!(
            server
                .handle(Request::new("POST", "/v1/service/rescan"), now)
                .status,
            401
        );
        assert_eq!(
            server
                .handle(authed("POST", "/v1/service/rescan"), now)
                .status,
            200
        );
    }

    #[test]
    fn maps_ack_and_stop_routes_and_errors() {
        let backend = FakeBackend::default();
        backend.next_error.replace(Some(ApiError::SessionNotFound));
        let server = Server::new("test-token", backend).unwrap();
        let now = fixed_now();
        let missing = server.handle(
            authed("POST", "/v1/sessions/missing/ack").body(r#"{"extend_seconds":60}"#),
            now,
        );
        assert_eq!(missing.status, 404);

        server
            .backend
            .next_error
            .replace(Some(ApiError::StopConflict("busy".to_string())));
        let conflict = server.handle(
            authed("POST", "/v1/sessions/codex:session%2Fone/stop").body(stop_body()),
            now,
        );
        assert_eq!(conflict.status, 409);

        let ok = server.handle(
            authed("POST", "/v1/sessions/codex:session%2Fone/ack")
                .body(r#"{"extend_seconds":60,"reason":"still supervising"}"#),
            now,
        );
        assert_eq!(ok.status, 200);
        assert!(ok.text().contains("still supervising"));

        let stopped = server.handle(
            authed("POST", "/v1/sessions/codex:session%2Fone/stop").body(stop_body()),
            now,
        );
        assert_eq!(stopped.status, 200);
        assert!(
            stopped
                .text()
                .contains("\"result\":{\"soft_signaled\":[4242]}")
        );
    }

    #[test]
    fn serves_notification_health_and_test_with_conflict_shapes() {
        let server = Server::new("test-token", FakeBackend::default()).unwrap();
        let now = fixed_now();

        let health = server.handle(authed("GET", "/v1/notifications/health"), now);
        assert_eq!(health.status, 200);
        assert!(health.text().contains("\"status\":\"ready\""));

        let tested = server.handle(authed("POST", "/v1/notifications/test"), now);
        assert_eq!(tested.status, 200);
        assert!(tested.text().contains("\"status\":\"delivered\""));
        assert!(
            tested
                .text()
                .contains("\"last_test_at\":\"2026-05-28T16:00:00Z\"")
        );

        assert_eq!(
            server
                .handle(authed("POST", "/v1/notifications/health"), now)
                .status,
            405
        );
        assert_eq!(
            server
                .handle(authed("GET", "/v1/notifications/test"), now)
                .status,
            405
        );

        server
            .backend
            .next_error
            .replace(Some(ApiError::NotificationsDisabled(notification_view(
                false, false, "disabled",
            ))));
        let disabled = server.handle(authed("POST", "/v1/notifications/test"), now);
        assert_eq!(disabled.status, 409);
        assert!(disabled.text().contains("\"enabled\":false"));

        server
            .backend
            .next_error
            .replace(Some(ApiError::NotificationsUnavailable(notification_view(
                true,
                false,
                "unavailable",
            ))));
        let unavailable = server.handle(authed("POST", "/v1/notifications/test"), now);
        assert_eq!(unavailable.status, 503);
        assert!(unavailable.text().contains("\"available\":false"));

        let cross_origin_cookie = server.handle(
            Request::new("POST", "/v1/notifications/test")
                .cookie("curb_token=test-token")
                .origin("http://evil.example"),
            now,
        );
        assert_eq!(cross_origin_cookie.status, 403);
    }

    #[test]
    fn serves_and_updates_config() {
        let server = Server::new("test-token", FakeBackend::default()).unwrap();
        let now = fixed_now();

        let view = server.handle(authed("GET", "/v1/config"), now);
        assert_eq!(view.status, 200);
        assert!(view.text().contains("\"mode\":\"alert\""));
        assert!(view.text().contains("\"warn_turn_tokens\":1000"));

        let updated = server.handle(
            authed("PUT", "/v1/config").body(
                r#"{"mode":"visibility","warn_turn_tokens":2000,"kill_turn_tokens":4000,"usage_window_seconds":120,"local_notifications":false}"#,
            ),
            now,
        );
        assert_eq!(updated.status, 200);
        assert!(updated.text().contains("\"mode\":\"visibility\""));
        assert!(updated.text().contains("\"warn_turn_tokens\":2000"));
        assert!(updated.text().contains("\"local_notifications\":false"));

        let bad = server.handle(authed("PUT", "/v1/config").body("{"), now);
        assert_eq!(bad.status, 400);

        assert_eq!(server.handle(authed("POST", "/v1/config"), now).status, 405);
    }

    #[test]
    fn serves_and_completes_onboarding() {
        let server = Server::new("test-token", FakeBackend::default()).unwrap();
        let now = fixed_now();

        let initial = server.handle(authed("GET", "/v1/onboarding"), now);
        assert_eq!(initial.status, 200);
        assert!(initial.text().contains("\"required\":true"));
        assert!(initial.text().contains("\"mode\":\"alert\""));
        assert!(initial.text().contains("\"process_capture\""));

        let completed = server.handle(authed("POST", "/v1/onboarding/complete"), now);
        assert_eq!(completed.status, 200);
        assert!(completed.text().contains("\"required\":false"));

        assert_eq!(
            server.handle(authed("POST", "/v1/onboarding"), now).status,
            405
        );
        assert_eq!(
            server
                .handle(authed("GET", "/v1/onboarding/complete"), now)
                .status,
            405
        );

        let cross_origin_cookie = server.handle(
            Request::new("POST", "/v1/onboarding/complete")
                .cookie("curb_token=test-token")
                .origin("http://evil.example"),
            now,
        );
        assert_eq!(cross_origin_cookie.status, 403);
    }

    #[test]
    fn invalid_encoded_session_key_returns_bad_request_shape() {
        let server = Server::new("test-token", FakeBackend::default()).unwrap();
        let now = fixed_now();

        let response = server.handle(authed("GET", "/v1/sessions/bad%XX"), now);

        assert_eq!(response.status, 400);
        assert!(response.text().contains("invalid session key"));
    }

    #[test]
    fn malformed_ack_and_stop_payloads_are_bad_requests() {
        let server = Server::new("test-token", FakeBackend::default()).unwrap();
        let now = fixed_now();

        let ack = server.handle(
            authed("POST", "/v1/sessions/codex:session%2Fone/ack").body("{"),
            now,
        );
        assert_eq!(ack.status, 400);

        let stop = server.handle(
            authed("POST", "/v1/sessions/codex:session%2Fone/stop").body("{"),
            now,
        );
        assert_eq!(stop.status, 400);
    }

    #[test]
    fn load_or_create_token_persists_and_reuses_private_token() {
        let dir = tempfile::tempdir().unwrap();

        let (token, path) = load_or_create_token(dir.path()).unwrap();
        let (again, same_path) = load_or_create_token(dir.path()).unwrap();

        assert_eq!(token.len(), 64);
        assert_eq!(again, token);
        assert_eq!(same_path, path);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn load_or_create_token_rejects_empty_and_repairs_existing_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api.token");
        fs::write(&path, "\n").unwrap();
        assert!(matches!(
            load_or_create_token(dir.path()),
            Err(ApiError::Config(message)) if message.contains("empty")
        ));

        fs::write(&path, "existing-token\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let (token, _) = load_or_create_token(dir.path()).unwrap();
        assert_eq!(token, "existing-token");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    fn authed(method: &str, target: &str) -> Request {
        Request::new(method, target).header("Authorization", "Bearer test-token")
    }

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap()
    }

    fn stop_body() -> &'static str {
        r#"{"confirm":true,"scope":"tree","expected":{"pid":4242,"started_at":"2026-05-28T15:59:00Z","owner":"phaedrus","executable":"/usr/local/bin/codex"}}"#
    }

    #[derive(Default)]
    struct FakeBackend {
        next_error: RefCell<Option<ApiError>>,
    }

    struct SharedBackend;

    impl Backend for SharedBackend {
        fn snapshot(&self, _now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
            Ok(snapshot())
        }

        fn rescan(&self, _now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
            Ok(snapshot())
        }

        fn session(&self, _key: &str, _now: DateTime<Utc>) -> Result<SessionView, ApiError> {
            Err(ApiError::SessionNotFound)
        }

        fn turns(
            &self,
            _key: &str,
            _query: TurnQuery,
            _now: DateTime<Utc>,
        ) -> Result<Vec<TurnView>, ApiError> {
            Ok(Vec::new())
        }

        fn events(&self, _limit: usize) -> Result<Vec<EventView>, ApiError> {
            Ok(Vec::new())
        }

        fn alerts(&self, _limit: usize, _now: DateTime<Utc>) -> Result<Vec<AlertView>, ApiError> {
            Ok(Vec::new())
        }

        fn acknowledge_session(
            &self,
            _key: &str,
            _request: AckRequest,
            _now: DateTime<Utc>,
        ) -> Result<AckView, ApiError> {
            Err(ApiError::SessionNotFound)
        }

        fn stop_session(
            &self,
            _key: &str,
            _request: StopRequest,
            _now: DateTime<Utc>,
        ) -> Result<StopView, ApiError> {
            Err(ApiError::SessionNotFound)
        }

        fn config(&self) -> Result<ConfigView, ApiError> {
            Ok(config_view("alert", 1000, 3000, 900, true))
        }

        fn update_config(&self, _update: ConfigUpdate) -> Result<ConfigView, ApiError> {
            Ok(config_view("alert", 1000, 3000, 900, true))
        }

        fn onboarding(&self, _now: DateTime<Utc>) -> Result<OnboardingView, ApiError> {
            Ok(onboarding_view(false))
        }

        fn complete_onboarding(&self, _now: DateTime<Utc>) -> Result<OnboardingView, ApiError> {
            Ok(onboarding_view(false))
        }

        fn notification_health(&self) -> Result<NotificationView, ApiError> {
            Ok(notification_view(true, true, "ready"))
        }

        fn test_notification(&self, _now: DateTime<Utc>) -> Result<NotificationView, ApiError> {
            Ok(notification_view(true, true, "delivered"))
        }
    }

    impl FakeBackend {
        fn maybe_error(&self) -> Result<(), ApiError> {
            match self.next_error.borrow_mut().take() {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }
    }

    impl Backend for FakeBackend {
        fn snapshot(&self, _now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
            self.maybe_error()?;
            Ok(snapshot())
        }

        fn rescan(&self, _now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
            self.maybe_error()?;
            Ok(snapshot())
        }

        fn session(&self, key: &str, _now: DateTime<Utc>) -> Result<SessionView, ApiError> {
            self.maybe_error()?;
            snapshot()
                .sessions
                .into_iter()
                .find(|session| session.key == key || session.id == key)
                .ok_or(ApiError::SessionNotFound)
        }

        fn turns(
            &self,
            _key: &str,
            _query: TurnQuery,
            _now: DateTime<Utc>,
        ) -> Result<Vec<TurnView>, ApiError> {
            self.maybe_error()?;
            Ok(vec![TurnView {
                id: None,
                request_id: None,
                session_key: "codex:session/one".to_string(),
                session_id: Some("session/one".to_string()),
                provider: "codex".to_string(),
                at: Some(fixed_now()),
                model: None,
                input_tokens: 789,
                cached_input_tokens: 0,
                output_tokens: 0,
                cache_creation_input_tokens: 0,
                reasoning_output_tokens: 0,
                total_tokens: 789,
                spent_tokens: 789,
                cumulative_tokens: 789,
                source: "test".to_string(),
            }])
        }

        fn events(&self, limit: usize) -> Result<Vec<EventView>, ApiError> {
            self.maybe_error()?;
            Ok(vec![
                EventView {
                    seq: 1,
                    at: fixed_now(),
                    category: "alert".to_string(),
                    kind: "warning".to_string(),
                    message: "warning".to_string(),
                    run_id: None,
                    agent_id: Some("codex-worker".to_string()),
                    mode: Some("alert".to_string()),
                },
                EventView {
                    seq: 2,
                    at: fixed_now(),
                    category: "termination".to_string(),
                    kind: "completed".to_string(),
                    message: "stopped".to_string(),
                    run_id: None,
                    agent_id: Some("codex-worker".to_string()),
                    mode: Some("enforcement".to_string()),
                },
            ]
            .into_iter()
            .take(limit)
            .collect())
        }

        fn alerts(&self, limit: usize, _now: DateTime<Utc>) -> Result<Vec<AlertView>, ApiError> {
            self.maybe_error()?;
            Ok(vec![AlertView {
                severity: "warn".to_string(),
                label: "warning".to_string(),
                category: "warning".to_string(),
                message: "warning".to_string(),
                at: fixed_now(),
                seq: 1,
                run_id: None,
                agent_id: Some("codex-worker".to_string()),
                provider: Some("codex".to_string()),
                mode: Some("alert".to_string()),
                cwd: Some("/repo".to_string()),
                session_key: Some("codex:session/one".to_string()),
                session_id: Some("session/one".to_string()),
                actionable: false,
                can_acknowledge: true,
                explanation: "Usage or runtime crossed the warning policy.".to_string(),
            }]
            .into_iter()
            .take(limit)
            .collect())
        }

        fn acknowledge_session(
            &self,
            key: &str,
            request: AckRequest,
            _now: DateTime<Utc>,
        ) -> Result<AckView, ApiError> {
            self.maybe_error()?;
            Ok(AckView {
                session_key: key.to_string(),
                extend_seconds: request.extend_seconds,
                until: fixed_now(),
                reason: request.reason,
            })
        }

        fn stop_session(
            &self,
            key: &str,
            _request: StopRequest,
            _now: DateTime<Utc>,
        ) -> Result<StopView, ApiError> {
            self.maybe_error()?;
            Ok(StopView {
                session_key: key.to_string(),
                agent_id: "codex-worker".to_string(),
                pid: 4242,
                started_at: fixed_now(),
                owner: "phaedrus".to_string(),
                executable: Some("/usr/local/bin/codex".into()),
                bundle_id: None,
                team_id: None,
                scope: "tree".to_string(),
                scope_pids: vec![4242],
                result: crate::platform::TerminationResult {
                    soft_signaled: vec![4242],
                    ..crate::platform::TerminationResult::default()
                },
            })
        }

        fn config(&self) -> Result<ConfigView, ApiError> {
            self.maybe_error()?;
            Ok(config_view("alert", 1000, 3000, 900, true))
        }

        fn update_config(&self, update: ConfigUpdate) -> Result<ConfigView, ApiError> {
            self.maybe_error()?;
            Ok(config_view(
                update.mode.as_deref().unwrap_or("alert"),
                update.warn_turn_tokens.unwrap_or(1000),
                update.kill_turn_tokens.unwrap_or(3000),
                update.usage_window_seconds.unwrap_or(900),
                update.local_notifications.unwrap_or(true),
            ))
        }

        fn onboarding(&self, _now: DateTime<Utc>) -> Result<OnboardingView, ApiError> {
            self.maybe_error()?;
            Ok(onboarding_view(true))
        }

        fn complete_onboarding(&self, _now: DateTime<Utc>) -> Result<OnboardingView, ApiError> {
            self.maybe_error()?;
            Ok(onboarding_view(false))
        }

        fn notification_health(&self) -> Result<NotificationView, ApiError> {
            self.maybe_error()?;
            Ok(notification_view(true, true, "ready"))
        }

        fn test_notification(&self, _now: DateTime<Utc>) -> Result<NotificationView, ApiError> {
            self.maybe_error()?;
            let mut view = notification_view(true, true, "delivered");
            view.last_test_at = Some(fixed_now());
            Ok(view)
        }
    }

    fn onboarding_view(required: bool) -> OnboardingView {
        OnboardingView {
            required,
            config_path: Some("/tmp/curb/config.yaml".to_string()),
            mode: "alert".to_string(),
            action: "notify only; never kill".to_string(),
            mode_can_terminate: false,
            detected_providers: vec!["codex".to_string()],
            detected_workers: vec!["Codex Worker".to_string()],
            enforceable_agent_types: 1,
            watch_only_agent_types: 1,
            notifications: notification_view(true, true, "ready"),
            capabilities: PlatformCapabilities {
                platform: "test".to_string(),
                notifications: capability(true, "ready", "ready"),
                process_capture: capability(true, "ready", "process capture available"),
                process_identity: capability(true, "ready", "identity evidence available"),
                enforcement: capability(
                    false,
                    "disabled",
                    "current mode never terminates processes",
                ),
            },
            sources: snapshot().overview.sources,
            final_sentence: "Curb will notify on high-token turns.".to_string(),
            steps: Vec::new(),
        }
    }

    fn capability(available: bool, status: &str, message: &str) -> CapabilityView {
        CapabilityView {
            available,
            status: status.to_string(),
            message: message.to_string(),
        }
    }

    fn notification_view(enabled: bool, available: bool, status: &str) -> NotificationView {
        NotificationView {
            enabled,
            available,
            status: status.to_string(),
            message: status.to_string(),
            last_test_at: None,
            last_error: None,
        }
    }

    fn config_view(
        mode: &str,
        warn: i64,
        kill: i64,
        window: i64,
        local_notifications: bool,
    ) -> ConfigView {
        ConfigView {
            path: Some("/tmp/curb/config.yaml".to_string()),
            mode: mode.to_string(),
            usage_enabled: true,
            warn_turn_tokens: warn,
            kill_turn_tokens: kill,
            usage_window_seconds: window,
            usage_scan_seconds: 5,
            lookback_seconds: 86_400,
            process_warn_seconds: 90 * 60,
            process_kill_seconds: 120 * 60,
            ack_extension_seconds: 30 * 60,
            local_notifications,
            escalate_supervised: false,
            ledger_forward_url: None,
            agents: Vec::new(),
        }
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            overview: Overview {
                mode: "watch".to_string(),
                status: "WATCH".to_string(),
                message: "1 agent past your warn line".to_string(),
                working: 1,
                warn: 1,
                kill: 0,
                busiest_turn_tokens: 789,
                last_scan: fixed_now(),
                sources: Vec::new(),
                changes: Default::default(),
                capabilities: PlatformCapabilities {
                    platform: "test".to_string(),
                    notifications: capability(true, "ready", "ready"),
                    process_capture: capability(true, "ready", "process capture available"),
                    process_identity: capability(true, "ready", "identity evidence available"),
                    enforcement: capability(
                        false,
                        "disabled",
                        "current mode never terminates processes",
                    ),
                },
            },
            agents: vec![AgentView {
                id: "codex-worker".to_string(),
                provider: "codex".to_string(),
                label: "Codex Worker".to_string(),
                status: "working".to_string(),
                pid: 4242,
                process_started_at: Some(fixed_now()),
                running_for_seconds: Some(60),
                project: Some("repo".to_string()),
                cwd: Some("/repo".into()),
                session_key: Some("codex:session/one".to_string()),
                turn_tokens: 789,
                explanation: "Past your warn line since your last input.".to_string(),
            }],
            sessions: vec![SessionView {
                key: "codex:session/one".to_string(),
                id: "session/one".to_string(),
                provider: "codex".to_string(),
                status: "working".to_string(),
                alert: "warn".to_string(),
                can_stop: false,
                can_acknowledge: true,
                project: Some("repo".to_string()),
                cwd: Some("/repo".into()),
                models: vec!["model".to_string()],
                turn_tokens: 789,
                turn_context_tokens: 789,
                total_tokens: 1000,
                calls: 1,
                last_activity_at: Some(fixed_now()),
                pid: Some(4242),
                process_started_at: Some(fixed_now()),
                owner: Some("phaedrus".to_string()),
                executable: Some("/usr/local/bin/codex".into()),
                explanation: "Past your warn line since your last input.".to_string(),
                ..Default::default()
            }],
            turns: Vec::new(),
        }
    }
}
