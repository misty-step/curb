use chrono::{DateTime, Utc};
use serde_json::json;

use super::{Backend, Request, Response, response, routes, wire};

pub(super) fn public<B: Backend>(backend: &B, request: Request) -> Response {
    match routes::public(&request) {
        routes::PublicRoute::Live => response::json_response(
            200,
            json!({
                "status": "live",
                "app": "curb",
                "api_version": 1,
            }),
        ),
        routes::PublicRoute::Ready => ready(backend),
        routes::PublicRoute::MethodNotAllowed => {
            response::error_response(405, "method not allowed")
        }
        routes::PublicRoute::NotFound => response::error_response(404, "not found"),
    }
}

pub(super) fn protected<B: Backend>(backend: &B, request: Request, now: DateTime<Utc>) -> Response {
    match routes::protected(&request, now) {
        routes::Route::Health => response::json_response(
            200,
            json!({
                "ok": true,
                "app": "curb",
                "api_version": 1,
            }),
        ),
        routes::Route::Snapshot => backend
            .snapshot(now)
            .map(response::json_ok)
            .unwrap_or_else(response::api_error_response),
        routes::Route::Overview => backend
            .snapshot(now)
            .map(|snapshot| response::json_ok(snapshot.overview))
            .unwrap_or_else(response::api_error_response),
        routes::Route::Agents => backend
            .snapshot(now)
            .map(|snapshot| response::json_ok(snapshot.agents))
            .unwrap_or_else(response::api_error_response),
        routes::Route::Sessions => backend
            .snapshot(now)
            .map(|snapshot| response::json_ok(snapshot.sessions))
            .unwrap_or_else(response::api_error_response),
        routes::Route::Rescan => backend
            .rescan(now)
            .map(response::json_ok)
            .unwrap_or_else(response::api_error_response),
        routes::Route::Events { limit } => backend
            .events(limit)
            .map(response::json_ok)
            .unwrap_or_else(response::api_error_response),
        routes::Route::Alerts { limit } => backend
            .alerts(limit, now)
            .map(response::json_ok)
            .unwrap_or_else(response::api_error_response),
        routes::Route::NotificationHealth => backend
            .notification_health()
            .map(response::json_ok)
            .unwrap_or_else(response::api_error_response),
        routes::Route::NotificationTest => {
            let response = backend
                .test_notification(now)
                .map(response::json_ok)
                .unwrap_or_else(response::api_error_response);
            crate::observability::emit_notification_attempt(response.status);
            response
        }
        routes::Route::Config => backend
            .config()
            .map(response::json_ok)
            .unwrap_or_else(response::api_error_response),
        routes::Route::UpdateConfig => wire::decode_config_update(&request)
            .and_then(|update| backend.update_config(update))
            .map(response::json_ok)
            .unwrap_or_else(response::api_error_response),
        routes::Route::Onboarding => backend
            .onboarding(now)
            .map(response::json_ok)
            .unwrap_or_else(response::api_error_response),
        routes::Route::CompleteOnboarding => backend
            .complete_onboarding(now)
            .map(response::json_ok)
            .unwrap_or_else(response::api_error_response),
        routes::Route::Session { key } => backend
            .session(&key, now)
            .map(response::json_ok)
            .unwrap_or_else(response::api_error_response),
        routes::Route::SessionTurns { key, query } => backend
            .turns(&key, query, now)
            .map(response::json_ok)
            .unwrap_or_else(response::api_error_response),
        routes::Route::Ack { key } => wire::decode_ack(&request)
            .and_then(|ack| backend.acknowledge_session(&key, ack, now))
            .map(response::json_ok)
            .unwrap_or_else(response::api_error_response),
        routes::Route::Stop { key } => {
            let response = wire::decode_stop(&request)
                .and_then(|stop| backend.stop_session(&key, stop, now))
                .map(response::json_ok)
                .unwrap_or_else(response::api_error_response);
            crate::observability::emit_stop_outcome(response.status);
            response
        }
        routes::Route::InvalidSessionKey => response::error_response(400, "invalid session key"),
        routes::Route::MethodNotAllowed => response::error_response(405, "method not allowed"),
        routes::Route::NotFound => response::error_response(404, "not found"),
    }
}

fn ready<B: Backend>(backend: &B) -> Response {
    match backend.readiness() {
        Ok(view) => {
            let status = if view.status == "ready" { 200 } else { 503 };
            response::json_response(
                status,
                serde_json::to_value(view).expect("serialize readiness"),
            )
        }
        Err(error) => response::api_error_response(error),
    }
}
