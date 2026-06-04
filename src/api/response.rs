use serde::Serialize;
use serde_json::{Value, json};

use super::{ApiError, HeaderMap, Response};

pub(super) fn json_ok(value: impl Serialize) -> Response {
    json_response(
        200,
        serde_json::to_value(value).expect("serialize api response"),
    )
}

pub(super) fn json_response(status: u16, value: Value) -> Response {
    let mut headers = HeaderMap::default();
    headers.insert("content-type", "application/json");
    Response {
        status,
        headers,
        body: serde_json::to_vec(&value).expect("serialize json"),
    }
}

pub(super) fn error_response(status: u16, message: &str) -> Response {
    json_response(status, json!({ "error": message }))
}

pub(super) fn api_error_response(error: ApiError) -> Response {
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
