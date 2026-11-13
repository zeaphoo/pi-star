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
                && entry.get("customType").and_then(Value::as_str) == Some("pi-star:tool-selection")
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

// ---------------------------------------------------------------------------
// Trace view: the raw entry log
// ---------------------------------------------------------------------------

/// Entries per `/entries` page. Big enough that a long session is a handful of
/// requests, small enough that the payload stays under a megabyte after
/// previews.
const DEFAULT_ENTRIES_LIMIT: usize = 400;
const MAX_ENTRIES_LIMIT: usize = 2000;
/// Characters kept per text block in a list page. The rows show one truncated
/// line each; 160 covers the widest column before the CSS clamps it anyway.
const DEFAULT_PREVIEW_CHARS: usize = 160;
const MAX_PREVIEW_CHARS: usize = 4000;

#[derive(Debug, Deserialize, Default)]
pub struct EntriesQuery {
    /// Exclusive cursor: return the entries appended after this entry id.
    after: Option<String>,
    limit: Option<usize>,
    /// Characters kept per text block. `0` sends entries untouched.
    #[serde(rename = "previewChars")]
    preview_chars: Option<usize>,
}

/// `GET /sessions/{id}/entries` — the session's entry log in append order.
///
/// `/context` projects entries into model-visible messages and drops everything
/// that is not one: model changes, thinking-level changes, labels, tool
/// selection, context edits, empty branch summaries. The trace view exists to
/// inspect exactly those events, so it reads the log itself — no projection, no
/// reordering, `parentId` intact.
///
/// `previewChars` caps every text block so a 20 MB tool result never crosses
/// the wire for a list row; the detail panel re-reads one entry uncapped.
pub async fn entries(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<EntriesQuery>,
) -> Response {
    let Some(path) = session::resolve_session_path(&state.config, &id) else {
        return not_found("Session not found");
    };
    let Ok(session) = read_session(&path) else {
        return not_found("Session not found");
    };
    let limit = query
        .limit
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_ENTRIES_LIMIT)
        .clamp(1, MAX_ENTRIES_LIMIT);
    let preview = match query.preview_chars {
        Some(0) => None,
        Some(value) => Some(value.clamp(1, MAX_PREVIEW_CHARS)),
        None => Some(DEFAULT_PREVIEW_CHARS),
    };
    // The cursor is exclusive. An unknown one would silently restart at the
    // first entry and duplicate the whole list, so reject it instead.
    let start = match query.after.as_deref() {
        Some(after) => match session
            .entries
            .iter()
            .position(|entry| entry_id(entry).as_deref() == Some(after))
        {
            Some(index) => index + 1,
            None => return bad_request("Unknown entry cursor"),
        },
        None => 0,
    };
    let end = start.saturating_add(limit).min(session.entries.len());
    let page: Vec<Value> = session.entries[start..end]
        .iter()
        .map(|entry| match preview {
            Some(chars) => preview_entry(entry, chars),
            None => entry.clone(),
        })
        .collect();
    no_store(json!({
        "sessionId": id,
        "leafId": session.leaf_id(),
        "entries": page,
        "total": session.entries.len(),
        "hasMore": end < session.entries.len(),
        "nextAfter": page.last().and_then(entry_id),
    }))
}

/// `GET /sessions/{id}/entries/{entry_id}` — one raw entry, no preview cap.
/// The trace detail panel asks for this when a block arrived truncated.
pub async fn entry(
    State(state): State<Arc<AppState>>,
    AxumPath((id, entry_id)): AxumPath<(String, String)>,
) -> Response {
    let Some(path) = session::resolve_session_path(&state.config, &id) else {
        return not_found("Session not found");
    };
    let Ok(session) = read_session(&path) else {
        return not_found("Session not found");
    };
    match session
        .entries
        .iter()
        .find(|entry| session::entry_id(entry).as_deref() == Some(entry_id.as_str()))
    {
        Some(entry) => no_store(json!({ "entry": entry })),
        None => not_found("Entry not found"),
    }
}

/// Shrink an entry for a list row. Only content is capped: every other field is
/// small metadata (ids, timestamps, model names, tool names) the row needs in
/// full. Both content shapes pi writes are handled — the `message.content` of a
/// message, and the flat `content` of a `custom_message`.
fn preview_entry(entry: &Value, chars: usize) -> Value {
    let mut entry = entry.clone();
    if !preview_content_at(&mut entry, "/message/content", chars) {
        preview_content_at(&mut entry, "/content", chars);
    }
    // Tool `details` keeps its own copies of output — a bash truncation record
    // holds the whole tail — so it is capped the same way. The client re-reads
    // the entry when it needs the original.
    if let Some(details) = entry.pointer_mut("/message/details")
        && truncate_strings(details, chars, 4)
        && let Some(object) = details.as_object_mut()
    {
        object.insert("previewTruncated".into(), Value::Bool(true));
    }
    entry
}

/// Cap every long string inside `value`, `depth` levels deep. Returns whether
/// anything was cut.
fn truncate_strings(value: &mut Value, chars: usize, depth: usize) -> bool {
    if depth == 0 {
        return false;
    }
    match value {
        Value::String(text) => {
            let total = text.chars().count();
            if total <= chars {
                return false;
            }
            *text = text.chars().take(chars).collect();
            true
        }
        // `any` short-circuits, which would stop capping after the first cut.
        Value::Array(items) => {
            let mut cut = false;
            for item in items.iter_mut() {
                cut |= truncate_strings(item, chars, depth - 1);
            }
            cut
        }
        Value::Object(map) => {
            let mut cut = false;
            for item in map.values_mut() {
                cut |= truncate_strings(item, chars, depth - 1);
            }
            cut
        }
        _ => false,
    }
}

/// Cap one content value: a bare string, or an array of blocks. Returns whether
/// there was anything to cap, so the caller can try the other location.
fn preview_content_at(entry: &mut Value, pointer: &str, chars: usize) -> bool {
    let Some(content) = entry.pointer_mut(pointer) else {
        return false;
    };
    if let Some(text) = content.as_str() {
        let total = text.chars().count();
        if total <= chars {
            return true;
        }
        // Keep the block-array shape the client already reads rather than
        // inventing a third one: a bare string becomes one capped text block.
        *content = json!([{
            "type": "text",
            "text": text.chars().take(chars).collect::<String>(),
            "truncated": true,
            "fullLength": total,
        }]);
        return true;
    }
    let Some(blocks) = content.as_array_mut() else {
        return false;
    };
    for block in blocks.iter_mut() {
        let Some(kind) = block.get("type").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        match kind.as_str() {
            "text" => truncate_string_field(block, "text", chars),
            "thinking" => {
                truncate_string_field(block, "thinking", chars);
                // The detail panel loads the rest through the existing
                // `/entries/{id}/thinking` route, the same contract `/context`
                // uses for deferred reasoning.
                block["deferred"] = Value::Bool(true);
            }
            "image" => {
                // Base64 never rides along with a list page; the image route
                // already serves it by block index. pi stores two shapes:
                // `{data, mimeType}` and `{source: {data, media_type}}`.
                if let Some(source) = block.get_mut("source").and_then(Value::as_object_mut) {
                    source.remove("data");
                }
                if let Some(object) = block.as_object_mut() {
                    if object.contains_key("data") {
                        object.remove("data");
                    }
                }
                block["deferred"] = Value::Bool(true);
            }
            _ => {}
        }
    }
    true
}

fn truncate_string_field(block: &mut Value, field: &str, chars: usize) {
    let Some(text) = block.get(field).and_then(Value::as_str).map(str::to_owned) else {
        return;
    };
    let total = text.chars().count();
    if total <= chars {
        return;
    }
    block[field] = Value::String(text.chars().take(chars).collect());
    block["truncated"] = Value::Bool(true);
    block["fullLength"] = Value::from(total);
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
