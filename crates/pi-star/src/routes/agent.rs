use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::routes::{bad_request, internal, no_store, not_found};
use crate::state::AppState;

#[derive(Debug, Deserialize, Default)]
pub struct NewSessionBody {
    cwd: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    message: Option<Value>,
    images: Option<Value>,
    provider: Option<String>,
    #[serde(rename = "modelId")]
    model_id: Option<String>,
    #[serde(rename = "thinkingLevel")]
    thinking_level: Option<String>,
    #[serde(rename = "toolNames")]
    tool_names: Option<Vec<String>>,
    #[serde(flatten)]
    rest: serde_json::Map<String, Value>,
}

pub async fn new_session(
    State(state): State<Arc<AppState>>,
    Json(body): Json<NewSessionBody>,
) -> Response {
    let Some(cwd) = body.cwd else {
        return bad_request("cwd is required");
    };
    let cwd = PathBuf::from(cwd);
    if !cwd.is_dir() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(json!({
                "error": format!("Directory not found: {}", cwd.display()),
                "reason": "cwd-missing",
                "missingCwd": cwd.to_string_lossy(),
            })),
        )
            .into_response();
    }
    state.roots.add(&cwd);
    let runtime = match state.agents.create(&cwd) {
        Ok(runtime) => runtime,
        Err(error) => return internal(error),
    };
    if let (Some(provider), Some(model_id)) = (body.provider.as_ref(), body.model_id.as_ref()) {
        let _ = runtime.send(&json!({
            "type": "set_model",
            "provider": provider,
            "modelId": model_id,
        }));
    }
    if let Some(level) = body.thinking_level {
        let _ = runtime.send(&json!({"type": "set_thinking_level", "level": level}));
    }
    if let Some(tool_names) = body.tool_names {
        let _ = runtime.send(&json!({"type": "set_tools", "toolNames": tool_names}));
    }
    let kind = body.kind.as_deref().unwrap_or("prompt");
    let data = if kind == "ensure_session" {
        Value::Null
    } else {
        let mut command = serde_json::Map::new();
        command.insert("type".into(), Value::String("prompt".into()));
        if let Some(message) = body.message {
            command.insert("message".into(), message);
        }
        if let Some(images) = body.images {
            command.insert("images".into(), images);
        }
        for (key, value) in body.rest {
            command.insert(key, value);
        }
        match runtime.send(&Value::Object(command)) {
            Ok(data) => data,
            Err(error) => return internal(error),
        }
    };
    let state_json = runtime.state_json(false);
    no_store(json!({
        "success": true,
        "sessionId": runtime.id,
        "data": data,
        "model": state_json.get("model").cloned().unwrap_or(Value::Null),
        "thinkingLevel": state_json.get("thinkingLevel").cloned().unwrap_or(Value::Null),
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct StateQuery {
    #[serde(rename = "systemPrompt")]
    system_prompt: Option<String>,
}

pub async fn get_state(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<StateQuery>,
) -> Response {
    if let Some(runtime) = state.agents.get(&id) {
        return no_store(json!({
            "running": true,
            "state": runtime.state_json(query.system_prompt.as_deref() != Some("0")),
        }));
    }
    no_store(json!({ "running": false }))
}

pub async fn command(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let runtime = state
        .agents
        .get(&id)
        .map(Ok)
        .unwrap_or_else(|| state.agents.open_or_create(&id));
    let runtime = match runtime {
        Ok(runtime) => runtime,
        Err(_) => return not_found("Session not found"),
    };
    match runtime.send(&body) {
        Ok(data) => no_store(json!({ "success": true, "data": data })),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn events(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let runtime = state
        .agents
        .get(&id)
        .map(Ok)
        .unwrap_or_else(|| state.agents.open_or_create(&id));
    let runtime = match runtime {
        Ok(runtime) => runtime,
        Err(_) => return not_found("Session not found"),
    };
    let mut receiver = runtime.subscribe();
    let streaming = runtime
        .state_json(false)
        .get("isStreaming")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let pending_ui = runtime.pending_extension_ui_ids();
    let stream = async_stream::stream! {
        yield Ok::<_, std::convert::Infallible>(
            axum::response::sse::Event::default().data(json!({
                "type": "connected",
                "isStreaming": streaming,
                "pendingExtensionUiIds": pending_ui,
            }).to_string())
        );
        loop {
            tokio::select! {
                received = receiver.recv() => {
                    match received {
                        Ok(event) => {
                            let event = filter_event(event);
                            if let Some(event) = event {
                                yield Ok(axum::response::sse::Event::default().data(event.to_string()));
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    yield Ok(axum::response::sse::Event::default().data("{\"type\":\"heartbeat\"}"));
                }
            }
        }
    };
    axum::response::sse::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

fn filter_event(event: Value) -> Option<Value> {
    let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
    if matches!(kind, "turn_start" | "turn_end" | "entry_appended") {
        return None;
    }
    if kind == "message_start" || kind == "message_end" {
        if event.pointer("/message/role").and_then(Value::as_str) == Some("system") {
            return None;
        }
        if kind == "message_start" {
            let role = event
                .pointer("/message/role")
                .and_then(Value::as_str)
                .unwrap_or("");
            if role != "assistant" {
                return Some(json!({"type":"message_start","message":{"role":role}}));
            }
        }
    }
    if kind == "agent_end" {
        return Some(json!({"type":"agent_end"}));
    }
    Some(event)
}

pub async fn running(State(state): State<Arc<AppState>>) -> Response {
    no_store(json!({
        "sessionListVersion": state.started.elapsed().as_millis() as u64,
        "runningSessionIds": state.agents.running_ids(),
        "awaitingInputSessionIds": state.agents.awaiting_input_ids(),
        "completionNotificationSuppressedSessionIds": [],
        "subagentsActiveSessionIds": [],
        "sessionUiStateRevision": state.ui_state.revision(),
    }))
}

pub async fn lease(State(state): State<Arc<AppState>>, AxumPath(id): AxumPath<String>) -> Response {
    if let Some(runtime) = state.agents.get(&id) {
        runtime.touch();
        let state = runtime.state_json(false);
        return no_store(json!({
            "running": runtime.is_running(),
            "awaitingInput": false,
            "subagentsActive": false,
            "state": state,
        }));
    }
    no_store(json!({
        "running": false,
        "awaitingInput": false,
        "subagentsActive": false,
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct DebugQuery {
    session: Option<String>,
}

pub async fn debug(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DebugQuery>,
) -> Response {
    let runtimes = if let Some(id) = query.session {
        state
            .agents
            .get(&id)
            .map(|runtime| vec![runtime.debug_snapshot()])
            .unwrap_or_default()
    } else {
        let ids = state.agents.running_ids();
        ids.into_iter()
            .filter_map(|id| {
                state
                    .agents
                    .get(&id)
                    .map(|runtime| runtime.debug_snapshot())
            })
            .collect()
    };
    no_store(json!({
        "now": chrono::Utc::now().to_rfc3339(),
        "protocol": 1,
        "sessions": runtimes,
        "streams": [],
        "subagents": [],
        "terminals": { "count": state.terminals.count() },
        "mcp": [],
        "fileWatches": 0,
        "serverUptimeMs": state.started.elapsed().as_millis() as u64,
    }))
}
