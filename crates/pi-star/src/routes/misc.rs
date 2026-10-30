use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use pi_star_core::project::{ProjectInfo, project_key};
use pi_star_core::settings;

use crate::auth::{check_password, login_cookie, logout_cookie};
use crate::routes::{bad_request, forbidden, internal, no_store, not_found};
use crate::state::AppState;

pub async fn web_auth_status(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Response {
    let enabled = state.password.is_some();
    let authenticated = !enabled || crate::auth::authenticated(&state, &headers);
    no_store(json!({"enabled": enabled, "authenticated": authenticated}))
}

#[derive(Debug, Deserialize)]
pub struct LoginBody {
    password: String,
}

pub async fn web_auth_login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginBody>,
) -> Response {
    if state.password.is_none() {
        return not_found("Password authentication is disabled");
    }
    if let Some(retry_after) = state.auth_throttle.lock().retry_after() {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [
                (header::CONTENT_TYPE, "application/json".to_string()),
                (header::RETRY_AFTER, retry_after.to_string()),
            ],
            json!({"error": "Too many failed attempts", "retryAfterMs": retry_after * 1000})
                .to_string(),
        )
            .into_response();
    }
    if !check_password(&state, &body.password) {
        let retry_after = state.auth_throttle.lock().fail();
        return (
            StatusCode::UNAUTHORIZED,
            [
                (header::CONTENT_TYPE, "application/json".to_string()),
                (header::RETRY_AFTER, retry_after.to_string()),
            ],
            json!({"error": "Invalid password", "retryAfterMs": retry_after * 1000}).to_string(),
        )
            .into_response();
    }
    state.auth_throttle.lock().succeed();
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (header::SET_COOKIE, login_cookie(&state.auth_token)),
        ],
        json!({"ok": true}).to_string(),
    )
        .into_response()
}

pub async fn web_auth_logout() -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (header::SET_COOKIE, logout_cookie()),
        ],
        json!({"ok": true}).to_string(),
    )
        .into_response()
}

pub async fn push_config(State(state): State<Arc<AppState>>) -> Response {
    match state.push.public_key() {
        Ok(public_key) => no_store(json!({"publicKey": public_key, "enabled": true})),
        Err(error) => internal(error),
    }
}

pub async fn push_subscribe(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    let Some(subscription) = body
        .get("subscription")
        .and_then(pi_star_core::push::parse_subscription)
    else {
        return bad_request("Invalid push subscription");
    };
    match state.push.add_subscription(subscription) {
        Ok(()) => no_store(json!({"ok": true})),
        Err(error) => internal(error),
    }
}

pub async fn app_update() -> Response {
    no_store(json!({
        "currentVersion": env!("CARGO_PKG_VERSION"),
        "latestVersion": env!("CARGO_PKG_VERSION"),
        "updateAvailable": false,
        "releaseUrl": "",
    }))
}

pub async fn home() -> Response {
    let home = settings::home_dir().unwrap_or_default();
    no_store(json!({"home": home.to_string_lossy()}))
}

pub async fn default_cwd() -> Response {
    let dir = settings::default_cwd();
    match std::fs::create_dir_all(&dir) {
        Ok(()) => no_store(json!({"cwd": dir.to_string_lossy()})),
        Err(error) => internal(error),
    }
}

#[derive(Debug, Deserialize)]
pub struct CwdBody {
    cwd: Option<String>,
}

pub async fn validate_cwd(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CwdBody>,
) -> Response {
    let Some(cwd) = body.cwd.filter(|value| !value.trim().is_empty()) else {
        return bad_request("Path is required");
    };
    let cwd = normalize_cwd(&cwd);
    if !cwd.is_dir() {
        return bad_request(format!("Directory does not exist: {}", cwd.display()));
    }
    state.roots.add(&cwd);
    let project = ProjectInfo::for_cwd(&cwd);
    no_store(json!({
        "success": true,
        "cwd": cwd.to_string_lossy(),
        "projectRoot": project.project_root,
        "projectKey": project_key(Path::new(&project.project_root)),
    }))
}

fn normalize_cwd(cwd: &str) -> PathBuf {
    if cwd == "~" {
        return settings::home_dir().unwrap_or_default();
    }
    if let Some(rest) = cwd.strip_prefix("~/") {
        return settings::home_dir().unwrap_or_default().join(rest);
    }
    let path = PathBuf::from(cwd);
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct BrowseQuery {
    path: Option<String>,
}

pub async fn browse_cwd(Query(query): Query<BrowseQuery>) -> Response {
    let start = query
        .path
        .filter(|value| !value.trim().is_empty())
        .map(|value| normalize_cwd(&value))
        .or_else(settings::home_dir)
        .unwrap_or_default();
    let resolved = start.canonicalize().unwrap_or(start);
    if !resolved.is_dir() {
        return not_found("Directory does not exist");
    }
    let mut directories = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&resolved) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false)
                || (entry
                    .file_type()
                    .map(|kind| kind.is_symlink())
                    .unwrap_or(false)
                    && path.is_dir());
            if is_dir {
                directories.push(json!({
                    "name": entry.file_name().to_string_lossy(),
                    "path": path.to_string_lossy(),
                }));
            }
        }
    }
    directories.sort_by(|a, b| {
        a.get("name")
            .and_then(Value::as_str)
            .cmp(&b.get("name").and_then(Value::as_str))
    });
    no_store(json!({
        "path": resolved.to_string_lossy(),
        "parentPath": resolved.parent().map(|parent| parent.to_string_lossy().into_owned()),
        "directories": directories,
    }))
}

#[derive(Debug, Deserialize)]
pub struct CreateDirBody {
    path: String,
    name: String,
}

pub async fn create_cwd(Json(body): Json<CreateDirBody>) -> Response {
    let parent = normalize_cwd(&body.path);
    let name = body.name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return bad_request("Directory name must be a single folder name");
    }
    if !parent.is_dir() {
        return bad_request("Parent path is not a directory");
    }
    let target = parent.join(name);
    match std::fs::create_dir(&target) {
        Ok(()) => no_store(json!({"success": true, "path": target.to_string_lossy()})),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            crate::routes::json_error(
                StatusCode::CONFLICT,
                "A file or directory with this name already exists",
            )
        }
        Err(error) => internal(error),
    }
}

pub async fn open_in_explorer_status(headers: axum::http::HeaderMap) -> Response {
    let loopback = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|host| {
            host.starts_with("127.0.0.1")
                || host.starts_with("localhost")
                || host.starts_with("[::1]")
        });
    no_store(json!({
        "supported": loopback && matches!(std::env::consts::OS, "linux" | "macos" | "windows"),
        "reason": if loopback { Value::Null } else { Value::String("remote".into()) },
        "platform": std::env::consts::OS,
    }))
}

#[derive(Debug, Deserialize)]
pub struct OpenBody {
    cwd: String,
}

pub async fn open_in_explorer(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<OpenBody>,
) -> Response {
    let loopback = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|host| {
            host.starts_with("127.0.0.1")
                || host.starts_with("localhost")
                || host.starts_with("[::1]")
        });
    if !loopback {
        return forbidden("remote");
    }
    let cwd = PathBuf::from(body.cwd);
    if !state.roots.is_allowed_existing(&cwd) {
        return forbidden("access-denied");
    }
    if !cwd.is_dir() {
        return bad_request("not-a-directory");
    }
    #[cfg(target_os = "macos")]
    let (program, args) = ("open", vec![cwd.to_string_lossy().into_owned()]);
    #[cfg(target_os = "windows")]
    let (program, args) = ("explorer", vec![cwd.to_string_lossy().into_owned()]);
    #[cfg(all(unix, not(target_os = "macos")))]
    let (program, args) = ("xdg-open", vec![cwd.to_string_lossy().into_owned()]);
    match std::process::Command::new(program).args(args).spawn() {
        Ok(_) => no_store(json!({"opened": true})),
        Err(error) => internal(error),
    }
}

pub async fn _unused(_body: Json<Value>) -> Response {
    no_store(json!({}))
}

/// `GET /manifest.webmanifest`: pi-web generated this from `app/manifest.ts`;
/// the Rust server keeps the same JSON and reads `PI_WEB_APP_NAME` per request.
pub async fn manifest() -> Response {
    let name = std::env::var("PI_WEB_APP_NAME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Pi Web".to_string());
    no_store(json!({
        "id": "/",
        "name": name,
        "short_name": name,
        "description": "Local web interface for the pi coding agent",
        "start_url": "/",
        "scope": "/",
        "display": "standalone",
        "background_color": "#ffffff",
        "theme_color": "#1a1a1a",
        "categories": ["developer", "productivity"],
        "lang": "en",
        "icons": [
            {"src": "/icons/icon-192.png", "sizes": "192x192", "type": "image/png", "purpose": "any"},
            {"src": "/icons/icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any"}
        ]
    }))
}
