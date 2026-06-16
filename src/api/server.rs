use chrono::{DateTime, Utc};
use serde_json::json;

use super::{ApiError, Backend, Request, Response, auth, dispatch, response, routes};

#[derive(Clone)]
pub struct Server<B: Backend> {
    token: String,
    pub(super) backend: B,
    ui: bool,
    headless: bool,
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
            headless: false,
        })
    }

    pub fn serve_ui(&mut self) {
        self.ui = true;
        self.headless = false;
    }

    pub fn serve_headless(&mut self) {
        self.ui = false;
        self.headless = true;
    }

    pub fn handle(&self, request: Request, now: DateTime<Utc>) -> Response {
        if !request.path.starts_with("/v1/") {
            if self.ui
                && let Some(mut response) = crate::web::handle(&request)
            {
                response.headers.insert(
                    "set-cookie",
                    auth::token_cookie(&self.token, request.scheme == "https"),
                );
                return response;
            }
            if self.headless {
                return response::json_response(
                    404,
                    json!({
                        "error": "headless server",
                        "app": "curb",
                        "ui": false,
                    }),
                );
            }
            return Response::empty(404);
        }
        let mut cors_headers = auth::cors_headers(&request);
        if request.method == "OPTIONS" {
            return Response::empty(204).with_headers(cors_headers);
        }
        if routes::is_public(&request.path) {
            let mut response = dispatch::public(&self.backend, request);
            response.headers.append(&mut cors_headers);
            return response;
        }
        if !auth::authorized(&request, &self.token) {
            return response::error_response(401, "unauthorized").with_headers(cors_headers);
        }
        if auth::uses_cookie_auth(&request, &self.token)
            && (auth::unsafe_method(&request.method) || auth::has_origin(&request))
            && !auth::same_origin(&request)
        {
            return response::error_response(403, "forbidden").with_headers(cors_headers);
        }
        let mut response = dispatch::protected(&self.backend, request, now);
        response.headers.append(&mut cors_headers);
        response
    }
}
