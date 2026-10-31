use std::path::Path;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};

use pi_star_core::models;
use pi_star_core::session::{
    self, append_lines, append_session_info, build_context, build_tree, compute_stats,
    create_session_file, entry_id, entry_type, message_text, read_session, slice_active_branch,
    total_active_ms,
};

use pi_star_core::agent::backend::BackendKind;

use crate::routes::{bad_request, internal, json_error, no_store, not_found};
use crate::state::AppState;

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    #[serde(rename = "force")]
    _force: Option<String>,
    summary: Option<String>,
}

pub async fn list(State(state): State<Arc<AppState>>, Query(query): Query<ListQuery>) -> Response {
    let summary = query.summary.as_deref() == Some("1");
    let sessions = state.session_index.scan(&state.config, summary);
    let version = sessions
        .iter()
        .map(|session| session.modified.clone())
        .max()
        .unwrap_or_default();
    let mut running = state.agents.running_ids();
    let mut awaiting = state.agents.awaiting_input_ids();
    running.extend(state.pi_rpc.running_ids());
    awaiting.extend(state.pi_rpc.awaiting_input_ids());
    no_store(json!({
        "sessions": sessions,
        "sessionListVersion": version,
        "runningSessionIds": running,
        "awaitingInputSessionIds": awaiting,
        "completionNotificationSuppressedSessionIds": [],
        "sessionUiStateRevision": state.ui_state.revision(),
        "subagentsActiveSessionIds": [],
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct SessionQuery {
    #[serde(rename = "force")]
    force: Option<String>,
    tail: Option<usize>,
    before: Option<String>,
    tree: Option<String>,
    #[serde(rename = "deferThinking")]
    defer_thinking: Option<String>,
    #[serde(rename = "deferMedia")]
    defer_media: Option<String>,
}

pub async fn get_session(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<SessionQuery>,
) -> Response {
    let path = match session::resolve_session_path(&state.config, &id) {
        Some(path) => path,
        None => return not_found("Session not found"),
    };
    let session = match read_session(&path) {
        Ok(session) => session,
        Err(error) => return internal(error),
    };
    // A live wrapper serves its own snapshot: two processes writing one JSONL
    // is unsupported. A mount/refresh read (`?force=1`) adopts the file and
    // says it rebuilt, after which ordinary reads are stable again (#632).
    let runtime = state.agents.get(&id);
    let force = query.force.as_deref() == Some("1");
    let wrapper_rebuilt = if let Some(runtime) = &runtime {
        if force {
            runtime.replace_entries(session.entries.clone());
            Some(true)
        } else {
            None
        }
    } else {
        None
    };
    let entries = match (&runtime, force) {
        (Some(runtime), false) => runtime.entries_snapshot(),
        _ => session.entries.clone(),
    };
    let info = session.info();
    state.roots.add(&info.cwd);
    if let Some(root) = &info.project_root {
        state.roots.add(root);
    }
    let leaf_id = session.leaf_id();
    let tail = query
        .tail
        .unwrap_or(session::DEFAULT_TAIL)
        .clamp(1, session::MAX_TAIL);
    let context = build_context(
        &id,
        &entries,
        leaf_id.as_deref(),
        tail,
        query.before.as_deref(),
        query
            .defer_thinking
            .as_deref()
            .is_some_and(|value| value != "0"),
        query
            .defer_media
            .as_deref()
            .is_some_and(|value| value != "0"),
    );
    let tree = build_tree(&entries);
    let stats = compute_stats(&entries);
    let tool_names = entries
        .iter()
        .find(|entry| {
            entry_type(entry) == Some("custom")
                && entry.get("customType").and_then(Value::as_str) == Some("pi-web:tool-selection")
        })
        .and_then(|entry| entry.pointer("/data/tools").cloned());
    let latest = entries.last().and_then(entry_id).unwrap_or_default();
    let revision = format!("{}:{}:{}", info.modified, session.entries.len(), latest);
    let mut response = json!({
        "sessionId": id,
        "filePath": path.to_string_lossy(),
        "info": info,
        "leafId": leaf_id,
        "tree": tree,
        "snapshotRevision": revision,
        "context": context,
        "stats": stats,
        "totalActiveMs": total_active_ms(&entries),
    });
    if let Some(rebuilt) = wrapper_rebuilt {
        response["wrapperRebuilt"] = Value::Bool(rebuilt);
    }
    if query.tree.as_deref() == Some("summary") {
        response["treeFormat"] = Value::String("summary".into());
    }
    if let Some(tool_names) = tool_names {
        response["toolNames"] = tool_names;
    }
    no_store(response)
}

#[derive(Debug, Deserialize)]
pub struct RenameBody {
    name: String,
}

pub async fn rename(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RenameBody>,
) -> Response {
    if state.route_backend(&id) == BackendKind::Rpc {
        if let Some(active) = state.pi_rpc.get(&id) {
            return match active
                .send(&json!({"type": "set_session_name", "name": body.name.trim()}))
                .await
            {
                Ok(_) => no_store(json!({ "ok": true })),
                Err(error) => internal(error),
            };
        }
    }
    let Some(path) = session::resolve_session_path(&state.config, &id) else {
        return not_found("Session not found");
    };
    match append_session_info(&path, body.name.trim()) {
        Ok(()) => no_store(json!({ "ok": true })),
        Err(error) => internal(error),
    }
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let Some(path) = session::resolve_session_path(&state.config, &id) else {
        return not_found("Session not found");
    };
    state.agents.remove(&id);
    state.pi_rpc.remove(&id);
    let deleted = delete_session_file(&state, &path);
    no_store(json!({ "ok": true, "deleted": deleted }))
}

#[derive(Debug, Deserialize)]
pub struct DeleteManyBody {
    ids: Vec<String>,
}

pub async fn delete_many(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DeleteManyBody>,
) -> Response {
    let mut deleted = Vec::new();
    let mut missing = Vec::new();
    for id in body.ids {
        match session::resolve_session_path(&state.config, &id) {
            Some(path) => {
                state.agents.remove(&id);
                state.pi_rpc.remove(&id);
                delete_session_file(&state, &path);
                deleted.push(id);
            }
            None => missing.push(id),
        }
    }
    no_store(json!({ "deleted": deleted, "missing": missing }))
}

fn delete_session_file(state: &AppState, path: &Path) -> usize {
    let target_id = read_session(path).ok().map(|session| session.header.id);
    let mut deleted = 0usize;
    for candidate in state.session_index.scan(&state.config, false) {
        let parent_matches = candidate
            .parent_session_id
            .as_deref()
            .zip(target_id.as_deref())
            .is_some_and(|(parent, target)| parent == target);
        let subagent_matches = matches!(
            &candidate.relation,
            Some(session::SessionRelation::Subagent { parent_session_id, .. })
                if target_id.as_deref() == Some(parent_session_id.as_str())
        );
        if parent_matches || subagent_matches {
            let _ = std::fs::remove_file(&candidate.path);
            deleted += 1;
        }
    }
    if std::fs::remove_file(path).is_ok() {
        deleted += 1;
    }
    state.session_index.invalidate(path);
    deleted
}

#[derive(Debug, Deserialize)]
pub struct StatsBody {
    ids: Vec<String>,
}

pub async fn stats(State(state): State<Arc<AppState>>, Json(body): Json<StatsBody>) -> Response {
    let mut stats = serde_json::Map::new();
    for id in body.ids {
        if let Some(path) = session::resolve_session_path(&state.config, &id) {
            if let Ok(session) = read_session(&path) {
                let value = compute_stats(&session.entries);
                let (thinking_level, model) = session::session_settings(&session.entries, None);
                stats.insert(
                    id,
                    json!({
                        "tokens": value.tokens,
                        "cost": value.cost,
                        "toolCalls": value.tool_calls,
                        "totalActiveMs": total_active_ms(&session.entries),
                        "model": model,
                        "thinkingLevel": thinking_level,
                    }),
                );
            }
        }
    }
    no_store(json!({ "stats": stats }))
}

#[derive(Debug, Deserialize, Default)]
pub struct SearchQuery {
    q: Option<String>,
}

pub async fn search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Response {
    let query = query.q.unwrap_or_default().trim().to_string();
    if query.len() > 200 {
        return bad_request("Search query exceeds 200 characters");
    }
    if query.is_empty() {
        return no_store(json!({ "results": [], "truncated": false }));
    }
    let needle = query.to_lowercase();
    let mut results = Vec::new();
    let mut truncated = false;
    for info in state.session_index.scan(&state.config, false) {
        if results.len() >= 30 {
            truncated = true;
            break;
        }
        let Ok(session) = read_session(Path::new(&info.path)) else {
            continue;
        };
        let title_hit = info
            .name
            .as_deref()
            .is_some_and(|name| name.to_lowercase().contains(&needle))
            || info.first_message.to_lowercase().contains(&needle);
        if title_hit {
            results.push(json!({
                "session": info,
                "blockIndex": 0,
                "titleMatch": true,
                "before": "",
                "match": query,
                "after": "",
            }));
            continue;
        }
        for entry in &session.entries {
            let text = message_text(entry);
            let lower = text.to_lowercase();
            if let Some(index) = lower.find(&needle) {
                let start = text[..index]
                    .char_indices()
                    .rev()
                    .nth(60)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                let end = text[index + needle.len()..]
                    .char_indices()
                    .nth(60)
                    .map(|(i, _)| index + needle.len() + i)
                    .unwrap_or(text.len());
                results.push(json!({
                    "session": info,
                    "entryId": entry_id(entry),
                    "blockIndex": 0,
                    "before": &text[start..index],
                    "match": &text[index..index + needle.len()],
                    "after": &text[index + needle.len()..end],
                }));
                break;
            }
        }
    }
    no_store(json!({ "results": results, "truncated": truncated }))
}

pub async fn ui_state_get(State(state): State<Arc<AppState>>) -> Response {
    no_store(json!({ "state": state.ui_state.read() }))
}

pub async fn ui_state_post(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Response {
    match state.ui_state.apply(&body) {
        Ok(state) => no_store(json!({ "state": state })),
        Err(error) => json_error(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct ForkBody {
    #[serde(rename = "entryId")]
    entry_id: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    id: Option<String>,
}

pub async fn fork_body(State(state): State<Arc<AppState>>, Json(body): Json<ForkBody>) -> Response {
    let id = body.session_id.or(body.id).unwrap_or_default();
    fork_session_inner(&state, &id, body.entry_id.as_deref()).await
}

pub async fn fork_session(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<ForkBody>,
) -> Response {
    fork_session_inner(&state, &id, body.entry_id.as_deref()).await
}

async fn fork_session_inner(state: &AppState, id: &str, entry_id: Option<&str>) -> Response {
    if state.route_backend(id) == BackendKind::Rpc {
        if let Some(active) = state.pi_rpc.get(id) {
            let entry = match entry_id {
                Some(entry) => entry.to_string(),
                None => {
                    let entries = match active.send(&json!({"type":"get_entries"})).await {
                        Ok(entries) => entries,
                        Err(error) => return internal(error),
                    };
                    entries
                        .get("leafId")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                }
            };
            if entry.is_empty() {
                return bad_request("entryId is required");
            }
            let result = match active.send(&json!({"type":"fork", "entryId": entry})).await {
                Ok(result) => result,
                Err(error) => return internal(error),
            };
            let new_id = result
                .get("newSessionId")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| active.id());
            let new_path = active.path();
            let mut info = read_session(&new_path)
                .map(|session| serde_json::to_value(session.info()))
                .ok()
                .and_then(Result::ok)
                .unwrap_or(Value::Null);
            if info.is_object() {
                info["parentSessionId"] = Value::String(id.to_string());
                info["relation"] = json!({"kind":"fork","originSessionId": id});
            }
            return no_store(json!({ "sessionId": new_id, "session": info }));
        }
    }
    let Some(path) = session::resolve_session_path(&state.config, id) else {
        return not_found("Session not found");
    };
    let session = match read_session(&path) {
        Ok(session) => session,
        Err(error) => return internal(error),
    };
    let leaf = entry_id.map(str::to_owned).or_else(|| session.leaf_id());
    let entries = slice_active_branch(&session.entries, leaf.as_deref(), 100_000, false);
    let new_path = match create_session_file(&state.config, Path::new(&session.cwd())) {
        Ok(path) => path,
        Err(error) => return internal(error),
    };
    if let Err(error) = append_lines(&new_path, &entries) {
        return internal(error);
    }
    if let Ok(mut new_session) = read_session(&new_path) {
        let source_title = session.name().unwrap_or_else(|| "Session".into());
        let title = format!("{} · {:04x}", source_title, rand_suffix());
        let _ = append_session_info(&new_path, &title);
        new_session.header.parent_session = Some(path.to_string_lossy().into_owned());
        let _ = rewrite_header(&new_path, &new_session.header);
        let mut info = read_session(&new_path)
            .map(|session| serde_json::to_value(session.info()))
            .ok()
            .and_then(Result::ok)
            .unwrap_or(Value::Null);
        if info.is_object() {
            info["parentSessionId"] = Value::String(id.to_string());
            info["relation"] = json!({"kind": "fork", "originSessionId": id});
        }
        return no_store(json!({
            "sessionId": new_session.header.id,
            "session": info,
        }));
    }
    internal("The forked session could not be read")
}

fn rewrite_header(path: &Path, header: &session::SessionHeader) -> std::io::Result<()> {
    let text = std::fs::read_to_string(path)?;
    let mut lines = text.lines();
    let _ = lines.next();
    let rest = lines.collect::<Vec<_>>().join("\n");
    let header = serde_json::to_string(header).unwrap_or_default();
    std::fs::write(path, format!("{header}\n{rest}\n"))
}

fn rand_suffix() -> u16 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    uuid::Uuid::new_v4().to_string().hash(&mut hasher);
    hasher.finish() as u16
}

#[derive(Debug, Deserialize, Default)]
pub struct ContextQuery {
    #[serde(rename = "leafId")]
    leaf_id: Option<String>,
    tail: Option<usize>,
    before: Option<String>,
    #[serde(rename = "deferThinking")]
    defer_thinking: Option<String>,
    #[serde(rename = "deferMedia")]
    defer_media: Option<String>,
}

pub async fn context(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<ContextQuery>,
) -> Response {
    let Some(path) = session::resolve_session_path(&state.config, &id) else {
        return not_found("Session not found");
    };
    let session = match read_session(&path) {
        Ok(session) => session,
        Err(error) => return internal(error),
    };
    let tail = query
        .tail
        .unwrap_or(session::DEFAULT_TAIL)
        .clamp(1, session::MAX_TAIL);
    let context = build_context(
        &id,
        &session.entries,
        query.leaf_id.as_deref(),
        tail,
        query.before.as_deref(),
        query
            .defer_thinking
            .as_deref()
            .is_some_and(|value| value != "0"),
        query
            .defer_media
            .as_deref()
            .is_some_and(|value| value != "0"),
    );
    no_store(json!({
        "context": context,
        "tail": tail,
        "before": query.before,
    }))
}

pub async fn state(State(state): State<Arc<AppState>>, AxumPath(id): AxumPath<String>) -> Response {
    let backend = state.route_backend(&id);
    if let Some(session) = state.pi_rpc.get(&id) {
        let mut state_json = session.state_json();
        if let Some(object) = state_json.as_object_mut() {
            object.insert("backend".into(), Value::String(backend.id().into()));
        }
        return no_store(json!({
            "running": true,
            "backend": backend.id(),
            "state": state_json,
        }));
    }
    if let Some(runtime) = state.agents.get(&id) {
        let mut state_json = runtime.state_json(true);
        if let Some(object) = state_json.as_object_mut() {
            object.insert("backend".into(), Value::String(backend.id().into()));
        }
        return no_store(json!({
            "running": true,
            "backend": backend.id(),
            "state": state_json,
        }));
    }
    if session::resolve_session_path(&state.config, &id).is_none() {
        return not_found("Session not found");
    }
    no_store(json!({ "running": false, "backend": backend.id() }))
}

#[derive(Debug, Deserialize, Default)]
pub struct ExportQuery {
    inline: Option<String>,
}

async fn export_with_pi_cli(path: &Path) -> Option<String> {
    let temp = std::env::temp_dir().join(format!("pi-star-export-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp).ok()?;
    let pi = crate::pi_runtime::command();
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        tokio::process::Command::new(&pi.program)
            .args(&pi.prefix)
            .arg("--export")
            .arg(path)
            .current_dir(&temp)
            .env("PI_OFFLINE", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    let html = if output.status.success() {
        std::fs::read_dir(&temp).ok().and_then(|entries| {
            entries.flatten().find_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.starts_with("pi-session-")
                    .then(|| std::fs::read_to_string(entry.path()).ok())
                    .flatten()
            })
        })
    } else {
        None
    };
    let _ = std::fs::remove_dir_all(&temp);
    html
}

pub async fn export(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<ExportQuery>,
) -> Response {
    let Some(path) = session::resolve_session_path(&state.config, &id) else {
        return not_found("Session not found");
    };
    if let Some(html) = export_with_pi_cli(&path).await {
        let disposition = if query.inline.as_deref() == Some("1") {
            "inline"
        } else {
            "attachment"
        };
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("{disposition}; filename=\"session-{id}.html\""),
                ),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
            html,
        )
            .into_response();
    }
    let session = match read_session(&path) {
        Ok(session) => session,
        Err(error) => return internal(error),
    };
    let title = session
        .name()
        .unwrap_or_else(|| session.first_user_text().chars().take(60).collect());
    let mut body = String::new();
    for entry in &session.entries {
        if entry_type(entry) != Some("message") {
            continue;
        }
        let role = entry
            .pointer("/message/role")
            .and_then(Value::as_str)
            .unwrap_or("");
        if role == "system" {
            continue;
        }
        let text = message_text(entry);
        if text.is_empty() {
            continue;
        }
        body.push_str(&format!(
            "<section class=\"{}\"><h2>{}</h2><pre>{}</pre></section>\n",
            html_escape(role),
            html_escape(role),
            html_escape(&text)
        ));
    }
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title>\
         <style>body{{font-family:system-ui,sans-serif;max-width:900px;margin:2rem auto;padding:0 1rem;background:#fff;color:#111}}\
         section{{border-top:1px solid #ddd;padding:.75rem 0}}h2{{font-size:.8rem;text-transform:uppercase;color:#666}}\
         pre{{white-space:pre-wrap;word-break:break-word;font-family:ui-monospace,monospace}}</style></head>\
         <body><h1>{title}</h1>{body}</body></html>",
        title = html_escape(&title)
    );
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "{}; filename=\"session-{id}.html\"",
                    if query.inline.as_deref() == Some("1") {
                        "inline"
                    } else {
                        "attachment"
                    }
                ),
            ),
        ],
        html,
    )
        .into_response()
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub async fn auto_name(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let Some(path) = session::resolve_session_path(&state.config, &id) else {
        return not_found("Session not found");
    };
    let Ok(session) = read_session(&path) else {
        return internal("could not read session");
    };
    let (_, session_model) = session::session_settings(&session.entries, None);
    let default_model = state.models().ok().and_then(|data| data.default_model);
    let model_ref = session_model
        .map(|model| (model.provider, model.model_id))
        .or_else(|| default_model.map(|model| (model.provider, model.model_id)));
    if let Some((provider, model_id)) = model_ref {
        let defs = models::model_defs(&state.config).unwrap_or_default();
        if let Some(model) = defs.get(&format!("{provider}/{model_id}")).cloned() {
            let generated = tokio::time::timeout(
                std::time::Duration::from_secs(90),
                pi_star_core::session_title::generate_title(
                    state.agents.client.clone(),
                    model,
                    &session.entries,
                ),
            )
            .await;
            match generated {
                Ok(Ok(generated)) => {
                    if let Err(error) = append_session_info(&path, &generated.title) {
                        return internal(error);
                    }
                    return no_store(json!({
                        "title": generated.title,
                        "usage": generated.usage,
                    }));
                }
                Ok(Err(error)) => {
                    return crate::routes::json_error(
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        error.to_string(),
                    );
                }
                Err(_) => {
                    return crate::routes::json_error(
                        axum::http::StatusCode::GATEWAY_TIMEOUT,
                        "Session title generation timed out",
                    );
                }
            }
        }
    }
    // No configured model: keep the local fallback rather than failing the
    // action outright. A configured model always goes through the model.
    let text = session.first_user_text();
    let title = text
        .split_whitespace()
        .take(12)
        .collect::<Vec<_>>()
        .join(" ");
    let title = if title.is_empty() {
        "New session".to_string()
    } else {
        title
    };
    if state.route_backend(&id) == BackendKind::Rpc {
        if let Some(active) = state.pi_rpc.get(&id) {
            if let Err(error) = active
                .send(&json!({"type": "set_session_name", "name": title}))
                .await
            {
                return internal(error);
            }
            return no_store(json!({ "title": title, "usage": null }));
        }
    }
    if let Err(error) = append_session_info(&path, &title) {
        return internal(error);
    }
    no_store(json!({ "title": title, "usage": null }))
}

#[derive(Debug, Deserialize)]
pub struct BlockQuery {
    #[serde(rename = "blockIndex")]
    block_index: Option<usize>,
}

pub async fn thinking(
    State(state): State<Arc<AppState>>,
    AxumPath((id, entry_id)): AxumPath<(String, String)>,
    Query(query): Query<BlockQuery>,
) -> Response {
    let Some(path) = session::resolve_session_path(&state.config, &id) else {
        return not_found("Session not found");
    };
    let Ok(session) = read_session(&path) else {
        return not_found("Session not found");
    };
    let Some(entry) = session
        .entries
        .iter()
        .find(|entry| session::entry_id(entry).as_deref() == Some(entry_id.as_str()))
    else {
        return not_found("Assistant message not found");
    };
    if entry_type(entry) != Some("message")
        || entry.pointer("/message/role").and_then(Value::as_str) != Some("assistant")
    {
        return not_found("Assistant message not found");
    }
    let index = query.block_index.unwrap_or(0);
    let Some(block) = entry
        .pointer("/message/content")
        .and_then(Value::as_array)
        .and_then(|blocks| blocks.get(index))
    else {
        return not_found("Thinking block not found");
    };
    if block.get("type").and_then(Value::as_str) != Some("thinking") {
        return not_found("Thinking block not found");
    }
    no_store(json!({ "thinking": block.get("thinking").cloned().unwrap_or(Value::Null) }))
}

pub async fn tool_result_image(
    State(state): State<Arc<AppState>>,
    AxumPath((id, entry_id)): AxumPath<(String, String)>,
    Query(query): Query<BlockQuery>,
) -> Response {
    let Some(path) = session::resolve_session_path(&state.config, &id) else {
        return not_found("Session not found");
    };
    let Ok(session) = read_session(&path) else {
        return not_found("Session not found");
    };
    let Some(entry) = session
        .entries
        .iter()
        .find(|entry| session::entry_id(entry).as_deref() == Some(entry_id.as_str()))
    else {
        return not_found("Tool result not found");
    };
    if entry_type(entry) != Some("message")
        || entry.pointer("/message/role").and_then(Value::as_str) != Some("toolResult")
    {
        return not_found("Tool result not found");
    }
    let index = query.block_index.unwrap_or(0);
    let Some(block) = entry
        .pointer("/message/content")
        .and_then(Value::as_array)
        .and_then(|blocks| blocks.get(index))
    else {
        return not_found("Tool result image not found");
    };
    let source = block.get("source").cloned().unwrap_or(Value::Null);
    let data = if source.get("type").and_then(Value::as_str) == Some("base64") {
        source.get("data").and_then(Value::as_str)
    } else {
        block.get("data").and_then(Value::as_str)
    };
    let mime = source
        .get("media_type")
        .or_else(|| block.get("mimeType"))
        .and_then(Value::as_str)
        .unwrap_or("image/png")
        .to_string();
    let Some(data) = data else {
        return not_found("Tool result image not found");
    };
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else {
        return bad_request("Invalid image data");
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable".to_string(),
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        bytes,
    )
        .into_response()
}
