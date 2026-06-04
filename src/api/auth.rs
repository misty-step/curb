use url::Url;

use super::{HeaderMap, Request, TOKEN_COOKIE};

pub(super) fn token_cookie(token: &str, secure: bool) -> String {
    let mut cookie = format!("{TOKEN_COOKIE}={token}; Path=/v1/; HttpOnly; SameSite=Strict");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

pub(super) fn authorized(request: &Request, token: &str) -> bool {
    constant_time_eq(bearer_token(request.header_value("authorization")), token)
        || constant_time_eq(
            request.header_value("x-curb-token").unwrap_or_default(),
            token,
        )
        || request
            .cookie_value(TOKEN_COOKIE)
            .is_some_and(|cookie| constant_time_eq(&cookie, token))
}

pub(super) fn uses_cookie_auth(request: &Request, token: &str) -> bool {
    !constant_time_eq(bearer_token(request.header_value("authorization")), token)
        && !constant_time_eq(
            request.header_value("x-curb-token").unwrap_or_default(),
            token,
        )
        && request
            .cookie_value(TOKEN_COOKIE)
            .is_some_and(|cookie| constant_time_eq(&cookie, token))
}

pub(super) fn cors_headers(request: &Request) -> HeaderMap {
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

pub(super) fn same_origin(request: &Request) -> bool {
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

pub(super) fn has_origin(request: &Request) -> bool {
    request.header_value("origin").is_some()
}

pub(super) fn unsafe_method(method: &str) -> bool {
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
