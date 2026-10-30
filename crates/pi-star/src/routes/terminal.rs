use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use pi_star_core::terminal::TerminalEvent;

use crate::routes::{bad_request, forbidden, internal, no_store, not_found};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateTerminalBody {
    id: Option<String>,
    cwd: String,
    cols: Option<u16>,
    rows: Option<u16>,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateTerminalBody>,
) -> Response {
    if let Some(id) = &body.id {
        if id.len() != 32 || !id.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return bad_request("Invalid terminal id");
        }
    }
    let cwd = PathBuf::from(&body.cwd);
    if !cwd.is_dir() {
        return bad_request("cwd must be a directory");
    }
    if !state.roots.is_allowed_existing(&cwd) {
        return forbidden("Access denied");
    }
    match state.terminals.create(
        &cwd,
        body.cols.unwrap_or(80),
        body.rows.unwrap_or(24),
        body.id,
    ) {
        Ok(id) => no_store(json!({ "id": id })),
        Err(error) => internal(error),
    }
}

pub async fn get_terminal(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match state.terminals.cwd(&id) {
        Some(cwd) => no_store(json!({ "id": id, "cwd": cwd })),
        None => not_found("Terminal expired or closed"),
    }
}

#[derive(Debug, Deserialize)]
pub struct TerminalCommand {
    #[serde(rename = "type")]
    kind: String,
    data: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
}

pub async fn command(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<TerminalCommand>,
) -> Response {
    match body.kind.as_str() {
        "input" => {
            let Some(data) = body.data else {
                return bad_request("Invalid terminal command");
            };
            if data.len() > 64 * 1024 {
                return bad_request("Invalid terminal command");
            }
            if state.terminals.write(&id, &data) {
                no_store(json!({ "success": true }))
            } else {
                not_found("Terminal not found")
            }
        }
        "resize" => {
            let cols = body.cols.unwrap_or(80).clamp(2, 1000);
            let rows = body.rows.unwrap_or(24).clamp(2, 1000);
            if state.terminals.resize(&id, cols, rows) {
                no_store(json!({ "success": true }))
            } else {
                not_found("Terminal not found")
            }
        }
        _ => bad_request("Invalid terminal command"),
    }
}

pub async fn kill(State(state): State<Arc<AppState>>, AxumPath(id): AxumPath<String>) -> Response {
    state.terminals.kill(&id);
    no_store(json!({ "success": true }))
}

pub async fn events(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let Some(mut subscription) = state.terminals.subscribe(&id) else {
        return not_found("Terminal not found");
    };
    let stream = async_stream::stream! {
        yield Ok::<_, std::convert::Infallible>(
            axum::response::sse::Event::default().comment("")
        );
        yield Ok(axum::response::sse::Event::default().data(serde_json::to_string(&subscription.output).unwrap_or_default()));
        if subscription.exited {
            let event = TerminalEvent::Exit { exit_code: subscription.exit_code.unwrap_or(0) };
            yield Ok(axum::response::sse::Event::default().data(serde_json::to_string(&event).unwrap_or_default()));
        }
        loop {
            tokio::select! {
                received = subscription.receiver.recv() => {
                    match received {
                        Ok(event) => {
                            let terminal = matches!(event, TerminalEvent::Exit { .. } | TerminalEvent::Closed);
                            yield Ok(axum::response::sse::Event::default().data(serde_json::to_string(&event).unwrap_or_default()));
                            if terminal {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    yield Ok(axum::response::sse::Event::default().comment(""));
                }
            }
        }
    };
    axum::response::sse::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}
