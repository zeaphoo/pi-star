use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::state::AppState;

const COOKIE: &str = "pi-star-auth";

pub async fn security_middleware(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    if !host_allowed(&state, request.headers()) {
        return json_error(StatusCode::FORBIDDEN, "Untrusted host");
    }
    if !origin_allowed(&state, request.headers()) {
        return json_error(StatusCode::FORBIDDEN, "Untrusted origin");
    }
    if state.password.is_none() || is_public_path(request.uri().path()) {
        return next.run(request).await;
    }
    if authenticated(&state, request.headers()) {
        return next.run(request).await;
    }
    if request.uri().path().starts_with("/api/") {
        return json_error(StatusCode::UNAUTHORIZED, "Authentication required");
    }
    let next_path = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let location = format!("/login?next={}", urlencoding(next_path));
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .body(Body::empty())
        .unwrap()
        .into_response()
}

fn is_public_path(path: &str) -> bool {
    path == "/login"
        || path == "/api/web-auth"
        || path == "/manifest.webmanifest"
        || path == "/sw.js"
        || path == "/offline.html"
        || path == "/favicon.ico"
        || path == "/provider-icons.svg"
        || path == "/api/mcp/oauth/callback"
        || path.starts_with("/assets/")
        || path.starts_with("/icons/")
}

pub fn authenticated(state: &AppState, headers: &HeaderMap) -> bool {
    if let Some(value) = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
    {
        for cookie in value.split(';') {
            let Some((name, value)) = cookie.trim().split_once('=') else {
                continue;
            };
            if name == COOKIE && constant_time_eq(value.trim(), &state.auth_token) {
                return true;
            }
        }
    }
    if let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Basic "))
    {
        use base64::Engine;
        if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(value) {
            let decoded = String::from_utf8_lossy(&decoded);
            if let Some((user, password)) = decoded.split_once(':') {
                if user == "pi" {
                    if let Some(expected) = &state.password {
                        return constant_time_eq(password, expected);
                    }
                }
            }
        }
    }
    false
}

pub fn login_cookie(token: &str) -> String {
    format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict")
}

pub fn logout_cookie() -> String {
    format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

fn host_allowed(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    let hostname = host
        .trim()
        .trim_start_matches('[')
        .split(']')
        .next()
        .unwrap_or(host)
        .split(':')
        .next()
        .unwrap_or(host);
    if hostname == "127.0.0.1"
        || hostname == "localhost"
        || hostname == "::1"
        || hostname.starts_with("10.")
        || hostname.starts_with("192.168.")
        || hostname.starts_with("172.")
    {
        return true;
    }
    state
        .allowed_hosts
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(hostname) || allowed == host)
}

fn origin_allowed(_state: &AppState, headers: &HeaderMap) -> bool {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    if origin == "null" {
        return false;
    }
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    match origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    {
        Some(origin_host) => origin_host.eq_ignore_ascii_case(host),
        None => false,
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in a.bytes().zip(b.bytes()) {
        difference |= left ^ right;
    }
    difference == 0
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        format!(
            "{{\"error\":{}}}",
            serde_json::to_string(message).unwrap_or_else(|_| "\"error\"".into())
        ),
    )
        .into_response()
}

fn urlencoding(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub fn check_password(state: &AppState, password: &str) -> bool {
    state
        .password
        .as_deref()
        .is_some_and(|expected| constant_time_eq(password, expected))
}
