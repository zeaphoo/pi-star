use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::debug_streams::StreamKind;
use crate::pi_rpc::PiSpawnOptions;
use crate::routes::{bad_request, blocking, internal, no_store, not_found};
use crate::state::AppState;
use pi_star_core::agent::backend::BackendKind;

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
    /// `rpc` (default) or `quick`, persisted on the new session.
    backend: Option<String>,
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
    let backend = match body.backend.as_deref() {
        None | Some("") => BackendKind::Rpc,
        Some(value) => match BackendKind::parse(value) {
            Some(kind) => kind,
            None => return bad_request("backend must be rpc or quick"),
        },
    };
    state.roots.add(&cwd);

    // Write the choice before any backend process opens the file: pi ignores
    // the custom entry, and QuickPI's harness reads it when it lazily starts.
    let config = state.config.clone();
    let create_cwd = cwd.clone();
    let path = match blocking(move || {
        let path = pi_star_core::session::create_session_file(&config, &create_cwd)?;
        pi_star_core::agent::backend::set_session_backend(&path, backend)?;
        Ok::<_, anyhow::Error>(path)
    })
    .await
    {
        Ok(Ok(path)) => path,
        Ok(Err(error)) => return internal(error),
        Err(response) => return response,
    };

    let kind = body.kind.as_deref().unwrap_or("prompt");
    let command = if kind == "ensure_session" {
        Value::Null
    } else {
        let mut command = serde_json::Map::new();
        command.insert("type".into(), Value::String("prompt".into()));
        if let Some(message) = body.message.clone() {
            command.insert("message".into(), message);
        }
        if let Some(images) = body.images.clone() {
            command.insert("images".into(), images);
        }
        for (key, value) in body.rest.clone() {
            command.insert(key, value);
        }
        Value::Object(command)
    };

    match backend {
        BackendKind::Rpc => {
            let options = PiSpawnOptions {
                provider: body.provider.clone(),
                model_id: body.model_id.clone(),
                thinking_level: body.thinking_level.clone(),
                tool_names: body.tool_names.clone(),
                name: None,
            };
            let session = match state.pi_rpc.open_path(&path, options).await {
                Ok(session) => session,
                Err(error) => return internal(error),
            };
            let data = if command.is_null() {
                Value::Null
            } else {
                match session.send(&command).await {
                    Ok(data) => data,
                    Err(error) => return internal(error),
                }
            };
            let state_json = session.state_json();
            no_store(json!({
                "success": true,
                "sessionId": session.id(),
                "backend": backend.id(),
                "data": data,
                "model": state_json.get("model").cloned().unwrap_or(Value::Null),
                "thinkingLevel": state_json.get("thinkingLevel").cloned().unwrap_or(Value::Null),
            }))
        }
        BackendKind::Quick => {
            let state_for = state.clone();
            let open_path = path.clone();
            let runtime = match blocking(move || state_for.agents.open_path(&open_path)).await {
                Ok(Ok(runtime)) => runtime,
                Ok(Err(error)) => return internal(error),
                Err(response) => return response,
            };
            if let (Some(provider), Some(model_id)) =
                (body.provider.as_ref(), body.model_id.as_ref())
            {
                let _ = runtime
                    .send(&json!({
                        "type": "set_model",
                        "provider": provider,
                        "modelId": model_id,
                    }))
                    .await;
            }
            if let Some(level) = body.thinking_level.clone() {
                let _ = runtime
                    .send(&json!({"type": "set_thinking_level", "level": level}))
                    .await;
            }
            if let Some(tool_names) = body.tool_names.clone() {
                let _ = runtime
                    .send(&json!({"type": "set_tools", "toolNames": tool_names}))
                    .await;
            }
            let data = if command.is_null() {
                Value::Null
            } else {
                match runtime.send(&command).await {
                    Ok(data) => data,
                    Err(error) => return internal(error),
                }
            };
            let runtime_for_state = runtime.clone();
            let state_json = match blocking(move || runtime_for_state.state_json(false)).await {
                Ok(state) => state,
                Err(response) => return response,
            };
            no_store(json!({
                "success": true,
                "sessionId": runtime.id,
                "backend": backend.id(),
                "data": data,
                "model": state_json.get("model").cloned().unwrap_or(Value::Null),
                "thinkingLevel": state_json.get("thinkingLevel").cloned().unwrap_or(Value::Null),
            }))
        }
    }
}

fn with_backend(mut value: Value, backend: BackendKind) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("backend".into(), Value::String(backend.id().into()));
    }
    value
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
    let backend = state.route_backend_async(&id).await;
    if let Some(session) = state.pi_rpc.get(&id) {
        // pi's RPC get_state has no context usage; the host extension reads it
        // from the live session, so status polls carry the current percentage.
        session.refresh_context_usage().await;
        return no_store(json!({
            "running": true,
            "backend": backend.id(),
            "state": with_backend(session.state_json(), backend),
        }));
    }
    if let Some(runtime) = state.agents.get(&id) {
        let runtime_for_state = runtime.clone();
        let include_prompt = query.system_prompt.as_deref() != Some("0");
        let state_json = match blocking(move || runtime_for_state.state_json(include_prompt)).await
        {
            Ok(state) => state,
            Err(response) => return response,
        };
        return no_store(json!({
            "running": true,
            "backend": backend.id(),
            "state": with_backend(state_json, backend),
        }));
    }
    no_store(json!({ "running": false, "backend": backend.id() }))
}

pub async fn command(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    run_command(state, id, body).await
}

/// The command dispatcher shared by `POST /api/agent/{id}` and the stream
/// bus's `cmd` frame. Returns the same JSON shape either way.
pub(crate) async fn run_command(state: Arc<AppState>, id: String, body: Value) -> Response {
    if body.get("type").and_then(Value::as_str) == Some("set_backend") {
        let Some(requested) = body.get("backend").and_then(Value::as_str) else {
            return bad_request("backend is required");
        };
        let Some(kind) = BackendKind::parse(requested) else {
            return bad_request("backend must be rpc or quick");
        };
        let state_for_backend = state.clone();
        let id_for_backend = id.clone();
        return match blocking(move || state_for_backend.set_backend(&id_for_backend, kind)).await {
            Ok(Ok(kind)) => no_store(json!({
                "success": true,
                "data": { "backend": kind.id() },
            })),
            Ok(Err(error)) => {
                crate::routes::json_error(axum::http::StatusCode::CONFLICT, error.to_string())
            }
            Err(response) => response,
        };
    }

    let backend = state.route_backend_async(&id).await;
    match backend {
        BackendKind::Rpc => {
            let session = match state.pi_rpc.open_or_create(&id).await {
                Ok(session) => session,
                Err(_) => return not_found("Session not found"),
            };
            match session.send(&body).await {
                Ok(data) => no_store(json!({
                    "success": true,
                    "data": backend_command_data(data, &body, backend),
                })),
                Err(error) => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({ "error": error })),
                )
                    .into_response(),
            }
        }
        BackendKind::Quick => {
            // QuickPI builds its tool registry when the harness opens, so a
            // settings change (the subagent switch included) only takes effect
            // after the runtime is dropped and reopened.
            if body.get("type").and_then(Value::as_str) == Some("reload") {
                state.agents.remove(&id);
            }
            let runtime = match state.agents.get(&id) {
                Some(runtime) => Ok(runtime),
                None => {
                    let state_for = state.clone();
                    let open_id = id.clone();
                    match blocking(move || state_for.agents.open_or_create(&open_id)).await {
                        Ok(runtime) => runtime,
                        Err(response) => return response,
                    }
                }
            };
            let runtime = match runtime {
                Ok(runtime) => runtime,
                Err(_) => return not_found("Session not found"),
            };
            match runtime.send(&body).await {
                Ok(data) => no_store(json!({
                    "success": true,
                    "data": backend_command_data(data, &body, backend),
                })),
                Err(error) => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({ "error": error.to_string() })),
                )
                    .into_response(),
            }
        }
    }
}

fn backend_command_data(data: Value, command: &Value, backend: BackendKind) -> Value {
    if command.get("type").and_then(Value::as_str) == Some("get_state") {
        with_backend(data, backend)
    } else {
        data
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct StreamQuery {
    holder: Option<String>,
}

pub async fn events(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Response {
    if state.route_backend_async(&id).await == BackendKind::Rpc {
        let session = match state.pi_rpc.open_or_create(&id).await {
            Ok(session) => session,
            Err(_) => return not_found("Session not found"),
        };
        return pi_events(&state, session, query.holder, headers);
    }
    let runtime = match state.agents.get(&id) {
        Some(runtime) => Ok(runtime),
        None => {
            let state_for = state.clone();
            let open_id = id.clone();
            match blocking(move || state_for.agents.open_or_create(&open_id)).await {
                Ok(runtime) => runtime,
                Err(response) => return response,
            }
        }
    };
    let runtime = match runtime {
        Ok(runtime) => runtime,
        Err(_) => return not_found("Session not found"),
    };
    let mut receiver = match runtime.subscribe().await {
        Ok(receiver) => receiver,
        Err(error) => return internal(error),
    };
    let runtime_for_state = runtime.clone();
    let streaming = match blocking(move || runtime_for_state.state_json(false)).await {
        Ok(state) => state,
        Err(response) => return response,
    }
    .get("isStreaming")
    .and_then(Value::as_bool)
    .unwrap_or(false);
    let pending_ui = runtime.pending_extension_ui_ids();
    let stream_handle =
        state
            .debug_streams
            .register(StreamKind::Agent, Some(id.clone()), query.holder, &headers);
    let stream = async_stream::stream! {
        stream_handle.write();
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
                                stream_handle.write();
                                yield Ok(axum::response::sse::Event::default().data(event.to_string()));
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            stream_handle.write();
                            continue;
                        }
                        Err(_) => break,
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    stream_handle.write();
                    yield Ok(axum::response::sse::Event::default().data("{\"type\":\"heartbeat\"}"));
                }
            }
        }
    };
    axum::response::sse::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

fn pi_events(
    state: &Arc<AppState>,
    session: Arc<crate::pi_rpc::PiRpcSession>,
    holder: Option<String>,
    headers: HeaderMap,
) -> Response {
    let mut receiver = session.subscribe();
    let state_json = session.state_json();
    let streaming = state_json
        .get("isStreaming")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let pending_ui = session.pending_extension_ui_ids();
    let stream_handle =
        state
            .debug_streams
            .register(StreamKind::Agent, Some(session.id()), holder, &headers);
    let stream = async_stream::stream! {
        stream_handle.write();
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
                            if let Some(event) = filter_event(event) {
                                stream_handle.write();
                                yield Ok(axum::response::sse::Event::default().data(event.to_string()));
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            stream_handle.write();
                            continue;
                        }
                        Err(_) => break,
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    stream_handle.write();
                    yield Ok(axum::response::sse::Event::default().data("{\"type\":\"heartbeat\"}"));
                }
            }
        }
    };
    axum::response::sse::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

pub(crate) fn filter_event(event: Value) -> Option<Value> {
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

/// Whether one of this session's subagents is still live (QuickPI only; the RPC
/// extension tracks its own children in the browser's family rows).
fn subagents_active_for(state: &AppState, parent_id: &str) -> bool {
    state
        .agents
        .subagent_controller()
        .map(|controller| {
            controller.live_ids().into_iter().any(|id| {
                controller
                    .get(&id)
                    .is_some_and(|info| info.parent_session_id == parent_id)
            })
        })
        .unwrap_or(false)
}

pub async fn running(State(state): State<Arc<AppState>>) -> Response {
    let mut running = state.agents.running_ids();
    let mut awaiting = state.agents.awaiting_input_ids();
    running.extend(state.pi_rpc.running_ids());
    awaiting.extend(state.pi_rpc.awaiting_input_ids());
    // QuickPI subagents run as ordinary sessions, but their parent's row shows a
    // family "working" spinner only while one of them is live.
    let mut subagents_active: Vec<String> = Vec::new();
    let mut running_set: std::collections::HashSet<String> = running.iter().cloned().collect();
    if let Some(controller) = state.agents.subagent_controller() {
        for id in controller.live_ids() {
            if running_set.insert(id.clone()) {
                running.push(id.clone());
            }
            if let Some(info) = controller.get(&id)
                && !info.parent_session_id.is_empty()
                && !subagents_active.contains(&info.parent_session_id)
            {
                subagents_active.push(info.parent_session_id);
            }
        }
    }
    no_store(json!({
        "sessionListVersion": state.started.elapsed().as_millis() as u64,
        "runningSessionIds": running,
        "awaitingInputSessionIds": awaiting,
        "completionNotificationSuppressedSessionIds": [],
        "subagentsActiveSessionIds": subagents_active,
        "sessionUiStateRevision": state.ui_state.revision(),
    }))
}

pub async fn lease(State(state): State<Arc<AppState>>, AxumPath(id): AxumPath<String>) -> Response {
    let backend = state.route_backend_async(&id).await;
    if let Some(session) = state.pi_rpc.get(&id) {
        session.touch();
        return no_store(json!({
            "running": session.is_running(),
            "awaitingInput": !session.pending_extension_ui_ids().is_empty(),
            "subagentsActive": false,
            "backend": backend.id(),
            "state": with_backend(session.state_json(), backend),
        }));
    }
    if let Some(runtime) = state.agents.get(&id) {
        runtime.touch();
        let runtime_for_state = runtime.clone();
        let state_json = match blocking(move || runtime_for_state.state_json(false)).await {
            Ok(state) => state,
            Err(response) => return response,
        };
        return no_store(json!({
            "running": runtime.is_running(),
            "awaitingInput": false,
            "subagentsActive": subagents_active_for(&state, &id),
            "backend": backend.id(),
            "state": with_backend(state_json, backend),
        }));
    }
    no_store(json!({
        "running": false,
        "awaitingInput": false,
        "subagentsActive": false,
        "backend": backend.id(),
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct DebugQuery {
    session: Option<String>,
}

fn session_summary(state: &AppState, id: &str) -> Value {
    let runtime = state.agents.get(id);
    let streams = state.debug_streams.session_agent_streams(id);
    let running = runtime.as_ref().is_some_and(|runtime| runtime.is_running());
    json!({
        "sessionId": id,
        "state": if runtime.is_some() { "alive" } else { "none" },
        "running": running,
        "awaitingInput": false,
        "subagent": false,
        "eventStreams": streams.len() + state.hub.agent_subscribers(id),
        "providers": 0,
        "nextIdleCheckAt": Value::Null,
        "forcedByStop": false,
        "starts": if runtime.is_some() { 1 } else { 0 },
    })
}

/// Merges the SSE registry's pages with the bus's: one row per holder, each
/// kind's count added, the first address and client label kept.
fn merge_stream_pages(sse: Vec<Value>, bus: Vec<Value>) -> Vec<Value> {
    let mut rows: BTreeMap<String, Value> = BTreeMap::new();
    for page in sse.into_iter().chain(bus) {
        let key = page
            .get("holder")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        match rows.entry(key) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(page);
            }
            std::collections::btree_map::Entry::Occupied(mut slot) => {
                let target = slot.get_mut();
                for field in ["agent", "terminal", "fileWatch", "auth"] {
                    let sum = target.get(field).and_then(Value::as_u64).unwrap_or(0)
                        + page.get(field).and_then(Value::as_u64).unwrap_or(0);
                    if sum > 0 || field != "auth" {
                        target[field] = Value::from(sum);
                    }
                }
                for field in ["address", "client"] {
                    let missing = target.get(field).map(Value::is_null).unwrap_or(true);
                    if missing {
                        if let Some(value) = page.get(field).filter(|value| !value.is_null()) {
                            target[field] = value.clone();
                        }
                    }
                }
            }
        }
    }
    rows.into_values().collect()
}

fn wrapper_json(
    state: &AppState,
    id: &str,
    runtime: &pi_star_core::agent::SessionRuntime,
) -> Value {
    let snapshot = runtime.debug_snapshot();
    let state_json = runtime.state_json(false);
    let queued = state_json
        .pointer("/queuedMessages/steering")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
        + state_json
            .pointer("/queuedMessages/followUp")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
    let running = runtime.is_running();
    let uptime = snapshot
        .get("uptimeMs")
        .and_then(Value::as_u64)
        .unwrap_or(0) as i64;
    let started_at = crate::debug_streams::now_ms() - uptime;
    json!({
        "sessionId": id,
        "state": "alive",
        "stale": false,
        "subagent": false,
        "chatOnly": false,
        "createdAt": started_at,
        "startedAt": started_at,
        "closingAt": Value::Null,
        "extensions": "none",
        "run": {
            "running": running,
            "streaming": state_json.get("isStreaming").and_then(Value::as_bool).unwrap_or(false),
            "promptRunning": state_json.get("isPromptRunning").and_then(Value::as_bool).unwrap_or(false),
            "pendingPrompts": 0,
            "bashRunning": state_json.get("isBashRunning").and_then(Value::as_bool).unwrap_or(false),
            "compacting": state_json.get("isCompacting").and_then(Value::as_bool).unwrap_or(false),
            "retrying": false,
            "queuedMessages": queued,
            "waitingForMcp": false,
            "runningTools": 0,
            "mutatingCommands": 0,
            "replacement": Value::Null,
        },
        "idle": {
            "timeoutMs": state.idle_timeout_ms,
            "nextCheckAt": Value::Null,
            "lastResetAt": Value::Null,
            "forcedByStop": false,
        },
        "awaitingInput": false,
        "pendingUiRequests": runtime.pending_extension_ui_ids().len(),
        "customUis": 0,
        "eventListeners": state.debug_streams.session_agent_streams(id).len(),
        "lastEvent": Value::Null,
        "lastCommand": Value::Null,
    })
}

pub async fn debug(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DebugQuery>,
) -> Response {
    let state_for = state.clone();
    match blocking(move || {
        let state = &state_for;
        let mut ids = state.agents.ids();
        ids.extend(state.debug_streams.session_ids_with_streams());
        ids.extend(state.hub.agent_session_ids());
        ids.sort();
        ids.dedup();
        let sessions: Vec<Value> = ids.iter().map(|id| session_summary(&state, id)).collect();

        let session = query.session.as_ref().map(|id| {
            let runtime = state.agents.get(id);
            let mut streams: Vec<Value> = state
                .debug_streams
                .session_agent_streams(id)
                .iter()
                .map(|stream| stream.to_client_json())
                .collect();
            streams.extend(state.hub.agent_streams(id));
            json!({
                "sessionId": id,
                "wrapper": runtime
                    .as_ref()
                    .map(|runtime| wrapper_json(&state, id, runtime))
                    .unwrap_or(Value::Null),
                "lifecycle": Value::Null,
                "starting": false,
                "providers": [],
                "eventStreams": streams,
                "subagents": { "active": false, "runs": [], "notifications": [] },
                "mcp": Value::Null,
            })
        });

        let agent_streams = state.debug_streams.count(StreamKind::Agent);
        let terminal_streams = state.debug_streams.count(StreamKind::Terminal);
        let file_watches = state.debug_streams.count(StreamKind::FileWatch);
        let bus = state.hub.stream_counts();
        let sse_total = agent_streams + terminal_streams + file_watches;
        let bus_total = bus.agent + bus.terminal + bus.file;
        let terminals = state.terminals.count();
        no_store(json!({
            "now": crate::debug_streams::now_ms(),
            "server": {
                "startedAt": state.started_epoch_ms,
                "idleTimeoutMs": state.idle_timeout_ms,
                "heartbeatMs": 30_000,
                "wrappers": {
                    "alive": state.agents.ids().len(),
                    "closing": 0,
                    "starting": 0,
                },
                "eventStreams": { "total": agent_streams + bus.agent },
                "sseTotal": sse_total,
                "busTotal": bus_total,
                "streamTotal": sse_total + bus_total,
                "pages": merge_stream_pages(
                    state.debug_streams.pages(),
                    state.hub.pages(),
                ),
                "fileWatches": file_watches,
                "terminals": {
                    "count": terminals,
                    "listeners": terminal_streams,
                    "detached": 0,
                    "exited": state.terminals.exited_count(),
                    "nextCleanupAt": Value::Null,
                },
            },
            "hub": state.hub.snapshot(),
            "sessions": sessions,
            "session": session,
            "piRpc": state.pi_rpc.ids()
                .into_iter()
                .filter_map(|id| state.pi_rpc.get(&id))
                .map(|session| session.debug_snapshot())
                .collect::<Vec<_>>(),
        }))
    })
    .await
    {
        Ok(response) => response,
        Err(response) => response,
    }
}

const MAX_INLINE_BASH_OUTPUT_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Deserialize, Default)]
pub struct BashOutputQuery {
    path: Option<String>,
    download: Option<String>,
}

fn bash_output_name_valid(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("pi-bash-") else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".log") else {
        return false;
    };
    !stem.is_empty()
        && stem
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

/// `GET /api/agent/{id}/bash-output?path=...`: a bash output file under the
/// temp directory, and only when this session's transcript names it.
pub async fn bash_output(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<BashOutputQuery>,
) -> Response {
    use axum::http::{StatusCode, header};

    let Some(path) = query.path.filter(|value| !value.is_empty()) else {
        return bad_request("path required");
    };
    let requested = PathBuf::from(path);
    let Some(name) = requested.file_name().and_then(|name| name.to_str()) else {
        return bad_request("invalid path");
    };
    if !bash_output_name_valid(name) {
        return bad_request("invalid path");
    }
    let temp_root = std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir());
    let Ok(resolved) = requested.canonicalize() else {
        return not_found("full output unavailable");
    };
    if resolved.parent() != Some(temp_root.as_path()) {
        return bad_request("invalid path");
    }
    if !resolved.is_file() {
        return not_found("full output unavailable");
    }
    let config = state.config.clone();
    let session_id = id.clone();
    let resolved_for_check = resolved.clone();
    let referenced = match blocking(move || -> Option<bool> {
        let session_path = pi_star_core::session::resolve_session_path(&config, &session_id)?;
        let session = pi_star_core::session::read_session(&session_path).ok()?;
        Some(session.entries.iter().any(|entry| {
            pi_star_core::session::entry_type(entry) == Some("message")
                && entry.pointer("/message/role").and_then(Value::as_str) == Some("bashExecution")
                && entry
                    .pointer("/message/fullOutputPath")
                    .and_then(Value::as_str)
                    .is_some_and(|value| {
                        pi_star_core::project::same_path(Path::new(value), &resolved_for_check)
                    })
        }))
    })
    .await
    {
        Ok(referenced) => referenced,
        Err(response) => return response,
    };
    let Some(referenced) = referenced else {
        return not_found("Session not found");
    };
    if !referenced {
        return crate::routes::json_error(StatusCode::FORBIDDEN, "forbidden");
    }
    let meta_path = resolved.clone();
    let metadata = match blocking(move || std::fs::metadata(&meta_path)).await {
        Ok(Ok(metadata)) => metadata,
        Ok(Err(_)) => return not_found("full output unavailable"),
        Err(response) => return response,
    };
    if query.download.as_deref() == Some("1") {
        let read_path = resolved.clone();
        let bytes = match blocking(move || std::fs::read(&read_path)).await {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(_)) => return not_found("full output unavailable"),
            Err(response) => return response,
        };
        return (
            StatusCode::OK,
            [
                (
                    header::CONTENT_TYPE,
                    "text/plain; charset=utf-8".to_string(),
                ),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"bash-output.log\"".to_string(),
                ),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
            bytes,
        )
            .into_response();
    }
    if metadata.len() > MAX_INLINE_BASH_OUTPUT_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            axum::Json(json!({
                "error": format!(
                    "Full output is too large to display (limit {MAX_INLINE_BASH_OUTPUT_BYTES} bytes)"
                ),
                "data": {"size": metadata.len(), "maxBytes": MAX_INLINE_BASH_OUTPUT_BYTES},
            })),
        )
            .into_response();
    }
    let read_path = resolved.clone();
    let bytes = match blocking(move || std::fs::read(&read_path)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => return not_found("full output unavailable"),
        Err(response) => return response,
    };
    no_store(json!({
        "success": true,
        "data": {"output": String::from_utf8_lossy(&bytes)},
    }))
}
