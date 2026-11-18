pub mod agent;
pub mod auth;
pub mod files;
pub mod git;
pub mod mcp_oauth;
pub mod misc;
pub mod oauth_bridge;
pub mod plugins_ops;
pub mod sessions;
pub mod settings;
pub mod skills_install;
pub mod terminal;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Run blocking filesystem, subprocess or whole-file parsing work on the
/// runtime's blocking pool instead of an async worker thread.
///
/// The HTTP handlers call a number of synchronous helpers (`std::fs`,
/// `std::process::Command`, `read_session`); running them inline would pin a
/// worker for as long as the IO takes and, with enough concurrent requests,
/// starve every other endpoint (SSE heartbeats included).
pub async fn blocking<T, F>(work: F) -> Result<T, Response>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(value) => Ok(value),
        Err(error) => Err(internal(format!("blocking task failed: {error}"))),
    }
}

/// `session::resolve_session_path` walks the sessions directory, so it belongs
/// on the blocking pool like any other filesystem read.
pub async fn session_path(config: pi_star_core::Config, id: String) -> Option<std::path::PathBuf> {
    blocking(move || pi_star_core::session::resolve_session_path(&config, &id))
        .await
        .ok()
        .flatten()
}

/// Same as [`blocking`], for helpers that return `Result` and need the error
/// rather than an HTTP response.
pub async fn blocking_result<T, E, F>(work: F) -> anyhow::Result<T>
where
    F: FnOnce() -> Result<T, E> + Send + 'static,
    T: Send + 'static,
    E: Into<anyhow::Error> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| anyhow::anyhow!("blocking task failed: {error}"))?
        .map_err(Into::into)
}

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
