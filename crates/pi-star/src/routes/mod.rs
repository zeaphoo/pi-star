pub mod agent;
pub mod auth;
pub mod files;
pub mod git;
pub mod mcp_oauth;
pub mod misc;
pub mod plugins_ops;
pub mod sessions;
pub mod settings;
pub mod skills_install;
pub mod terminal;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        [("Content-Type", "application/json")],
        serde_json::json!({ "error": message.into() }).to_string(),
    )
        .into_response()
}

pub fn internal(error: impl std::fmt::Display) -> Response {
    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

pub fn not_found(message: impl Into<String>) -> Response {
    json_error(StatusCode::NOT_FOUND, message)
}

pub fn forbidden(message: impl Into<String>) -> Response {
    json_error(StatusCode::FORBIDDEN, message)
}

pub fn bad_request(message: impl Into<String>) -> Response {
    json_error(StatusCode::BAD_REQUEST, message)
}

pub fn no_store<T: serde::Serialize>(value: T) -> Response {
    (
        StatusCode::OK,
        [
            ("Content-Type", "application/json"),
            ("Cache-Control", "no-store"),
        ],
        serde_json::to_string(&value).unwrap_or_else(|_| "{}".into()),
    )
        .into_response()
}
