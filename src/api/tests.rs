use std::cell::RefCell;
use std::fs;

use chrono::TimeZone;
use serde_json::Value;

use super::*;
use curb_core::onboarding::{CapabilityView, PlatformCapabilities};
use curb_core::service::{AgentView, Overview};

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
fn headless_routes_do_not_serve_ui_and_expose_public_liveness() {
    let mut server = Server::new("test-token", FakeBackend::default()).unwrap();
    server.serve_headless();
    let now = fixed_now();

    let root = server.handle(Request::new("GET", "/"), now);
    assert_eq!(root.status, 404);
    assert_eq!(root.headers.get("set-cookie"), None);
    assert!(root.text().contains("\"error\":\"headless server\""));
    assert!(root.text().contains("\"ui\":false"));
    assert!(!root.text().contains("<div id=\"root\"></div>"));

    let live = server.handle(Request::new("GET", "/v1/live"), now);
    assert_eq!(live.status, 200);
    assert!(live.text().contains("\"status\":\"live\""));

    let ready = server.handle(Request::new("GET", "/v1/ready"), now);
    assert_eq!(ready.status, 200);
    assert!(ready.text().contains("\"status\":\"ready\""));
    assert!(ready.text().contains("\"name\":\"config\""));
    assert!(ready.text().contains("\"name\":\"ledger\""));
    assert!(ready.text().contains("\"name\":\"usage_reader\""));
    assert!(ready.text().contains("\"name\":\"platform_capabilities\""));
    assert!(ready.text().contains("\"name\":\"watcher_runtime\""));
}

#[test]
fn headless_public_routes_do_not_weaken_protected_api_auth() {
    let mut server = Server::new("test-token", FakeBackend::default()).unwrap();
    server.serve_headless();
    let now = fixed_now();

    assert_eq!(
        server.handle(Request::new("GET", "/v1/health"), now).status,
        401
    );
    assert_eq!(
        server
            .handle(Request::new("GET", "/v1/overview"), now)
            .status,
        401
    );
    assert_eq!(server.handle(authed("GET", "/v1/health"), now).status, 200);

    let cross_origin_cookie = server.handle(
        Request::new("POST", "/v1/service/rescan")
            .cookie("curb_token=test-token")
            .origin("http://evil.example"),
        now,
    );
    assert_eq!(cross_origin_cookie.status, 403);
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
fn cookie_auth_requires_same_origin_for_unsafe_methods_and_cross_origin_reads() {
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

    let cross_origin_read = server.handle(
        Request::new("GET", "/v1/overview")
            .cookie("curb_token=test-token")
            .origin("http://127.0.0.1:3000")
            .endpoint("http", "127.0.0.1:8765"),
        now,
    );
    assert_eq!(cross_origin_read.status, 403);

    let same_origin_read = server.handle(
        Request::new("GET", "/v1/overview")
            .cookie("curb_token=test-token")
            .origin("http://127.0.0.1:8765")
            .endpoint("http", "127.0.0.1:8765"),
        now,
    );
    assert_eq!(same_origin_read.status, 200);

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
fn api_contract_fixtures_match_ui_facing_routes() {
    let mut server = Server::new("test-token", FakeBackend::default()).unwrap();
    server.serve_headless();
    let now = fixed_now();

    assert_json_fixture(
        server.handle(authed("GET", "/v1/snapshot"), now),
        include_str!("../../contracts/api/snapshot.json"),
    );
    assert_json_fixture(
        server.handle(authed("GET", "/v1/overview"), now),
        include_str!("../../contracts/api/overview.json"),
    );
    assert_json_fixture(
        server.handle(authed("GET", "/v1/sessions/codex:session%2Fone"), now),
        include_str!("../../contracts/api/session.json"),
    );
    assert_json_fixture(
        server.handle(
            authed("GET", "/v1/sessions/codex:session%2Fone/turns?limit=20"),
            now,
        ),
        include_str!("../../contracts/api/turns.json"),
    );
    assert_json_fixture(
        server.handle(authed("GET", "/v1/config"), now),
        include_str!("../../contracts/api/config.json"),
    );
    assert_json_fixture(
        server.handle(authed("GET", "/v1/onboarding"), now),
        include_str!("../../contracts/api/onboarding.json"),
    );
    assert_json_fixture(
        server.handle(Request::new("GET", "/v1/live"), now),
        include_str!("../../contracts/api/live.json"),
    );
    assert_json_fixture(
        server.handle(Request::new("GET", "/v1/ready"), now),
        include_str!("../../contracts/api/ready.json"),
    );
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
fn arc_backend_adapter_forwards_every_api_method() {
    let backend = Arc::new(SharedBackend);
    let now = fixed_now();

    assert!(Backend::snapshot(&backend, now).is_ok());
    assert!(Backend::readiness(&backend).is_ok());
    assert!(Backend::rescan(&backend, now).is_ok());
    assert!(matches!(
        Backend::session(&backend, "missing", now),
        Err(ApiError::SessionNotFound)
    ));
    assert!(
        Backend::turns(&backend, "missing", TurnQuery::default(), now)
            .unwrap()
            .is_empty()
    );
    assert!(Backend::events(&backend, 10).unwrap().is_empty());
    assert!(Backend::alerts(&backend, 10, now).unwrap().is_empty());
    assert!(matches!(
        Backend::acknowledge_session(&backend, "missing", AckRequest::default(), now),
        Err(ApiError::SessionNotFound)
    ));
    assert!(matches!(
        Backend::stop_session(&backend, "missing", StopRequest::default(), now),
        Err(ApiError::SessionNotFound)
    ));
    assert!(Backend::config(&backend).is_ok());
    assert!(Backend::update_config(&backend, ConfigUpdate::default()).is_ok());
    assert!(Backend::onboarding(&backend, now).is_ok());
    assert!(Backend::complete_onboarding(&backend, now).is_ok());
    assert!(Backend::notification_health(&backend).is_ok());
    assert!(Backend::test_notification(&backend, now).is_ok());
}

#[test]
fn runtime_backend_adapter_maps_core_runtime_contract_to_api_errors() {
    let state = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let cfg = curb_core::config::Config::local_default(
        curb_core::config::Mode::Visibility,
        state.path().join("state"),
    );
    let runtime = Runtime::new(cfg, home.path(), curb_core::platform::EmptyPlatform)
        .with_config_path(state.path().join("curb.yaml"));
    let now = fixed_now();

    assert!(Backend::snapshot(&runtime, now).is_ok());
    assert!(Backend::readiness(&runtime).is_ok());
    assert!(Backend::rescan(&runtime, now).is_ok());
    assert!(matches!(
        Backend::session(&runtime, "missing", now),
        Err(ApiError::SessionNotFound)
    ));
    assert!(matches!(
        Backend::turns(&runtime, "missing", TurnQuery::default(), now),
        Err(ApiError::SessionNotFound)
    ));
    assert!(Backend::events(&runtime, 10).unwrap().is_empty());
    assert!(Backend::alerts(&runtime, 10, now).unwrap().is_empty());
    assert!(matches!(
        Backend::acknowledge_session(&runtime, "missing", AckRequest::default(), now),
        Err(ApiError::SessionNotFound)
    ));
    assert!(matches!(
        Backend::stop_session(&runtime, "missing", StopRequest::default(), now),
        Err(ApiError::InvalidStop(_))
    ));
    assert!(Backend::config(&runtime).is_ok());
    assert!(Backend::update_config(&runtime, ConfigUpdate::default()).is_ok());
    assert!(Backend::onboarding(&runtime, now).is_ok());
    assert!(Backend::complete_onboarding(&runtime, now).is_ok());
    assert!(matches!(
        Backend::test_notification(&runtime, now),
        Err(ApiError::NotificationsUnavailable(_))
    ));
    assert!(matches!(
        Backend::notification_health(&runtime),
        Ok(view) if !view.available
    ));
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
    assert!(turns.text().contains("\"model\":\"model\""));
    assert!(turns.text().contains("\"input_tokens\":789"));
    assert!(turns.text().contains("\"cached_input_tokens\":12"));
    assert!(turns.text().contains("\"cache_creation_input_tokens\":34"));
    assert!(turns.text().contains("\"output_tokens\":56"));
    assert!(turns.text().contains("\"reasoning_output_tokens\":78"));
    assert!(turns.text().contains("\"total_tokens\":789"));
    assert!(turns.text().contains("\"spent_tokens\":777"));
    assert!(turns.text().contains("\"cumulative_tokens\":1234"));
    assert!(turns.text().contains("\"source\":\"test usage log\""));
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
fn write_payloads_reject_unknown_fields_before_backend_actions() {
    let server = Server::new("test-token", FakeBackend::default()).unwrap();
    let now = fixed_now();

    let ack = server.handle(
        authed("POST", "/v1/sessions/codex:session%2Fone/ack")
            .body(r#"{"extend_seconds":60,"unexpected":true}"#),
        now,
    );
    assert_eq!(ack.status, 400);
    assert_eq!(*server.backend.ack_calls.borrow(), 0);

    let stop = server.handle(
        authed("POST", "/v1/sessions/codex:session%2Fone/stop").body(
            r#"{"confirm":true,"scope":"tree","expected":{"pid":4242,"owner":"phaedrus","unexpected":true}}"#,
        ),
        now,
    );
    assert_eq!(stop.status, 400);
    assert_eq!(*server.backend.stop_calls.borrow(), 0);

    let config = server.handle(
        authed("PUT", "/v1/config").body(r#"{"mode":"alert","unexpected":true}"#),
        now,
    );
    assert_eq!(config.status, 400);
    assert_eq!(*server.backend.update_config_calls.borrow(), 0);
}

#[test]
fn config_update_rejects_unknown_mode_before_backend_mutation() {
    let server = Server::new("test-token", FakeBackend::default()).unwrap();
    let now = fixed_now();

    let response = server.handle(
        authed("PUT", "/v1/config").body(r#"{"mode":"surveillance"}"#),
        now,
    );

    assert_eq!(response.status, 400);
    assert!(response.text().contains("unknown variant"));
    assert_eq!(*server.backend.update_config_calls.borrow(), 0);
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

fn assert_json_fixture(response: Response, fixture: &str) {
    assert_eq!(response.status, 200);
    let actual = serde_json::from_slice::<Value>(&response.body).unwrap();
    let expected = serde_json::from_str::<Value>(fixture).unwrap();
    assert_eq!(actual, expected);
}

#[derive(Default)]
struct FakeBackend {
    next_error: RefCell<Option<ApiError>>,
    ack_calls: RefCell<usize>,
    stop_calls: RefCell<usize>,
    update_config_calls: RefCell<usize>,
}

struct SharedBackend;

impl Backend for SharedBackend {
    fn snapshot(&self, _now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
        Ok(snapshot())
    }

    fn readiness(&self) -> Result<ReadinessView, ApiError> {
        Ok(readiness_view())
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

    fn readiness(&self) -> Result<ReadinessView, ApiError> {
        self.maybe_error()?;
        Ok(readiness_view())
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
            model: Some("model".to_string()),
            input_tokens: 789,
            cached_input_tokens: 12,
            output_tokens: 56,
            cache_creation_input_tokens: 34,
            reasoning_output_tokens: 78,
            total_tokens: 789,
            spent_tokens: 777,
            cumulative_tokens: 1234,
            source: "test usage log".to_string(),
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
        *self.ack_calls.borrow_mut() += 1;
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
        *self.stop_calls.borrow_mut() += 1;
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
            result: curb_core::platform::TerminationResult {
                soft_signaled: vec![4242],
                ..curb_core::platform::TerminationResult::default()
            },
        })
    }

    fn config(&self) -> Result<ConfigView, ApiError> {
        self.maybe_error()?;
        Ok(config_view("alert", 1000, 3000, 900, true))
    }

    fn update_config(&self, update: ConfigUpdate) -> Result<ConfigView, ApiError> {
        self.maybe_error()?;
        *self.update_config_calls.borrow_mut() += 1;
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
            enforcement: capability(false, "disabled", "current mode never terminates processes"),
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

fn readiness_view() -> ReadinessView {
    ReadinessView {
        status: "ready".to_string(),
        app: "curb".to_string(),
        api_version: 1,
        checks: [
            "config",
            "ledger",
            "usage_reader",
            "platform_capabilities",
            "notifications",
            "watcher_runtime",
        ]
        .into_iter()
        .map(|name| curb_core::service::ReadinessCheckView {
            name: name.to_string(),
            status: "ok".to_string(),
            reason: None,
        })
        .collect(),
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
