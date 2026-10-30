use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::config::{Config, session_file_for_id};
use crate::project::{ProjectInfo, project_key};

pub const DEFAULT_TAIL: usize = 50;
pub const MAX_TAIL: usize = 1000;
const MIN_RAW_WINDOW_ENTRIES: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHeader {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub version: Option<u32>,
    pub id: String,
    pub timestamp: String,
    pub cwd: String,
    #[serde(default)]
    pub parent_session: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub path: String,
    pub id: String,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub created: String,
    pub modified: String,
    pub message_count: usize,
    pub first_message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details_pending: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation: Option<SessionRelation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_worktree: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transient: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionRelation {
    Fork {
        #[serde(skip_serializing_if = "Option::is_none")]
        origin_session_id: Option<String>,
    },
    Subagent {
        parent_session_id: String,
        profile: String,
        description: String,
        status: String,
    },
}

#[derive(Debug, Clone)]
pub struct SessionFile {
    pub path: PathBuf,
    pub header: SessionHeader,
    pub entries: Vec<Value>,
}

impl SessionFile {
    pub fn id(&self) -> &str {
        &self.header.id
    }

    pub fn cwd(&self) -> &str {
        &self.header.cwd
    }

    pub fn name(&self) -> Option<String> {
        self.entries
            .iter()
            .rev()
            .find(|entry| entry_type(entry) == Some("session_info"))
            .and_then(|entry| entry.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    pub fn leaf_id(&self) -> Option<String> {
        self.entries.last().and_then(entry_id)
    }

    pub fn first_user_text(&self) -> String {
        self.entries
            .iter()
            .find(|entry| {
                entry_type(entry) == Some("message")
                    && entry.pointer("/message/role").and_then(Value::as_str) == Some("user")
            })
            .map(message_text)
            .unwrap_or_default()
    }

    pub fn message_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry_type(entry) == Some("message"))
            .count()
    }

    pub fn info(&self) -> SessionInfo {
        let modified = fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok()
            .map(format_time)
            .unwrap_or_else(|| self.header.timestamp.clone());
        let project = ProjectInfo::for_cwd(Path::new(&self.header.cwd));
        let parent_path = self.header.parent_session.clone();
        let parent_session_id = parent_path.as_deref().and_then(|path| {
            read_header(Path::new(path))
                .ok()
                .map(|header| header.id)
                .or_else(|| {
                    Path::new(path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.rsplit('_').next().unwrap_or(s).to_string())
                })
        });
        let subagent_status = self
            .entries
            .iter()
            .rev()
            .find(|entry| {
                entry_type(entry) == Some("custom")
                    && entry.get("customType").and_then(Value::as_str)
                        == Some("pi-web:subagent-status")
            })
            .and_then(|entry| entry.pointer("/data/status"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let subagent = self
            .entries
            .iter()
            .find(|entry| entry_type(entry) == Some("custom"))
            .and_then(|entry| {
                let custom_type = entry.get("customType").and_then(Value::as_str)?;
                if custom_type != "pi-web:subagent" {
                    return None;
                }
                let data = entry.get("data")?;
                Some(SessionRelation::Subagent {
                    parent_session_id: data
                        .get("parentSessionId")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    profile: data
                        .get("profile")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    description: data
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    status: subagent_status
                        .clone()
                        .or_else(|| {
                            data.get("status")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                        })
                        .unwrap_or_else(|| "completed".to_string()),
                })
            });
        let relation = subagent.or_else(|| {
            self.header
                .parent_session
                .as_ref()
                .map(|_| SessionRelation::Fork {
                    origin_session_id: parent_session_id.clone(),
                })
        });
        let first = self.first_user_text();
        SessionInfo {
            path: self.path.to_string_lossy().into_owned(),
            id: self.header.id.clone(),
            cwd: self.header.cwd.clone(),
            name: self.name(),
            created: self.header.timestamp.clone(),
            modified,
            message_count: self.message_count(),
            first_message: if first.is_empty() {
                "(no messages)".to_string()
            } else {
                first
            },
            details_pending: None,
            parent_session_id,
            relation,
            project_root: Some(project.project_root.clone()),
            project_key: Some(project_key(Path::new(&project.project_root))),
            branch: project.branch,
            is_worktree: Some(project.is_worktree),
            transient: Some(false),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionContext {
    pub messages: Vec<Value>,
    pub entry_ids: Vec<String>,
    pub oldest_entry_id: Option<String>,
    pub has_more: bool,
    pub turns_before: usize,
    pub thinking_level: String,
    pub model: Option<ContextModel>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextModel {
    pub provider: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStats {
    pub user_messages: usize,
    pub assistant_messages: usize,
    pub tool_calls: usize,
    pub tool_results: usize,
    pub total_messages: usize,
    pub tokens: TokenStats,
    pub cost: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenStats {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub total: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub entry: Value,
    pub children: Vec<TreeNode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub compressed_entry_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_preview: Option<BranchPreview>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BranchPreview {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub text: String,
}

pub fn read_header(path: &Path) -> Result<SessionHeader> {
    let file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let header: SessionHeader = serde_json::from_str(line.trim())
        .with_context(|| format!("parse session header {}", path.display()))?;
    if header.kind != "session" {
        return Err(anyhow!("{} is not a pi session", path.display()));
    }
    Ok(header)
}

pub fn read_session(path: &Path) -> Result<SessionFile> {
    let header = read_header(path)?;
    let file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(&line) {
            Ok(value) if value.get("type").and_then(Value::as_str) == Some("session") => {}
            Ok(value) => entries.push(value),
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "skipping invalid session line");
            }
        }
    }
    Ok(SessionFile {
        path: path.to_path_buf(),
        header,
        entries,
    })
}

pub fn resolve_session_path(config: &Config, id: &str) -> Option<PathBuf> {
    session_file_for_id(&config.sessions_dir, id).or_else(|| {
        let path = PathBuf::from(id);
        if path.is_file() { Some(path) } else { None }
    })
}

pub fn load_session(config: &Config, id: &str) -> Result<SessionFile> {
    let path = resolve_session_path(config, id).ok_or_else(|| anyhow!("Session not found"))?;
    read_session(&path)
}

pub fn scan_sessions(config: &Config, summary: bool) -> Vec<SessionInfo> {
    let mut files = Vec::new();
    let Ok(projects) = fs::read_dir(&config.sessions_dir) else {
        return Vec::new();
    };
    for project in projects.flatten() {
        if !project.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let Ok(entries) = fs::read_dir(project.path()) else {
            continue;
        };
        for file in entries.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                files.push(path);
            }
        }
    }
    // A large agent directory is mostly independent files; parsing them on one
    // thread makes the first sidebar paint wait for every transcript.
    let worker_count = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
        .min(8)
        .max(1);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let collected = parking_lot::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(path) = files.get(index) else { break };
                    let info = if summary {
                        read_header_only_info(path)
                    } else {
                        read_session(path).ok().map(|session| session.info())
                    };
                    if let Some(info) = info {
                        collected.lock().push(info);
                    }
                }
            });
        }
    });
    let mut sessions = collected.into_inner();
    sessions.sort_by(|a, b| b.modified.cmp(&a.modified));
    sessions
}

pub fn read_header_only_info(path: &Path) -> Option<SessionInfo> {
    let header = read_header(path).ok()?;
    let modified = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map(format_time)
        .unwrap_or_else(|| header.timestamp.clone());
    let project = ProjectInfo::for_cwd(Path::new(&header.cwd));
    Some(SessionInfo {
        path: path.to_string_lossy().into_owned(),
        id: header.id.clone(),
        cwd: header.cwd.clone(),
        name: None,
        created: header.timestamp.clone(),
        modified,
        message_count: 0,
        first_message: String::new(),
        details_pending: Some(true),
        parent_session_id: None,
        relation: header
            .parent_session
            .as_ref()
            .map(|_| SessionRelation::Fork {
                origin_session_id: None,
            }),
        project_root: Some(project.project_root.clone()),
        project_key: Some(project_key(Path::new(&project.project_root))),
        branch: project.branch,
        is_worktree: Some(project.is_worktree),
        transient: Some(false),
    })
}

pub fn append_lines(path: &Path, values: &[Value]) -> Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    for value in values {
        serde_json::to_writer(&mut file, value)?;
        file.write_all(b"\n")?;
    }
    file.flush()?;
    Ok(())
}

pub fn new_entry(kind: &str, parent_id: Option<&str>, extra: Map<String, Value>) -> Value {
    let mut map = Map::new();
    map.insert("type".into(), Value::String(kind.to_string()));
    for (key, value) in extra {
        map.insert(key, value);
    }
    map.insert("id".into(), Value::String(short_id()));
    map.insert(
        "parentId".into(),
        parent_id.map_or(Value::Null, |id| Value::String(id.to_string())),
    );
    map.insert(
        "timestamp".into(),
        Value::String(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    );
    Value::Object(map)
}

pub fn append_session_info(path: &Path, name: &str) -> Result<()> {
    let session = read_session(path)?;
    let parent = session.leaf_id();
    let entry = new_entry(
        "session_info",
        parent.as_deref(),
        [("name".to_string(), Value::String(name.to_string()))]
            .into_iter()
            .collect(),
    );
    append_lines(path, &[entry])
}

pub fn session_dir_for_cwd(config: &Config, cwd: &Path) -> PathBuf {
    config.sessions_dir.join(crate::config::encode_cwd(cwd))
}

pub fn create_session_file(config: &Config, cwd: &Path) -> Result<PathBuf> {
    create_session_file_with_parent(config, cwd, None)
}

pub fn create_session_file_with_parent(
    config: &Config,
    cwd: &Path,
    parent_session: Option<&Path>,
) -> Result<PathBuf> {
    let dir = session_dir_for_cwd(config, cwd);
    fs::create_dir_all(&dir)?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now();
    let file_name = format!("{}_{}.jsonl", now.format("%Y-%m-%dT%H-%M-%S%.3fZ"), id);
    let path = dir.join(file_name);
    let mut header = json!({
        "type": "session",
        "version": 3,
        "id": id,
        "timestamp": now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "cwd": cwd.to_string_lossy(),
    });
    if let Some(parent) = parent_session {
        header["parentSession"] = Value::String(parent.to_string_lossy().into_owned());
    }
    append_lines(&path, &[header])?;
    Ok(path)
}

pub fn entry_id(entry: &Value) -> Option<String> {
    entry.get("id").and_then(Value::as_str).map(str::to_owned)
}

pub fn entry_parent_id(entry: &Value) -> Option<String> {
    match entry.get("parentId") {
        Some(Value::String(id)) => Some(id.clone()),
        _ => None,
    }
}

pub fn entry_type(entry: &Value) -> Option<&str> {
    entry.get("type").and_then(Value::as_str)
}

pub fn message_text(entry: &Value) -> String {
    let message = entry.get("message").unwrap_or(entry);
    let content = message.get("content");
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| {
                if block.get("type").and_then(Value::as_str) == Some("text") {
                    block.get("text").and_then(Value::as_str)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

pub fn build_tree(entries: &[Value]) -> Vec<TreeNode> {
    let mut nodes: HashMap<String, TreeNode> = HashMap::new();
    let mut order = Vec::new();
    for entry in entries {
        let Some(id) = entry_id(entry) else { continue };
        order.push(id.clone());
        nodes.insert(
            id,
            TreeNode {
                entry: entry.clone(),
                children: Vec::new(),
                compressed_entry_ids: Vec::new(),
                branch_preview: preview_for_entry(entry),
            },
        );
    }
    let mut roots = Vec::new();
    let mut attached: HashMap<String, Vec<String>> = HashMap::new();
    for entry in entries {
        let Some(id) = entry_id(entry) else { continue };
        match entry_parent_id(entry) {
            Some(parent) if nodes.contains_key(&parent) && parent != id => {
                attached.entry(parent).or_default().push(id);
            }
            _ => roots.push(id),
        }
    }
    // Build each root iteratively: a long linear session must not overflow the
    // stack (and a depth limit used to leave its tail behind as fake roots).
    fn take(
        id: &str,
        nodes: &mut HashMap<String, TreeNode>,
        attached: &HashMap<String, Vec<String>>,
    ) -> Option<TreeNode> {
        struct Frame {
            node: TreeNode,
            children: Vec<String>,
            next: usize,
        }
        let root = nodes.remove(id)?;
        let mut stack = vec![Frame {
            node: root,
            children: attached.get(id).cloned().unwrap_or_default(),
            next: 0,
        }];
        loop {
            let frame = stack.last_mut()?;
            if frame.next < frame.children.len() {
                let child_id = frame.children[frame.next].clone();
                frame.next += 1;
                if let Some(child) = nodes.remove(&child_id) {
                    let children = attached.get(&child_id).cloned().unwrap_or_default();
                    stack.push(Frame {
                        node: child,
                        children,
                        next: 0,
                    });
                }
                continue;
            }
            let finished = stack.pop()?;
            if let Some(parent) = stack.last_mut() {
                parent.node.children.push(finished.node);
            } else {
                return Some(finished.node);
            }
        }
    }
    let mut result = Vec::new();
    for root in roots {
        if let Some(node) = take(&root, &mut nodes, &attached) {
            result.push(node);
        }
    }
    // Orphans (missing parent) are still shown, in file order.
    for id in order {
        if nodes.contains_key(&id)
            && let Some(node) = take(&id, &mut nodes, &attached)
        {
            result.push(node);
        }
    }
    project_tree(result)
}

fn project_tree(nodes: Vec<TreeNode>) -> Vec<TreeNode> {
    fn keep(node: &TreeNode, root: bool) -> bool {
        root || node.children.len() != 1
    }
    fn project(node: TreeNode, root: bool, depth: usize) -> Option<TreeNode> {
        if depth > 200 {
            return Some(node);
        }
        if !keep(&node, root) {
            return project(node.children.into_iter().next()?, false, depth + 1);
        }
        let mut projected = TreeNode {
            entry: node.entry,
            children: Vec::new(),
            compressed_entry_ids: node.compressed_entry_ids,
            branch_preview: node.branch_preview,
        };
        for child in node.children {
            let mut compressed = Vec::new();
            let mut current = child;
            while !keep(&current, false) && current.children.len() == 1 {
                if let Some(id) = entry_id(&current.entry) {
                    compressed.push(id);
                }
                let preview = current.branch_preview.clone();
                current = current.children.into_iter().next()?;
                if projected.branch_preview.is_none() {
                    projected.branch_preview = preview;
                }
            }
            if let Some(mut projected_child) = project(current, false, depth + 1) {
                projected_child.compressed_entry_ids = compressed;
                projected.children.push(projected_child);
            }
        }
        Some(projected)
    }
    nodes
        .into_iter()
        .filter_map(|node| project(node, true, 0))
        .collect()
}

fn preview_for_entry(entry: &Value) -> Option<BranchPreview> {
    if entry_type(entry) != Some("message") {
        return None;
    }
    let role = entry.pointer("/message/role").and_then(Value::as_str)?;
    if role == "system" {
        return None;
    }
    let text = message_text(entry).replace(char::is_whitespace, " ");
    let text = text.trim();
    let text = if text.is_empty() {
        if role == "assistant" {
            "[assistant]".to_string()
        } else {
            "message".to_string()
        }
    } else if text.chars().count() > 40 {
        format!("{}…", text.chars().take(40).collect::<String>())
    } else {
        text.to_string()
    };
    Some(BranchPreview {
        role: matches!(role, "user" | "assistant").then(|| role.to_string()),
        text,
    })
}

pub fn slice_active_branch(
    entries: &[Value],
    leaf_id: Option<&str>,
    tail: usize,
    exclude_leaf: bool,
) -> Vec<Value> {
    if tail == 0 {
        return entries.to_vec();
    }
    let by_id: HashMap<&str, &Value> = entries
        .iter()
        .filter_map(|entry| {
            entry
                .get("id")
                .and_then(Value::as_str)
                .map(|id| (id, entry))
        })
        .collect();
    let mut leaf = leaf_id
        .and_then(|id| by_id.get(id).copied())
        .or_else(|| entries.last());
    if exclude_leaf {
        leaf = leaf
            .and_then(entry_parent_id)
            .and_then(|id| by_id.get(id.as_str()).copied());
    }
    let Some(mut current) = leaf else {
        return Vec::new();
    };
    let mut chain = Vec::new();
    let mut visible = 0usize;
    let raw_cap = MIN_RAW_WINDOW_ENTRIES.max(tail.saturating_mul(6));
    loop {
        chain.push(current.clone());
        if counts_toward_tail(current) {
            visible += 1;
        }
        if visible >= tail || chain.len() >= raw_cap {
            break;
        }
        let Some(parent) = entry_parent_id(current).and_then(|id| by_id.get(id.as_str()).copied())
        else {
            break;
        };
        current = parent;
    }
    chain.reverse();
    chain
}

fn counts_toward_tail(entry: &Value) -> bool {
    if entry_type(entry) == Some("compaction") {
        return true;
    }
    entry_type(entry) == Some("message")
        && matches!(
            entry.pointer("/message/role").and_then(Value::as_str),
            Some("user" | "assistant")
        )
}

pub fn build_context(
    session_id: &str,
    entries: &[Value],
    leaf_id: Option<&str>,
    tail: usize,
    before: Option<&str>,
    defer_thinking: bool,
    defer_media: bool,
) -> SessionContext {
    let tail = tail.clamp(1, MAX_TAIL);
    let (leaf, exclude) = match before {
        Some(before) => (Some(before.to_string()), true),
        None => (
            leaf_id
                .map(str::to_owned)
                .or_else(|| entries.last().and_then(entry_id)),
            false,
        ),
    };
    let sliced = slice_active_branch(entries, leaf.as_deref(), tail, exclude);
    let has_more = sliced.first().and_then(entry_parent_id).is_some();
    let turns_before = if has_more {
        sliced
            .first()
            .and_then(entry_parent_id)
            .map(|parent| count_turns_before(entries, &parent))
            .unwrap_or(0)
    } else {
        0
    };
    let mut messages = Vec::new();
    let mut entry_ids = Vec::new();
    for entry in &sliced {
        if let Some(message) = entry_to_ui_message(entry, session_id, defer_thinking, defer_media) {
            messages.push(message);
            if let Some(id) = entry_id(entry) {
                entry_ids.push(id);
            }
        }
    }
    let (thinking_level, model) = session_settings(entries, leaf.as_deref());
    SessionContext {
        messages,
        entry_ids,
        oldest_entry_id: sliced.first().and_then(entry_id),
        has_more,
        turns_before,
        thinking_level,
        model,
    }
}

fn entry_to_ui_message(
    entry: &Value,
    session_id: &str,
    defer_thinking: bool,
    defer_media: bool,
) -> Option<Value> {
    match entry_type(entry)? {
        "message" => {
            let message = entry.get("message")?.clone();
            if message.get("role").and_then(Value::as_str) == Some("system") {
                return None;
            }
            let mut message = normalize_message(message);
            if defer_media {
                message = defer_tool_images(message, session_id, entry_id(entry).as_deref());
            }
            if defer_thinking {
                if let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) {
                    for block in content {
                        if block.get("type").and_then(Value::as_str) == Some("thinking") {
                            let thinking = block
                                .get("thinking")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .trim_start()
                                .chars()
                                .take(240)
                                .collect::<String>();
                            block["thinking"] = Value::String(thinking.trim_end().to_string());
                            // An empty thinking block is hidden by the chat, not
                            // made loadable: deferring it would show a button.
                            if thinking.trim().is_empty() {
                                block["deferred"] = Value::Bool(false);
                            } else {
                                block["deferred"] = Value::Bool(true);
                            }
                        }
                    }
                }
            }
            Some(message)
        }
        "compaction" => Some(json!({
            "role": "custom",
            "customType": "compaction",
            "content": entry.get("summary").cloned().unwrap_or(Value::String(String::new())),
            "display": true,
            "details": {
                "tokensBefore": entry.get("tokensBefore").cloned().unwrap_or(Value::Null),
                "firstKeptEntryId": entry.get("firstKeptEntryId").cloned().unwrap_or(Value::Null),
            },
            "timestamp": parse_timestamp(entry),
        })),
        "branch_summary" => {
            let summary = entry.get("summary").and_then(Value::as_str)?;
            if summary.is_empty() {
                return None;
            }
            Some(json!({
                "role": "user",
                "content": format!("*The conversation briefly explored another branch and returned with this summary:*\n\n{summary}"),
                "timestamp": parse_timestamp(entry),
            }))
        }
        "custom_message" => Some(json!({
            "role": "custom",
            "customType": entry.get("customType").cloned().unwrap_or(Value::Null),
            "content": entry.get("content").cloned().unwrap_or(Value::Null),
            "display": entry.get("display").cloned().unwrap_or(Value::Bool(true)),
            "details": entry.get("details").cloned().unwrap_or(Value::Null),
            "timestamp": parse_timestamp(entry),
        })),
        _ => None,
    }
}

fn normalize_message(mut message: Value) -> Value {
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return message;
    }
    if let Some(content) = message.get("content") {
        if let Some(text) = content.as_str() {
            message["content"] = json!([{ "type": "text", "text": text }]);
        }
    }
    if let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) {
        for block in content {
            if block.get("type").and_then(Value::as_str) == Some("toolCall") {
                let id = block
                    .get("toolCallId")
                    .or_else(|| block.get("id"))
                    .cloned()
                    .unwrap_or(Value::String(String::new()));
                let name = block
                    .get("toolName")
                    .or_else(|| block.get("name"))
                    .cloned()
                    .unwrap_or(Value::String(String::new()));
                let input = block
                    .get("input")
                    .or_else(|| block.get("arguments"))
                    .filter(|v| v.is_object())
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                block["toolCallId"] = id;
                block["toolName"] = name;
                block["input"] = input;
            }
        }
    }
    message
}

fn defer_tool_images(mut message: Value, session_id: &str, entry_id: Option<&str>) -> Value {
    if message.get("role").and_then(Value::as_str) != Some("toolResult") {
        return message;
    }
    let Some(entry_id) = entry_id else {
        return message;
    };
    let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
        return message;
    };
    for (index, block) in content.iter_mut().enumerate() {
        if block.get("type").and_then(Value::as_str) != Some("image") {
            continue;
        }
        let Some(source) = block.get_mut("source") else {
            continue;
        };
        let media_type = source
            .get("media_type")
            .or_else(|| source.get("mimeType"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let is_base64 = source.get("type").and_then(Value::as_str) == Some("base64");
        if is_base64
            && matches!(
                media_type,
                "image/png" | "image/jpeg" | "image/webp" | "image/gif"
            )
        {
            *source = json!({
                "type": "url",
                "media_type": media_type,
                "url": format!(
                    "/api/sessions/{}/entries/{}/tool-result-image?blockIndex={index}",
                    urlencoding(session_id),
                    urlencoding(entry_id)
                ),
            });
        }
    }
    message
}

fn urlencoding(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn count_turns_before(entries: &[Value], oldest_parent: &str) -> usize {
    let by_id: HashMap<&str, &Value> = entries
        .iter()
        .filter_map(|entry| {
            entry
                .get("id")
                .and_then(Value::as_str)
                .map(|id| (id, entry))
        })
        .collect();
    let mut turns = 0;
    let mut current = by_id.get(oldest_parent).copied();
    while let Some(entry) = current {
        // The anchors the chat groups turns by (pi-web's isMessageGroupAnchor):
        // a user message, a compaction, a displayed subagent notification, or a
        // branch summary. An assistant reply belongs to the same turn.
        let anchor = match entry_type(entry) {
            Some("message") => {
                entry.pointer("/message/role").and_then(Value::as_str) == Some("user")
            }
            Some("compaction") => true,
            Some("branch_summary") => entry
                .get("summary")
                .and_then(Value::as_str)
                .is_some_and(|summary| !summary.is_empty()),
            Some("custom_message") => {
                let display = entry.get("display").and_then(Value::as_bool) != Some(false);
                let custom_type = entry.get("customType").and_then(Value::as_str);
                display
                    && matches!(
                        custom_type,
                        Some(
                            "compaction" | "pi-web:subagent-notification" | "subagent-notification"
                        )
                    )
            }
            _ => false,
        };
        if anchor {
            turns += 1;
        }
        current = entry_parent_id(entry).and_then(|id| by_id.get(id.as_str()).copied());
    }
    turns
}

pub fn session_settings(
    entries: &[Value],
    leaf_id: Option<&str>,
) -> (String, Option<ContextModel>) {
    let branch = slice_active_branch(entries, leaf_id, entries.len().max(1), false);
    let mut thinking = None;
    let mut response_model = None;
    for entry in branch.iter().rev() {
        if thinking.is_none() && entry_type(entry) == Some("thinking_level_change") {
            thinking = entry
                .get("thinkingLevel")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
        if response_model.is_none() && entry_type(entry) == Some("message") {
            if entry.pointer("/message/role").and_then(Value::as_str) == Some("assistant") {
                let provider = entry.pointer("/message/provider").and_then(Value::as_str);
                let model = entry.pointer("/message/model").and_then(Value::as_str);
                if let (Some(provider), Some(model)) = (provider, model) {
                    response_model = Some(ContextModel {
                        provider: provider.to_string(),
                        model_id: model.to_string(),
                    });
                }
            }
        }
        if thinking.is_some() && response_model.is_some() {
            break;
        }
    }
    let model = branch
        .iter()
        .rev()
        .find(|entry| entry_type(entry) == Some("model_change"))
        .and_then(|entry| {
            Some(ContextModel {
                provider: entry.get("provider")?.as_str()?.to_string(),
                model_id: entry.get("modelId")?.as_str()?.to_string(),
            })
        })
        .or(response_model);
    (thinking.unwrap_or_else(|| "off".to_string()), model)
}

pub fn compute_stats(entries: &[Value]) -> SessionStats {
    let mut stats = SessionStats {
        user_messages: 0,
        assistant_messages: 0,
        tool_calls: 0,
        tool_results: 0,
        total_messages: 0,
        tokens: TokenStats::default(),
        cost: 0.0,
    };
    for entry in entries {
        match entry_type(entry) {
            Some("compaction" | "branch_summary" | "usage") => {
                add_usage(&mut stats, entry.get("usage"));
            }
            Some("message") => {
                stats.total_messages += 1;
                match entry.pointer("/message/role").and_then(Value::as_str) {
                    Some("user") => stats.user_messages += 1,
                    Some("toolResult") => {
                        stats.tool_results += 1;
                        add_usage(&mut stats, entry.pointer("/message/usage"));
                    }
                    Some("assistant") => {
                        stats.assistant_messages += 1;
                        if let Some(content) =
                            entry.pointer("/message/content").and_then(Value::as_array)
                        {
                            stats.tool_calls += content
                                .iter()
                                .filter(|block| {
                                    block.get("type").and_then(Value::as_str) == Some("toolCall")
                                })
                                .count();
                        }
                        add_usage(&mut stats, entry.pointer("/message/usage"));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    stats.tokens.total = stats.tokens.input
        + stats.tokens.output
        + stats.tokens.cache_read
        + stats.tokens.cache_write;
    stats
}

fn add_usage(stats: &mut SessionStats, usage: Option<&Value>) {
    let Some(usage) = usage else { return };
    stats.tokens.input += usage.get("input").and_then(Value::as_f64).unwrap_or(0.0);
    stats.tokens.output += usage.get("output").and_then(Value::as_f64).unwrap_or(0.0);
    stats.tokens.cache_read += usage
        .get("cacheRead")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    stats.tokens.cache_write += usage
        .get("cacheWrite")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    stats.cost += usage
        .pointer("/cost/total")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
}

pub fn total_active_ms(entries: &[Value]) -> i64 {
    let mut total = 0i64;
    let mut previous = None;
    for entry in entries {
        let kind = entry_type(entry).unwrap_or("");
        if !matches!(
            kind,
            "message" | "compaction" | "branch_summary" | "custom_message"
        ) {
            continue;
        }
        let Some(timestamp) = entry
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.timestamp_millis())
        else {
            continue;
        };
        let role = entry.pointer("/message/role").and_then(Value::as_str);
        if matches!(role, Some("user" | "bashExecution")) {
            previous = Some(timestamp);
            continue;
        }
        if let Some(previous) = previous {
            if timestamp > previous {
                total += timestamp - previous;
            }
        }
        previous = Some(timestamp);
    }
    total
}

pub fn parse_timestamp(entry: &Value) -> Option<i64> {
    entry
        .pointer("/message/timestamp")
        .and_then(Value::as_i64)
        .or_else(|| {
            entry
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.timestamp_millis())
        })
}

pub fn format_time(time: SystemTime) -> String {
    let datetime: DateTime<Utc> = time.into();
    datetime.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn short_id() -> String {
    uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_cwd_like_pi() {
        assert_eq!(encode_for_test("/home/me/repo"), "--home-me-repo--");
    }

    fn encode_for_test(cwd: &str) -> String {
        crate::config::encode_cwd(Path::new(cwd))
    }

    #[test]
    fn builds_a_long_linear_tree_without_fake_roots() {
        let entries: Vec<Value> = (0..600)
            .map(|index| {
                let id = format!("e{index}");
                let parent = if index == 0 {
                    Value::Null
                } else {
                    Value::String(format!("e{}", index - 1))
                };
                let role = if index % 2 == 0 { "user" } else { "assistant" };
                json!({
                    "type": "message",
                    "id": id,
                    "parentId": parent,
                    "message": { "role": role, "content": "x" }
                })
            })
            .collect();
        let tree = build_tree(&entries);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].entry["id"], "e0");
        // The linear tail is compressed into one node, not repeated as roots.
        let mut nodes = 1;
        let mut current = &tree[0];
        while let Some(child) = current.children.first() {
            nodes += 1;
            current = child;
        }
        assert!(nodes <= 2, "expected a compressed chain, got {nodes} nodes");
    }

    #[test]
    fn turns_before_counts_user_turns_and_compactions_only() {
        let mut entries = Vec::new();
        for turn in 0..10 {
            let user = format!("u{turn}");
            let assistant = format!("a{turn}");
            entries.push(json!({
                "type": "message", "id": user,
                "parentId": if turn == 0 { Value::Null } else { Value::String(format!("a{}", turn - 1)) },
                "message": { "role": "user", "content": "x" }
            }));
            entries.push(json!({
                "type": "message", "id": assistant, "parentId": user,
                "message": { "role": "assistant", "content": "x" }
            }));
        }
        let context = build_context("s", &entries, Some("a9"), 2, None, false, false);
        assert_eq!(context.entry_ids, vec!["u9", "a9"]);
        assert_eq!(context.turns_before, 9);
    }

    #[test]
    fn builds_active_branch() {
        let entries = vec![
            json!({"type":"message","id":"a","parentId":null,"message":{"role":"user","content":"hi"}}),
            json!({"type":"message","id":"b","parentId":"a","message":{"role":"assistant","content":[{"type":"text","text":"hello"}]}}),
        ];
        let context = build_context("s", &entries, Some("b"), 50, None, false, false);
        assert_eq!(context.entry_ids, vec!["a", "b"]);
        assert!(!context.has_more);
    }
}
