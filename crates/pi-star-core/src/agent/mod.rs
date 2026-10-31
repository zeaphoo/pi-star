//! Agent façade: the in-process backend is the merged `AgentHarness`.
//!
//! `SessionRuntime` keeps the command/state surface the HTTP routes and the UI
//! depend on; all execution, tools, MCP, subagents and persistence live in
//! [`HarnessBackend`]. `pi-rpc` is selected in the server state before this
//! registry is used, so no second in-process runtime lives here.

pub mod harness_backend;
pub mod harness_env;
pub mod provider;
pub mod session_store;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::{OnceCell, broadcast};

use crate::config::Config;
use crate::file_access::AllowedRoots;
use crate::session::{read_session, session_settings};

use harness_backend::HarnessBackend;

pub struct AgentRegistry {
    runtimes: Mutex<HashMap<String, Arc<SessionRuntime>>>,
    completion: Mutex<Option<Arc<dyn Fn(&str, Option<&str>) + Send + Sync>>>,
    max_subagents: Mutex<usize>,
    pub config: Config,
    pub roots: Arc<AllowedRoots>,
    pub client: reqwest::Client,
}

impl AgentRegistry {
    pub fn new(config: Config, roots: Arc<AllowedRoots>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            runtimes: Mutex::new(HashMap::new()),
            completion: Mutex::new(None),
            max_subagents: Mutex::new(10),
            config,
            roots,
            client,
        }
    }

    pub fn get(&self, id: &str) -> Option<Arc<SessionRuntime>> {
        self.runtimes.lock().get(id).cloned()
    }

    pub fn running_ids(&self) -> Vec<String> {
        self.runtimes
            .lock()
            .iter()
            .filter(|(_, runtime)| runtime.is_running())
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn ids(&self) -> Vec<String> {
        self.runtimes.lock().keys().cloned().collect()
    }

    /// The merged backend has no extension UI, so no session awaits browser
    /// input.
    pub fn awaiting_input_ids(&self) -> Vec<String> {
        Vec::new()
    }

    pub fn open_or_create(self: &Arc<Self>, id: &str) -> Result<Arc<SessionRuntime>> {
        if let Some(runtime) = self.get(id) {
            return Ok(runtime);
        }
        let path = crate::session::resolve_session_path(&self.config, id)
            .ok_or_else(|| anyhow!("Session not found"))?;
        self.open_path(&path)
    }

    pub fn open_path(self: &Arc<Self>, path: &Path) -> Result<Arc<SessionRuntime>> {
        let session = read_session(path)?;
        let id = session.id().to_string();
        if let Some(runtime) = self.get(&id) {
            return Ok(runtime);
        }
        self.roots.add(Path::new(session.cwd()));
        let runtime = Arc::new(SessionRuntime {
            id: id.clone(),
            path: path.to_path_buf(),
            cwd: PathBuf::from(session.cwd()),
            config: self.config.clone(),
            roots: self.roots.clone(),
            client: self.client.clone(),
            registry: Arc::downgrade(self),
            max_subagents: Mutex::new(*self.max_subagents.lock()),
            harness: OnceCell::new(),
            last_activity: Mutex::new(Instant::now()),
        });
        self.runtimes.lock().insert(id, runtime.clone());
        Ok(runtime)
    }

    pub fn create(self: &Arc<Self>, cwd: &Path) -> Result<Arc<SessionRuntime>> {
        self.roots.add(cwd);
        let path = crate::session::create_session_file(&self.config, cwd)?;
        self.open_path(&path)
    }

    pub fn remove(&self, id: &str) {
        if let Some(runtime) = self.runtimes.lock().remove(id) {
            runtime.abort();
        }
    }

    pub fn set_completion_callback(&self, callback: Arc<dyn Fn(&str, Option<&str>) + Send + Sync>) {
        *self.completion.lock() = Some(callback);
    }

    pub fn shutdown(&self) {
        let runtimes: Vec<Arc<SessionRuntime>> = self.runtimes.lock().values().cloned().collect();
        for runtime in runtimes {
            runtime.abort();
        }
    }

    pub fn reap_idle(&self, timeout: Duration) -> Vec<String> {
        if timeout.is_zero() {
            return Vec::new();
        }
        let idle: Vec<String> = self
            .runtimes
            .lock()
            .iter()
            .filter(|(_, runtime)| !runtime.is_running() && runtime.idle_for() >= timeout)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &idle {
            self.remove(id);
        }
        idle
    }

    pub fn max_subagents(&self) -> usize {
        *self.max_subagents.lock()
    }

    pub fn set_max_subagents(&self, max: usize) {
        let max = max.max(1);
        *self.max_subagents.lock() = max;
        for runtime in self.runtimes.lock().values() {
            *runtime.max_subagents.lock() = max;
        }
    }
}

pub struct SessionRuntime {
    pub id: String,
    pub path: PathBuf,
    pub cwd: PathBuf,
    pub config: Config,
    pub roots: Arc<AllowedRoots>,
    pub client: reqwest::Client,
    registry: Weak<AgentRegistry>,
    max_subagents: Mutex<usize>,
    harness: OnceCell<Arc<HarnessBackend>>,
    last_activity: Mutex<Instant>,
}

impl SessionRuntime {
    async fn harness(&self) -> Result<&Arc<HarnessBackend>> {
        self.harness
            .get_or_try_init(|| async {
                HarnessBackend::open(
                    &self.config,
                    self.roots.clone(),
                    &self.path,
                    self.registry.clone(),
                )
                .await
            })
            .await
    }

    pub async fn subscribe(&self) -> Result<broadcast::Receiver<Value>> {
        Ok(self.harness().await?.subscribe())
    }

    pub fn is_running(&self) -> bool {
        self.harness
            .get()
            .map(|harness| harness.is_running())
            .unwrap_or(false)
    }

    pub fn entries_snapshot(&self) -> Vec<Value> {
        read_session(&self.path)
            .map(|session| session.entries)
            .unwrap_or_default()
    }

    /// The v3 file is the source of truth; a forced refresh keeps the cached
    /// harness state, so this is intentionally a no-op.
    pub fn replace_entries(&self, _entries: Vec<Value>) {}

    pub fn state_json(&self, include_prompt: bool) -> Value {
        if let Some(harness) = self.harness.get() {
            return harness.state_json(include_prompt);
        }
        let name = self.session_name();
        let model = session_settings(&self.entries_snapshot(), None)
            .1
            .map(|model| json!({ "provider": model.provider, "id": model.model_id }));
        let mut value = json!({
            "running": false,
            "sessionId": self.id,
            "sessionFile": self.path.to_string_lossy(),
            "sessionName": name,
            "model": model,
            "thinkingLevel": "off",
            "isStreaming": false,
            "isPromptRunning": false,
            "isBashRunning": false,
            "isCompacting": false,
            "autoCompactionEnabled": true,
            "extensionStatuses": [],
            "extensionWidgets": [],
            "queuedMessages": { "steering": [], "followUp": [] },
            "contextUsage": { "percent": Value::Null, "contextWindow": 0, "tokens": Value::Null },
            "totalActiveMs": 0,
            "lastUsage": {},
        });
        if include_prompt {
            value["systemPrompt"] =
                Value::String(build_system_prompt(&self.config, &self.cwd, &tool_names()));
        }
        value
    }

    pub fn tool_entries(&self) -> Value {
        Value::Array(
            tool_names()
                .into_iter()
                .map(|name| json!({ "name": name, "description": "", "active": true, "parameters": {} }))
                .collect(),
        )
    }

    pub fn session_name(&self) -> Option<String> {
        read_session(&self.path)
            .ok()
            .and_then(|session| session.name())
    }

    pub fn last_assistant_text(&self) -> Option<String> {
        last_assistant_text(&self.path)
    }

    pub fn abort(&self) -> Value {
        if let Some(harness) = self.harness.get() {
            let harness = harness.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _ = harness.send(&json!({"type": "abort"})).await;
                });
            }
        }
        json!({ "steering": [], "followUp": [] })
    }

    pub fn clear_queue(&self) -> Value {
        json!({ "steering": [], "followUp": [] })
    }

    pub fn pending_extension_ui_ids(&self) -> Vec<String> {
        Vec::new()
    }

    pub fn debug_snapshot(&self) -> Value {
        if let Some(harness) = self.harness.get() {
            return harness.debug_snapshot();
        }
        json!({
            "sessionId": self.id,
            "sessionFile": self.path.to_string_lossy(),
            "cwd": self.cwd.to_string_lossy(),
            "running": false,
            "entries": self.entries_snapshot().len(),
            "model": session_settings(&self.entries_snapshot(), None)
                .1
                .map(|model| json!({ "provider": model.provider, "id": model.model_id })),
            "thinkingLevel": "off",
            "activeTools": tool_names(),
            "uptimeMs": 0,
        })
    }

    pub fn touch(&self) {
        *self.last_activity.lock() = Instant::now();
    }

    pub fn idle_for(&self) -> Duration {
        self.last_activity.lock().elapsed()
    }

    pub fn session_stats(&self) -> Value {
        let entries = self.entries_snapshot();
        let count = |role: &str| {
            entries
                .iter()
                .filter(|entry| {
                    entry.get("type").and_then(Value::as_str) == Some("message")
                        && entry.pointer("/message/role").and_then(Value::as_str) == Some(role)
                })
                .count()
        };
        json!({
            "sessionFile": self.path.to_string_lossy(),
            "sessionId": self.id,
            "userMessages": count("user"),
            "assistantMessages": count("assistant"),
            "toolCalls": 0,
            "toolResults": count("toolResult"),
            "totalMessages": entries.len(),
            "tokens": 0,
            "cost": 0,
            "totalActiveMs": 0,
        })
    }

    pub async fn send(self: &Arc<Self>, command: &Value) -> Result<Value> {
        self.touch();
        self.harness().await?.send(command).await
    }
}

// ---------------------------------------------------------------------------
// Prompt helpers + system prompt (shared with the harness backend)
// ---------------------------------------------------------------------------

pub(crate) fn prompt_text(command: &Value) -> String {
    match command.get("message") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| {
                (block.get("type").and_then(Value::as_str) == Some("text"))
                    .then(|| block.get("text").and_then(Value::as_str))
                    .flatten()
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub(crate) fn prompt_images(command: &Value) -> Vec<(String, String)> {
    let mut images = Vec::new();
    if let Some(items) = command.get("images").and_then(Value::as_array) {
        for item in items {
            if let (Some(data), Some(mime)) = (
                item.get("data").and_then(Value::as_str),
                item.get("mimeType").and_then(Value::as_str),
            ) {
                images.push((mime.to_string(), data.to_string()));
            }
        }
    }
    if let Some(blocks) = command.get("message").and_then(Value::as_array) {
        for block in blocks {
            if block.get("type").and_then(Value::as_str) == Some("image") {
                let source = block.get("source").cloned().unwrap_or(Value::Null);
                if let (Some(data), Some(mime)) = (
                    source.get("data").and_then(Value::as_str),
                    source.get("media_type").and_then(Value::as_str),
                ) {
                    images.push((mime.to_string(), data.to_string()));
                }
            }
        }
    }
    images
}

fn tool_names() -> Vec<String> {
    [
        "read", "write", "edit", "bash", "find", "grep", "ls", "subagent",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

pub(crate) fn build_system_prompt(config: &Config, cwd: &Path, tools: &[String]) -> String {
    let mut prompt = format!(
        "You are pi, a coding agent working in {}. Use the available tools to inspect and modify the project. \
         Prefer reading files before editing. Keep replies concise and show the user what changed. \
         Available tools: {}.",
        cwd.display(),
        tools.join(", ")
    );
    prompt.push_str(&crate::skills::prompt_section(config, cwd));
    prompt
}

fn last_assistant_text(path: &Path) -> Option<String> {
    let session = read_session(path).ok()?;
    session.entries.iter().rev().find_map(|entry| {
        (entry.get("type").and_then(Value::as_str) == Some("message")
            && entry.pointer("/message/role").and_then(Value::as_str) == Some("assistant"))
        .then(|| {
            entry
                .pointer("/message/content")
                .and_then(Value::as_array)
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|block| block.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default()
        })
    })
}
