//! pi v3 JSONL backend for the merged agent harness.
//!
//! The harness models a session as entries plus lane records. pi's v3 format has
//! only the entry tree (`type`/`id`/`parentId`/`timestamp`), so this backend
//! keeps records in memory for the lifetime of the process and persists entries
//! through [`crate::session`]'s existing v3 reader/writer.
//!
//! Notes on pi semantics:
//! - the active leaf is the last entry in the file and is not persisted;
//! - lanes collapse to a single `main` lane;
//! - names and labels are written as pi's native `session_info` / `label`
//!   entries (and mirrored into the harness facts);
//! - entry kinds the harness does not model are preserved via [`Entry::Raw`].

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{SecondsFormat, TimeZone, Utc};
use serde_json::{Map, Value, json};
use tokio::sync::Mutex as AsyncMutex;

use pi_star_agent::message::{AgentMessage, CustomMessage};
use pi_star_harness::error::{SessionError, SessionResult};
use pi_star_harness::session::types::{
    BranchBounds, Entry, EntryBase, EntryQuery, ForkOptions, ForkPosition, IdGenerator,
    LanePointer, LaneRecord, LogItem, LogOptions, OperationStartedRecord, ProvisionedEntry,
    RawEntry, RecordQuery, SessionCreateOptions, SessionMetadata, SessionMutation, SessionRepo,
    SessionStats, SessionStorage,
};
use pi_star_harness::session::{
    Clock, DefaultIdGenerator, InMemorySessionStorage, SessionState, SystemClock,
};

use crate::config::{Config, session_file_for_id};
use crate::session::{append_lines, read_header, read_session, session_dir_for_cwd};

/// Repo bound to one workspace directory.
pub struct V3SessionRepo {
    config: Config,
    cwd: PathBuf,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGenerator>,
}

impl V3SessionRepo {
    pub fn new(config: Config, cwd: impl Into<PathBuf>) -> Self {
        Self {
            config,
            cwd: cwd.into(),
            clock: Arc::new(SystemClock),
            ids: Arc::new(DefaultIdGenerator::new()),
        }
    }

    fn open_path(&self, path: PathBuf) -> SessionResult<V3SessionStorage> {
        V3SessionStorage::load(path, Arc::clone(&self.clock), Arc::clone(&self.ids))
    }
}

#[async_trait::async_trait]
impl SessionRepo for V3SessionRepo {
    type Storage = V3SessionStorage;

    async fn create(&self, options: &SessionCreateOptions) -> SessionResult<Self::Storage> {
        let id = options
            .id
            .clone()
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| self.ids.next());
        let created_at = self.clock.now_ms();
        let path = write_header(&self.config, &self.cwd, &id, created_at, None)
            .map_err(|error| SessionError::storage(error.to_string()))?;
        self.open_path(path)
    }

    async fn open(&self, metadata: &SessionMetadata) -> SessionResult<Self::Storage> {
        let path = session_file_for_id(&self.config.sessions_dir, &metadata.id)
            .ok_or_else(|| SessionError::not_found(format!("Session {} not found", metadata.id)))?;
        self.open_path(path)
    }

    async fn list(&self) -> SessionResult<Vec<SessionMetadata>> {
        let dir = session_dir_for_cwd(&self.config, &self.cwd);
        let Ok(entries) = fs::read_dir(dir) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            if let Ok(header) = read_header(&path) {
                out.push(SessionMetadata {
                    id: header.id,
                    created_at: parse_iso_ms(&header.timestamp),
                    parent_session_id: None,
                });
            }
        }
        out.sort_by_key(|metadata| metadata.created_at);
        Ok(out)
    }

    async fn delete(&self, metadata: &SessionMetadata) -> SessionResult<()> {
        let path = session_file_for_id(&self.config.sessions_dir, &metadata.id)
            .ok_or_else(|| SessionError::not_found(format!("Session {} not found", metadata.id)))?;
        fs::remove_file(path).map_err(|error| SessionError::storage(error.to_string()))
    }

    async fn fork(
        &self,
        source: &SessionMetadata,
        options: &SessionCreateOptions,
        fork: &ForkOptions,
    ) -> SessionResult<Self::Storage> {
        let source_path = session_file_for_id(&self.config.sessions_dir, &source.id)
            .ok_or_else(|| SessionError::not_found(format!("Session {} not found", source.id)))?;
        let session =
            read_session(&source_path).map_err(|error| SessionError::storage(error.to_string()))?;
        let entries = match fork {
            ForkOptions::Tree => session.entries.clone(),
            ForkOptions::Branch { entry_id, position } => branch_entries(
                &session.entries,
                entry_id.as_deref(),
                matches!(position, Some(ForkPosition::Before)),
            ),
        };
        let id = options
            .id
            .clone()
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| self.ids.next());
        let path = write_header(
            &self.config,
            &self.cwd,
            &id,
            self.clock.now_ms(),
            Some(&source_path),
        )
        .map_err(|error| SessionError::storage(error.to_string()))?;
        append_lines(&path, &entries).map_err(|error| SessionError::storage(error.to_string()))?;
        self.open_path(path)
    }
}

/// v3-backed [`SessionStorage`]. Wraps the in-memory state machine and mirrors
/// every committed entry to the JSONL file, so reads/queries keep the harness
/// semantics while the file stays a plain pi session.
pub struct V3SessionStorage {
    inner: InMemorySessionStorage,
    path: PathBuf,
    ids: Arc<dyn IdGenerator>,
    clock: Arc<dyn Clock>,
    io: AsyncMutex<()>,
}

impl V3SessionStorage {
    fn load(
        path: PathBuf,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
    ) -> SessionResult<Self> {
        let header =
            read_header(&path).map_err(|error| SessionError::storage(error.to_string()))?;
        let session =
            read_session(&path).map_err(|error| SessionError::storage(error.to_string()))?;
        let metadata = SessionMetadata {
            id: header.id.clone(),
            created_at: parse_iso_ms(&header.timestamp),
            parent_session_id: None,
        };
        let mut state = SessionState::new();
        for (index, value) in session.entries.iter().enumerate() {
            let seq = index as u64 + 1;
            let mut entry = v3_to_entry(value, seq)?;
            // pi tolerates orphans; the reducer needs a parent that exists.
            if let Some(parent) = entry.parent_id() {
                if state.get_entry(parent).is_none() {
                    entry.base_mut().parent_id = None;
                }
            }
            let timestamp = entry.base().timestamp;
            state
                .apply_mutation(SessionMutation::Entry {
                    seq,
                    timestamp,
                    lane: None,
                    entry,
                })
                .map_err(|error| SessionError::storage(error.to_string()))?;
        }
        if let Some(last) = session.entries.last().and_then(entry_id) {
            let seq = state.next_sequence();
            state
                .apply_mutation(SessionMutation::Lane {
                    seq,
                    lane: "main".to_string(),
                    leaf_id: Some(last.to_string()),
                })
                .map_err(|error| SessionError::storage(error.to_string()))?;
        }
        let mut name: Option<String> = None;
        let mut labels: Vec<(String, Option<String>)> = Vec::new();
        for value in &session.entries {
            match value.get("type").and_then(Value::as_str) {
                Some("session_info") => {
                    let text = value
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    name = (!text.is_empty()).then_some(text);
                }
                Some("label") => {
                    if let Some(target) = value.get("targetId").and_then(Value::as_str) {
                        labels.push((
                            target.to_string(),
                            value
                                .get("label")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                        ));
                    }
                }
                _ => {}
            }
        }
        let apply_fact = |state: &mut SessionState, mutation| {
            let seq = state.next_sequence();
            match mutation {
                FactKind::Name(name) => {
                    state.apply_mutation(SessionMutation::FactName { seq, name })
                }
                FactKind::Label(target_id, label) => {
                    state.apply_mutation(SessionMutation::FactLabel {
                        seq,
                        target_id,
                        label,
                    })
                }
            }
        };
        if let Some(name) = name {
            apply_fact(&mut state, FactKind::Name(Some(name)))
                .map_err(|error| SessionError::storage(error.to_string()))?;
        }
        for (target, label) in labels {
            // Ignore labels whose target is not in this file.
            if state.get_entry(&target).is_none() {
                continue;
            }
            apply_fact(&mut state, FactKind::Label(target, label))
                .map_err(|error| SessionError::storage(error.to_string()))?;
        }
        let inner = InMemorySessionStorage::from_state(
            metadata,
            state,
            Arc::clone(&clock),
            Arc::clone(&ids),
        );
        Ok(Self {
            inner,
            path,
            ids,
            clock,
            io: AsyncMutex::new(()),
        })
    }

    fn persist(&self, entry: &Entry) -> Result<(), String> {
        let value = entry_to_v3(entry)?;
        append_lines(&self.path, &[value]).map_err(|error| error.to_string())
    }

    /// Append a pi-native control entry (session_info/label) and advance the
    /// in-memory leaf with it.
    fn append_control(&self, kind: &str, extra: Map<String, Value>) -> Result<(), String> {
        let mut state = self.inner.snapshot_state();
        let parent_id = state
            .require_lane("main")
            .map_err(|error| error.to_string())?;
        let seq = state.next_sequence();
        let id = self.ids.next();
        let timestamp = self.clock.now_ms();
        let mut value = Map::new();
        value.insert("type".into(), Value::String(kind.to_string()));
        value.insert("id".into(), Value::String(id.clone()));
        value.insert("seq".into(), Value::Number(seq.into()));
        value.insert(
            "parentId".into(),
            parent_id.clone().map(Value::String).unwrap_or(Value::Null),
        );
        value.insert("timestamp".into(), Value::Number(timestamp.into()));
        for (key, item) in extra {
            value.insert(key, item);
        }
        let value = Value::Object(value);
        let entry = Entry::Raw(RawEntry {
            base: EntryBase {
                entry_type: kind.to_string(),
                id,
                seq,
                parent_id,
                timestamp,
            },
            value,
        });
        let wire = entry_to_v3(&entry)?;
        append_lines(&self.path, &[wire]).map_err(|error| error.to_string())?;
        state
            .apply_mutation(SessionMutation::Entry {
                seq,
                timestamp,
                lane: Some("main".to_string()),
                entry,
            })
            .map_err(|error| error.to_string())?;
        self.inner.replace_state(state);
        Ok(())
    }
}

#[async_trait::async_trait]
impl SessionStorage for V3SessionStorage {
    fn metadata(&self) -> SessionMetadata {
        self.inner.metadata()
    }

    async fn get_lanes(&self) -> SessionResult<Vec<LanePointer>> {
        self.inner.get_lanes().await
    }

    async fn create_lane(&self, lane: &str, at: Option<&str>) -> SessionResult<()> {
        self.inner.create_lane(lane, at).await
    }

    async fn move_lane(&self, lane: &str, to: Option<&str>) -> SessionResult<()> {
        self.inner.move_lane(lane, to).await
    }

    async fn append_entry(&self, entry: ProvisionedEntry, lane: &str) -> SessionResult<Entry> {
        let _guard = self.io.lock().await;
        let before = self.inner.snapshot_state();
        let stamped = self.inner.append_entry(entry, lane).await?;
        if let Err(error) = self.persist(&stamped) {
            self.inner.replace_state(before);
            return Err(SessionError::storage(error));
        }
        Ok(stamped)
    }

    async fn append_record(&self, record: LaneRecord) -> SessionResult<LaneRecord> {
        // v3 has no record lines: keep the live process consistent and let the
        // next load start from the last committed entry (pi semantics).
        self.inner.append_record(record).await
    }

    async fn get_entry(&self, id: &str) -> SessionResult<Option<Entry>> {
        self.inner.get_entry(id).await
    }

    async fn find_entries(&self, query: &EntryQuery) -> SessionResult<Vec<Entry>> {
        self.inner.find_entries(query).await
    }

    async fn find_entries_on_branch(
        &self,
        query: &EntryQuery,
        bounds: &BranchBounds,
        start: &str,
    ) -> SessionResult<Vec<Entry>> {
        self.inner
            .find_entries_on_branch(query, bounds, start)
            .await
    }

    async fn context_on_branch(
        &self,
        bounds: &BranchBounds,
        start: &str,
    ) -> SessionResult<pi_star_harness::session::context::SessionContext> {
        self.inner.context_on_branch(bounds, start).await
    }

    async fn find_records(&self, query: &RecordQuery) -> SessionResult<Vec<LaneRecord>> {
        self.inner.find_records(query).await
    }

    async fn find_open_operations(
        &self,
        lane: &str,
        limit: Option<usize>,
    ) -> SessionResult<Vec<OperationStartedRecord>> {
        self.inner.find_open_operations(lane, limit).await
    }

    async fn get_log(&self, options: &LogOptions) -> SessionResult<Vec<LogItem>> {
        self.inner.get_log(options).await
    }

    async fn get_name(&self) -> SessionResult<Option<String>> {
        self.inner.get_name().await
    }

    async fn set_name(&self, name: Option<&str>) -> SessionResult<()> {
        let _guard = self.io.lock().await;
        let before = self.inner.snapshot_state();
        self.inner.set_name(name).await?;
        let mut extra = Map::new();
        extra.insert(
            "name".into(),
            Value::String(name.unwrap_or("").trim().to_string()),
        );
        if let Err(error) = self.append_control("session_info", extra) {
            self.inner.replace_state(before);
            return Err(SessionError::storage(error));
        }
        Ok(())
    }

    async fn get_label(&self, id: &str) -> SessionResult<Option<String>> {
        self.inner.get_label(id).await
    }

    async fn set_label(&self, id: &str, label: Option<&str>) -> SessionResult<()> {
        let _guard = self.io.lock().await;
        let before = self.inner.snapshot_state();
        self.inner.set_label(id, label).await?;
        let mut extra = Map::new();
        extra.insert("targetId".into(), Value::String(id.to_string()));
        extra.insert(
            "label".into(),
            label.map_or(Value::Null, |label| Value::String(label.to_string())),
        );
        if let Err(error) = self.append_control("label", extra) {
            self.inner.replace_state(before);
            return Err(SessionError::storage(error));
        }
        Ok(())
    }

    async fn get_stats(&self) -> SessionResult<SessionStats> {
        self.inner.get_stats().await
    }
}

enum FactKind {
    Name(Option<String>),
    Label(String, Option<String>),
}

// ---------------------------------------------------------------------------
// v3 <-> harness entry conversion
// ---------------------------------------------------------------------------

fn entry_to_v3(entry: &Entry) -> Result<Value, String> {
    if let Entry::Raw(raw) = entry {
        let mut value = raw.value.clone();
        let obj = value
            .as_object_mut()
            .ok_or_else(|| "raw entry is not an object".to_string())?;
        obj.remove("seq");
        obj.insert(
            "timestamp".into(),
            Value::String(ms_to_iso(raw.base.timestamp)),
        );
        obj.entry("type")
            .or_insert_with(|| Value::String(raw.base.entry_type.clone()));
        obj.entry("id")
            .or_insert_with(|| Value::String(raw.base.id.clone()));
        obj.insert(
            "parentId".into(),
            raw.base
                .parent_id
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        return Ok(value);
    }

    let mut obj = Map::new();
    obj.insert("type".into(), Value::String(entry.entry_type().to_string()));
    obj.insert("id".into(), Value::String(entry.id().to_string()));
    obj.insert(
        "parentId".into(),
        entry
            .parent_id()
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    obj.insert(
        "timestamp".into(),
        Value::String(ms_to_iso(entry.base().timestamp)),
    );
    match entry {
        Entry::Message(message) => {
            obj.insert("message".into(), message_to_v3(&message.message));
            if let Some(terminate) = message.terminate {
                obj.insert("terminate".into(), Value::Bool(terminate));
            }
        }
        Entry::ModelChange(change) => {
            obj.insert("provider".into(), Value::String(change.provider.clone()));
            obj.insert("modelId".into(), Value::String(change.model_id.clone()));
        }
        Entry::ThinkingLevel(level) => {
            obj.insert(
                "thinkingLevel".into(),
                Value::String(level.thinking_level.clone()),
            );
        }
        // pi v3 has no `active_tools_change`; pi-web stores the same state as a
        // custom entry, which pi/pi-web already understand.
        Entry::ActiveTools(tools) => {
            obj.insert("type".into(), Value::String("custom".into()));
            obj.insert(
                "customType".into(),
                Value::String("pi-web:tool-selection".into()),
            );
            obj.insert(
                "data".into(),
                json!({ "version": 1, "tools": tools.active_tool_names }),
            );
        }
        Entry::Compaction(compaction) => {
            obj.insert("summary".into(), Value::String(compaction.summary.clone()));
            obj.insert(
                "firstKeptEntryId".into(),
                Value::String(if compaction.first_kept_entry_id.is_empty() {
                    compaction.base.id.clone()
                } else {
                    compaction.first_kept_entry_id.clone()
                }),
            );
            obj.insert(
                "tokensBefore".into(),
                Value::Number(compaction.tokens_before.into()),
            );
            if let Some(details) = &compaction.details {
                obj.insert("details".into(), details.clone());
            }
            if let Some(usage) = &compaction.usage {
                obj.insert(
                    "usage".into(),
                    serde_json::to_value(usage).map_err(|error| error.to_string())?,
                );
            }
            if let Some(from_hook) = compaction.from_hook {
                obj.insert("fromHook".into(), Value::Bool(from_hook));
            }
            if let Some(system_message) = &compaction.system_message {
                obj.insert("systemMessage".into(), system_message.clone());
            }
        }
        Entry::BranchSummary(summary) => {
            obj.insert("fromId".into(), Value::String(summary.from_id.clone()));
            obj.insert("summary".into(), Value::String(summary.summary.clone()));
            if let Some(details) = &summary.details {
                obj.insert("details".into(), details.clone());
            }
            if let Some(usage) = &summary.usage {
                obj.insert(
                    "usage".into(),
                    serde_json::to_value(usage).map_err(|error| error.to_string())?,
                );
            }
        }
        Entry::Custom(custom) => {
            obj.insert(
                "customType".into(),
                Value::String(custom.custom_type.clone()),
            );
            if let Some(data) = &custom.data {
                obj.insert("data".into(), data.clone());
            }
        }
        Entry::Raw(_) => unreachable!("handled above"),
    }
    Ok(Value::Object(obj))
}

pub(crate) fn message_to_v3(message: &AgentMessage) -> Value {
    let mut value = serde_json::to_value(message).unwrap_or(Value::Null);
    let Some(obj) = value.as_object_mut() else {
        return value;
    };
    let kind = obj
        .remove("kind")
        .and_then(|kind| kind.as_str().map(str::to_owned))
        .unwrap_or_default();
    if kind == "custom" {
        let role = obj
            .remove("role")
            .and_then(|role| role.as_str().map(str::to_owned))
            .unwrap_or_else(|| "custom".to_string());
        let content = obj.remove("content").unwrap_or(Value::Null);
        let data = obj.remove("data").unwrap_or(Value::Null);
        let timestamp = obj.remove("timestamp").unwrap_or(Value::Null);
        if role == "bashExecution" {
            return json!({
                "role": "bashExecution",
                "command": data.get("command").cloned().unwrap_or(Value::Null),
                "output": data.get("output").cloned().unwrap_or(Value::Null),
                "exitCode": data.get("exitCode").cloned().unwrap_or(Value::Null),
                "timestamp": timestamp,
            });
        }
        return json!({
            "role": "custom",
            "customType": role,
            "content": content,
            "display": true,
            "details": data,
            "timestamp": timestamp,
        });
    }
    value
}

pub(crate) fn v3_to_entry(value: &Value, seq: u64) -> SessionResult<Entry> {
    let Some(obj) = value.as_object() else {
        return Err(SessionError::invalid_entry(
            "session entry is not an object",
        ));
    };
    let ty = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| SessionError::invalid_entry("session entry missing type"))?;
    let timestamp = parse_iso_ms(
        obj.get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );

    let mut normalized = obj.clone();
    normalized.insert("seq".into(), Value::Number(seq.into()));
    normalized.insert("timestamp".into(), Value::Number(timestamp.into()));

    if ty == "message" {
        if let Some(message) = obj.get("message") {
            if let Ok(message) = message_from_v3(message) {
                normalized.insert(
                    "message".into(),
                    serde_json::to_value(message).unwrap_or(Value::Null),
                );
            }
        }
    } else if ty == "custom" {
        let custom_type = obj.get("customType").and_then(Value::as_str).unwrap_or("");
        if custom_type == "pi-web:tool-selection" {
            if let Some(tools) = obj.get("data").and_then(|data| data.get("tools")).cloned() {
                normalized.insert("type".into(), Value::String("active_tools_change".into()));
                normalized.insert("activeToolNames".into(), tools);
            }
        }
    }

    serde_json::from_value(Value::Object(normalized))
        .map_err(|error| SessionError::invalid_entry(error.to_string()))
}

fn message_from_v3(value: &Value) -> Result<AgentMessage, String> {
    let role = value
        .get("role")
        .and_then(Value::as_str)
        .ok_or_else(|| "message missing role".to_string())?;
    match role {
        "user" | "assistant" | "toolResult" => {
            let mut obj = value
                .as_object()
                .cloned()
                .ok_or_else(|| "message is not an object".to_string())?;
            let kind = match role {
                "user" => "user",
                "assistant" => "assistant",
                _ => "tool_result",
            };
            obj.insert("kind".into(), Value::String(kind.to_string()));
            serde_json::from_value(Value::Object(obj)).map_err(|error| error.to_string())
        }
        "bashExecution" => Ok(AgentMessage::Custom(CustomMessage {
            role: "bashExecution".to_string(),
            content: Vec::new(),
            data: value.clone(),
            timestamp: value.get("timestamp").and_then(Value::as_i64).unwrap_or(0),
        })),
        "custom" => {
            let custom_type = value
                .get("customType")
                .and_then(Value::as_str)
                .unwrap_or("custom")
                .to_string();
            let content = value.get("content").cloned().unwrap_or(Value::Null);
            let content = if content.is_array() {
                content
            } else {
                json!([{
                    "type": "text",
                    "text": content.as_str().unwrap_or_default(),
                }])
            };
            let content = serde_json::from_value(content)
                .map_err(|error| format!("custom message content is invalid: {error}"))?;
            Ok(AgentMessage::Custom(CustomMessage {
                role: custom_type,
                content,
                data: value.get("details").cloned().unwrap_or(Value::Null),
                timestamp: value.get("timestamp").and_then(Value::as_i64).unwrap_or(0),
            }))
        }
        other => Err(format!("unsupported message role {other}")),
    }
}

fn entry_id(value: &Value) -> Option<&str> {
    value.get("id").and_then(Value::as_str)
}

fn branch_entries(entries: &[Value], leaf: Option<&str>, before: bool) -> Vec<Value> {
    let leaf = leaf
        .map(str::to_owned)
        .or_else(|| entries.last().and_then(entry_id).map(str::to_owned));
    let Some(leaf) = leaf else {
        return Vec::new();
    };
    let by_id: std::collections::HashMap<&str, &Value> = entries
        .iter()
        .filter_map(|entry| entry_id(entry).map(|id| (id, entry)))
        .collect();
    let mut path = Vec::new();
    let mut current = Some(leaf.as_str());
    while let Some(id) = current {
        let Some(entry) = by_id.get(id) else { break };
        path.push((*entry).clone());
        current = entry.get("parentId").and_then(Value::as_str);
    }
    path.reverse();
    if before {
        path.pop();
    }
    path
}

fn write_header(
    config: &Config,
    cwd: &Path,
    id: &str,
    created_at: i64,
    parent: Option<&Path>,
) -> Result<PathBuf, String> {
    let dir = session_dir_for_cwd(config, cwd);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let stamp = ms_to_iso(created_at).replace([':', '.'], "-");
    let path = dir.join(format!("{stamp}_{id}.jsonl"));
    let mut header = json!({
        "type": "session",
        "version": 3,
        "id": id,
        "timestamp": ms_to_iso(created_at),
        "cwd": cwd.to_string_lossy(),
    });
    if let Some(parent) = parent {
        header["parentSession"] = Value::String(parent.to_string_lossy().into_owned());
    }
    let mut line = serde_json::to_string(&header).map_err(|error| error.to_string())?;
    line.push('\n');
    fs::write(&path, line).map_err(|error| error.to_string())?;
    Ok(path)
}

fn parse_iso_ms(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|date| date.timestamp_millis())
        .unwrap_or(0)
}

fn ms_to_iso(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi_star_harness::session::types::{ActiveToolsEntry, CompactionEntry};

    #[test]
    fn message_roundtrip_keeps_the_pi_shape() {
        let message = AgentMessage::User(pi_star_ai::types::UserMessage::new("hi", 7));
        let wire = message_to_v3(&message);
        assert_eq!(wire["role"], "user");
        assert!(wire.get("kind").is_none());
        assert_eq!(message_from_v3(&wire).unwrap(), message);
    }

    #[test]
    fn custom_message_maps_to_pi_custom_message() {
        let message = AgentMessage::Custom(CustomMessage {
            role: "note".into(),
            content: vec![pi_star_ai::types::Content::text("body")],
            data: json!({"a": 1}),
            timestamp: 3,
        });
        let wire = message_to_v3(&message);
        assert_eq!(wire["role"], "custom");
        assert_eq!(wire["customType"], "note");
        assert_eq!(wire["details"], json!({"a": 1}));
        assert_eq!(message_from_v3(&wire).unwrap(), message);
    }

    #[test]
    fn system_messages_become_raw_and_survive_a_roundtrip() {
        let wire = json!({
            "type": "message",
            "id": "sys",
            "parentId": null,
            "timestamp": "2026-01-01T00:00:00.000Z",
            "message": {"role": "system", "content": "", "sections": {"preamble": "p"}},
        });
        let entry = v3_to_entry(&wire, 1).unwrap();
        assert!(matches!(entry, Entry::Raw(_)));
        let back = entry_to_v3(&entry).unwrap();
        assert_eq!(back["type"], "message");
        assert_eq!(back["message"]["role"], "system");
        assert_eq!(back["timestamp"], "2026-01-01T00:00:00.000Z");
    }

    #[test]
    fn compaction_writes_first_kept_entry_id() {
        let entry = Entry::Compaction(CompactionEntry {
            base: EntryBase {
                entry_type: "compaction".into(),
                id: "c1".into(),
                seq: 2,
                parent_id: Some("e1".into()),
                timestamp: 0,
            },
            summary: "s".into(),
            first_kept_entry_id: "e1".into(),
            tokens_before: 10,
            details: None,
            usage: None,
            from_hook: None,
            system_message: None,
        });
        let wire = entry_to_v3(&entry).unwrap();
        assert_eq!(wire["firstKeptEntryId"], "e1");
        assert!(wire.get("retainedTail").is_none());
    }

    #[test]
    fn tool_selection_maps_to_and_from_pi_custom_entry() {
        let entry = Entry::ActiveTools(ActiveToolsEntry {
            base: EntryBase {
                entry_type: "active_tools_change".into(),
                id: "tools".into(),
                seq: 2,
                parent_id: None,
                timestamp: 0,
            },
            active_tool_names: vec!["read".into()],
        });
        let wire = entry_to_v3(&entry).unwrap();
        assert_eq!(wire["type"], "custom");
        assert_eq!(wire["customType"], "pi-web:tool-selection");
        let back = v3_to_entry(&wire, 2).unwrap();
        assert!(matches!(back, Entry::ActiveTools(_)));
    }

    fn iso(ms: i64) -> String {
        ms_to_iso(ms)
    }

    fn write_fixture(dir: &Path) -> (Config, PathBuf, PathBuf) {
        let agent_dir = dir.join("agent");
        let cwd = dir.join("proj");
        std::fs::create_dir_all(&cwd).unwrap();
        let config = Config::new(Some(agent_dir), None).unwrap();
        let session_dir = session_dir_for_cwd(&config, &cwd);
        std::fs::create_dir_all(&session_dir).unwrap();
        let id = "01a00000-0000-7000-8000-000000000001";
        let path = session_dir.join(format!("2026-01-01T00-00-00-000Z_{id}.jsonl"));
        let user = AgentMessage::User(pi_star_ai::types::UserMessage::new("hello", 1000));
        let assistant =
            AgentMessage::Assistant(Box::new(pi_star_ai::types::AssistantMessage::empty(
                pi_star_ai::types::Api::AnthropicMessages,
                "anthropic",
                "claude",
                1100,
            )));
        let lines = vec![
            json!({
                "type": "session",
                "version": 3,
                "id": id,
                "timestamp": iso(1000),
                "cwd": cwd.to_string_lossy(),
            }),
            json!({
                "type": "message",
                "id": "sys",
                "parentId": null,
                "timestamp": iso(1000),
                "message": {"role": "system", "content": "", "sections": {"preamble": "p"}},
            }),
            json!({
                "type": "custom",
                "id": "tools",
                "parentId": "sys",
                "timestamp": iso(1000),
                "customType": "pi-web:tool-selection",
                "data": {"version": 1, "tools": ["read", "bash"]},
            }),
            json!({
                "type": "model_change",
                "id": "model",
                "parentId": "tools",
                "timestamp": iso(1000),
                "provider": "anthropic",
                "modelId": "claude",
            }),
            json!({
                "type": "thinking_level_change",
                "id": "thinking",
                "parentId": "model",
                "timestamp": iso(1000),
                "thinkingLevel": "high",
            }),
            json!({
                "type": "message",
                "id": "user",
                "parentId": "thinking",
                "timestamp": iso(1000),
                "message": message_to_v3(&user),
            }),
            json!({
                "type": "message",
                "id": "assistant",
                "parentId": "user",
                "timestamp": iso(1100),
                "message": message_to_v3(&assistant),
            }),
            json!({
                "type": "compaction",
                "id": "compact",
                "parentId": "assistant",
                "timestamp": iso(1200),
                "summary": "summary",
                "firstKeptEntryId": "user",
                "tokensBefore": 42,
            }),
            json!({
                "type": "usage",
                "id": "usage",
                "parentId": "compact",
                "timestamp": iso(1200),
                "kind": "summary",
                "provider": "anthropic",
                "model": "claude",
                "usage": {"input": 1, "output": 2, "totalTokens": 3},
            }),
            json!({
                "type": "session_info",
                "id": "info",
                "parentId": "usage",
                "timestamp": iso(1200),
                "name": "fixture",
            }),
        ];
        let text = lines
            .into_iter()
            .map(|line| serde_json::to_string(&line).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&path, text).unwrap();
        (config, cwd, path)
    }

    #[tokio::test]
    async fn loads_a_pi_v3_session_into_the_harness() {
        let dir = tempfile::tempdir().unwrap();
        let (config, cwd, _path) = write_fixture(dir.path());
        let repo = V3SessionRepo::new(config, cwd);
        let metadata = SessionMetadata {
            id: "01a00000-0000-7000-8000-000000000001".into(),
            created_at: 1000,
            parent_session_id: None,
        };
        let storage = repo.open(&metadata).await.unwrap();
        assert_eq!(
            storage.get_name().await.unwrap().as_deref(),
            Some("fixture")
        );
        let leaf = storage.get_lanes().await.unwrap()[0]
            .leaf_id
            .clone()
            .unwrap();
        assert_eq!(leaf, "info");
        assert!(storage.get_entry("sys").await.unwrap().is_some());
        assert!(matches!(
            storage.get_entry("tools").await.unwrap(),
            Some(Entry::ActiveTools(_))
        ));
        assert!(matches!(
            storage.get_entry("usage").await.unwrap(),
            Some(Entry::Raw(_))
        ));

        let path = storage
            .find_entries_on_branch(
                &EntryQuery {
                    order: Some(pi_star_harness::session::types::EntryOrder::OldestFirst),
                    ..Default::default()
                },
                &BranchBounds::default(),
                &leaf,
            )
            .await
            .unwrap();
        let context =
            pi_star_harness::session::context::build_session_context(&path, &Default::default());
        assert_eq!(
            context
                .messages
                .iter()
                .map(|message| message.role().as_str().to_string())
                .collect::<Vec<_>>(),
            ["compactionSummary", "user", "assistant"]
        );
    }

    #[tokio::test]
    async fn appended_messages_and_names_survive_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let (config, cwd, path) = write_fixture(dir.path());
        let repo = V3SessionRepo::new(config, cwd);
        let metadata = SessionMetadata {
            id: "01a00000-0000-7000-8000-000000000001".into(),
            created_at: 1000,
            parent_session_id: None,
        };
        let storage = repo.open(&metadata).await.unwrap();
        storage
            .append_entry(
                ProvisionedEntry {
                    id: "new-user".into(),
                    kind: pi_star_harness::session::types::ProvisionedKind::Message {
                        message: AgentMessage::User(pi_star_ai::types::UserMessage::new(
                            "again", 2000,
                        )),
                        terminate: None,
                    },
                },
                "main",
            )
            .await
            .unwrap();
        storage.set_name(Some("renamed")).await.unwrap();

        let storage = repo.open(&metadata).await.unwrap();
        assert_eq!(
            storage.get_name().await.unwrap().as_deref(),
            Some("renamed")
        );
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(lines.iter().any(|line| line["id"] == "new-user"));
        let last = lines.last().unwrap();
        assert_eq!(last["type"], "session_info");
        assert_eq!(last["name"], "renamed");
        assert_eq!(last["parentId"], "new-user");
        assert_eq!(last["timestamp"].as_str().unwrap().len(), 24);
        assert_eq!(
            storage.get_lanes().await.unwrap()[0].leaf_id.clone(),
            last["id"].as_str().map(str::to_owned)
        );
    }

    #[tokio::test]
    async fn fork_copies_the_branch_and_parent_link() {
        let dir = tempfile::tempdir().unwrap();
        let (config, cwd, _path) = write_fixture(dir.path());
        let repo = V3SessionRepo::new(config, cwd);
        let metadata = SessionMetadata {
            id: "01a00000-0000-7000-8000-000000000001".into(),
            created_at: 1000,
            parent_session_id: None,
        };
        let fork = repo
            .fork(
                &metadata,
                &SessionCreateOptions::default(),
                &ForkOptions::Branch {
                    entry_id: Some("user".into()),
                    position: Some(ForkPosition::At),
                },
            )
            .await
            .unwrap();
        let fork_metadata = fork.metadata();
        assert_ne!(fork_metadata.id, metadata.id);
        let path = session_file_for_id(&repo.config.sessions_dir, &fork_metadata.id).unwrap();
        let header = read_header(&path).unwrap();
        assert!(
            header
                .parent_session
                .as_deref()
                .is_some_and(|parent| parent.ends_with(".jsonl"))
        );
        let session = read_session(&path).unwrap();
        let ids: Vec<&str> = session
            .entries
            .iter()
            .filter_map(|entry| entry.get("id").and_then(Value::as_str))
            .collect();
        assert_eq!(ids.last(), Some(&"user"));
        assert!(!ids.contains(&"compact"));
    }

    #[test]
    fn iso_and_epoch_millis_roundtrip() {
        let iso = "2026-10-30T12:34:56.789Z";
        assert_eq!(ms_to_iso(parse_iso_ms(iso)), iso);
    }
}
