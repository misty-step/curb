use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;
use std::time::Instant;

use chrono::Utc;
use thiserror::Error;

use crate::api::{Backend, Request, Response, Server};

const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_HEADER_LINES: usize = 100;
const MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum HttpError {
    #[error("listen address must be loopback")]
    NonLoopbackAddress,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub fn bind_loopback(addr: &str) -> Result<TcpListener, HttpError> {
    if !is_loopback_host(addr) {
        return Err(HttpError::NonLoopbackAddress);
    }
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    if !local.ip().is_loopback() {
        return Err(HttpError::NonLoopbackAddress);
    }
    Ok(listener)
}

pub fn serve_until<B: Backend>(
    listener: TcpListener,
    server: &Server<B>,
    mut should_shutdown: impl FnMut() -> bool,
) -> Result<(), HttpError> {
    listener.set_nonblocking(true)?;
    for stream in listener.incoming() {
        if should_shutdown() {
            break;
        }
        match stream {
            Ok(stream) => {
                let _ = handle_stream(stream, server);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(_) => continue,
        }
    }
    Ok(())
}

pub fn handle_stream<B: Backend>(
    mut stream: TcpStream,
    server: &Server<B>,
) -> Result<(), HttpError> {
    let request = read_request(&mut stream, "http")?;
    let started = Instant::now();
    let method = request.method.clone();
    let path = request.path.clone();
    let response = server.handle(request, Utc::now());
    let status = response.status;
    write_response(&mut stream, &response)?;
    crate::observability::emit_api_request(&method, &path, status, started.elapsed());
    Ok(())
}

pub fn read_request(stream: &mut impl Read, scheme: &str) -> Result<Request, HttpError> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Err(HttpError::BadRequest("empty request".to_string()));
    }
    let mut header_bytes = request_line.len();
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| HttpError::BadRequest("missing method".to_string()))?;
    let target = parts
        .next()
        .ok_or_else(|| HttpError::BadRequest("missing target".to_string()))?;
    let mut request = Request::new(method, target);
    request.scheme = scheme.to_string();
    let mut content_length = 0usize;
    let mut header_lines = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(HttpError::BadRequest(
                "headers were not terminated".to_string(),
            ));
        }
        header_lines += 1;
        header_bytes += line.len();
        if header_lines > MAX_HEADER_LINES || header_bytes > MAX_HEADER_BYTES {
            return Err(HttpError::BadRequest("headers too large".to_string()));
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(HttpError::BadRequest("malformed header".to_string()));
        };
        let name = name.trim();
        let value = value.trim();
        if name.eq_ignore_ascii_case("host") {
            request.host = value.to_string();
        }
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value
                .parse::<usize>()
                .map_err(|_| HttpError::BadRequest("invalid content-length".to_string()))?;
            if content_length > MAX_BODY_BYTES {
                return Err(HttpError::BadRequest("body too large".to_string()));
            }
        }
        request.headers.insert(name, value);
    }
    if content_length > 0 {
        request.body.resize(content_length, 0);
        reader.read_exact(&mut request.body)?;
    }
    Ok(request)
}

pub fn write_response(mut writer: impl Write, response: &Response) -> io::Result<()> {
    write!(
        writer,
        "HTTP/1.1 {} {}\r\n",
        response.status,
        reason_phrase(response.status)
    )?;
    for (name, value) in response.headers.iter() {
        write!(writer, "{name}: {value}\r\n")?;
    }
    write!(writer, "Content-Length: {}\r\n", response.body.len())?;
    write!(writer, "Connection: close\r\n\r\n")?;
    writer.write_all(&response.body)?;
    writer.flush()
}

pub fn is_loopback_host(host: &str) -> bool {
    let without_port = host
        .strip_prefix('[')
        .and_then(|host| host.split_once(']').map(|(host, _)| host))
        .or_else(|| host.rsplit_once(':').map(|(host, _)| host))
        .unwrap_or(host);
    without_port == "localhost"
        || without_port
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        _ => "OK",
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io::Cursor;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use chrono::{DateTime, TimeZone, Utc};

    use super::*;
    use crate::api::{ApiError, Backend, Server};
    use curb_core::onboarding::{
        CapabilityView, NotificationView, OnboardingView, PlatformCapabilities,
    };
    use curb_core::runtime::TurnQuery;
    use curb_core::service::{
        AckRequest, AckView, AlertView, ConfigUpdate, ConfigView, EventView, Overview, SessionView,
        Snapshot, StopRequest, StopView, TurnView,
    };

    #[test]
    fn read_request_preserves_target_headers_and_body() {
        let raw = b"POST /v1/sessions/codex:session%2Fone/ack?limit=1 HTTP/1.1\r\nHost: 127.0.0.1:8765\r\nAuthorization: Bearer test-token\r\nContent-Length: 21\r\n\r\n{\"extend_seconds\":60}";

        let request = read_request(&mut Cursor::new(raw), "http").unwrap();

        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/sessions/codex:session%2Fone/ack");
        assert_eq!(request.query, "limit=1");
        assert_eq!(request.host, "127.0.0.1:8765");
        assert_eq!(
            request.headers.get("authorization"),
            Some("Bearer test-token")
        );
        assert_eq!(request.body, br#"{"extend_seconds":60}"#);
    }

    #[test]
    fn read_request_rejects_oversized_body_before_allocating() {
        let raw = b"POST /v1/snapshot HTTP/1.1\r\nHost: 127.0.0.1:8765\r\nContent-Length: 1048577\r\n\r\n";

        let err = read_request(&mut Cursor::new(raw), "http").unwrap_err();

        assert!(
            matches!(err, HttpError::BadRequest(message) if message.contains("body too large"))
        );
    }

    #[test]
    fn write_response_emits_status_headers_and_body() {
        let mut response = Response::empty_for_test(200, br#"{"ok":true}"#.to_vec());
        response.headers.insert("content-type", "application/json");
        let mut out = Vec::new();

        write_response(&mut out, &response).unwrap();
        let text = String::from_utf8(out).unwrap();

        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("content-type: application/json\r\n"));
        assert!(text.contains("Content-Length: 11\r\n"));
        assert!(text.ends_with(r#"{"ok":true}"#));
    }

    #[test]
    fn handle_stream_runs_router_end_to_end() {
        let server = Server::new("test-token", FakeBackend::default()).unwrap();
        let raw = b"GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1:8765\r\nAuthorization: Bearer test-token\r\n\r\n";

        let request = read_request(&mut Cursor::new(raw), "http").unwrap();
        let response = server.handle(request, fixed_now());

        assert_eq!(response.status, 200);
        assert!(response.text().contains("\"app\":\"curb\""));
    }

    #[test]
    fn serve_until_returns_when_shutdown_is_requested() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server = Server::new("test-token", FakeBackend::default()).unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let checks = RefCell::new(0usize);

        serve_until(listener, &server, || {
            let mut checks = checks.borrow_mut();
            *checks += 1;
            if *checks > 1 {
                shutdown.store(true, Ordering::SeqCst);
            }
            shutdown.load(Ordering::SeqCst)
        })
        .unwrap();

        assert!(shutdown.load(Ordering::SeqCst));
    }

    #[test]
    fn handle_stream_serves_embedded_ui_when_enabled() {
        let mut server = Server::new("test-token", FakeBackend::default()).unwrap();
        server.serve_ui();
        let raw = b"GET / HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n";

        let request = read_request(&mut Cursor::new(raw), "http").unwrap();
        let response = server.handle(request, fixed_now());

        assert_eq!(response.status, 200);
        assert_eq!(
            response.headers.get("content-type"),
            Some("text/html; charset=utf-8")
        );
        assert_eq!(
            response.headers.get("set-cookie"),
            Some("curb_token=test-token; Path=/v1/; HttpOnly; SameSite=Strict")
        );
        assert!(response.text().contains("<div id=\"root\"></div>"));
    }

    #[test]
    fn loopback_host_helper_accepts_only_loopback_hosts() {
        assert!(is_loopback_host("127.0.0.1:8765"));
        assert!(is_loopback_host("[::1]:8765"));
        assert!(is_loopback_host("localhost:8765"));
        assert!(!is_loopback_host("0.0.0.0:8765"));
        assert!(!is_loopback_host("192.168.1.50:8765"));
    }

    #[derive(Default)]
    struct FakeBackend {
        _calls: RefCell<usize>,
    }

    impl Backend for FakeBackend {
        fn snapshot(&self, _now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
            Ok(Snapshot {
                overview: Overview {
                    mode: "watch".to_string(),
                    status: "OK".to_string(),
                    message: "No agents over your limits".to_string(),
                    working: 0,
                    warn: 0,
                    kill: 0,
                    busiest_turn_tokens: 0,
                    last_scan: fixed_now(),
                    sources: Vec::new(),
                    changes: Default::default(),
                    capabilities: Default::default(),
                },
                agents: Vec::new(),
                sessions: Vec::new(),
                turns: Vec::new(),
            })
        }

        fn readiness(&self) -> Result<curb_core::service::ReadinessView, ApiError> {
            Ok(curb_core::service::ReadinessView {
                status: "ready".to_string(),
                app: "curb".to_string(),
                api_version: 1,
                checks: Vec::new(),
            })
        }

        fn rescan(&self, now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
            self.snapshot(now)
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
            key: &str,
            request: AckRequest,
            _now: DateTime<Utc>,
        ) -> Result<AckView, ApiError> {
            Ok(AckView {
                session_key: key.to_string(),
                extend_seconds: request.extend_seconds,
                until: fixed_now(),
                reason: request.reason,
            })
        }

        fn stop_session(
            &self,
            _key: &str,
            _request: StopRequest,
            _now: DateTime<Utc>,
        ) -> Result<StopView, ApiError> {
            Err(ApiError::StopConflict("not actionable".to_string()))
        }

        fn config(&self) -> Result<ConfigView, ApiError> {
            Ok(config_view())
        }

        fn update_config(&self, _update: ConfigUpdate) -> Result<ConfigView, ApiError> {
            Ok(config_view())
        }

        fn onboarding(&self, _now: DateTime<Utc>) -> Result<OnboardingView, ApiError> {
            Ok(onboarding_view(true))
        }

        fn complete_onboarding(&self, _now: DateTime<Utc>) -> Result<OnboardingView, ApiError> {
            Ok(onboarding_view(false))
        }

        fn notification_health(&self) -> Result<NotificationView, ApiError> {
            Ok(notification_view("ready"))
        }

        fn test_notification(&self, _now: DateTime<Utc>) -> Result<NotificationView, ApiError> {
            Ok(notification_view("delivered"))
        }
    }

    fn onboarding_view(required: bool) -> OnboardingView {
        OnboardingView {
            required,
            config_path: None,
            mode: "visibility".to_string(),
            action: "record only; no warnings or kills".to_string(),
            mode_can_terminate: false,
            detected_providers: Vec::new(),
            detected_workers: Vec::new(),
            enforceable_agent_types: 0,
            watch_only_agent_types: 0,
            notifications: notification_view("ready"),
            capabilities: PlatformCapabilities {
                platform: "test".to_string(),
                notifications: capability(true, "ready", "ready"),
                process_capture: capability(true, "ready", "ready"),
                process_identity: capability(false, "waiting", "waiting"),
                enforcement: capability(false, "disabled", "disabled"),
            },
            sources: Vec::new(),
            final_sentence: "Curb will record local agent activity.".to_string(),
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

    fn notification_view(status: &str) -> NotificationView {
        NotificationView {
            enabled: true,
            available: true,
            status: status.to_string(),
            message: status.to_string(),
            last_test_at: None,
            last_error: None,
        }
    }

    fn config_view() -> ConfigView {
        ConfigView {
            path: None,
            mode: "visibility".to_string(),
            usage_enabled: true,
            warn_turn_tokens: 1,
            kill_turn_tokens: 2,
            usage_window_seconds: 60,
            usage_scan_seconds: 5,
            lookback_seconds: 3600,
            process_warn_seconds: 60,
            process_kill_seconds: 120,
            ack_extension_seconds: 30,
            local_notifications: true,
            escalate_supervised: false,
            agents: Vec::new(),
        }
    }

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap()
    }
}
