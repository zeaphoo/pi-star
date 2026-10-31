pub mod harness_backend;
pub mod harness_env;
pub mod provider;
pub mod session_store;
pub mod tools;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::{OnceCell, broadcast, oneshot};
use tokio_util::sync::CancellationToken;

use crate::agent::harness_backend::HarnessBackend;
use crate::config::Config;
use crate::file_access::AllowedRoots;
use crate::models::{ModelDef, find_model, load_models};
use crate::session::{
    append_lines, build_context, compute_stats, create_session_file, entry_id, entry_type,
    message_text, new_entry, read_session, session_settings, short_id, total_active_ms,
};
use provider::{ProviderBlock, ProviderDelta, ProviderMessage, ToolSchema, estimate_cost};

#[derive(Debug, Clone)]
pub struct PromptRequest {
    pub message: String,
    pub images: Vec<(String, String)>,
    pub streaming_behavior: Option<String>,
}

pub(crate) struct PendingExtensionUi {
    pub sender: Option<oneshot::Sender<Value>>,
}

struct LiveState {
    entries: Vec<Value>,
    leaf_id: Option<String>,
    model: Option<ModelDef>,
    thinking: String,
    auto_compaction: bool,
    active_tools: Vec<String>,
    is_streaming: bool,
    is_prompt_running: bool,
    is_compacting: bool,
    system_prompt: String,
    last_usage: Value,
    pending: Vec<PromptRequest>,
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
    pending_extension_ui: Mutex<HashMap<String, PendingExtensionUi>>,
    state: Mutex<LiveState>,
    sender: broadcast::Sender<Value>,
    running: AtomicBool,
    cancel: Mutex<Option<CancellationToken>>,
    last_activity: Mutex<Instant>,
    subagent_slots: Mutex<usize>,
    mcp: tokio::sync::Mutex<Option<crate::mcp_runtime::McpRuntime>>,
    mcp_tool_list: Mutex<Vec<(String, String, Value)>>,
    started_at: Instant,
    /// Harness-backed engine (`PI_STAR_AGENT_BACKEND=merged`), initialized on
    /// the first async command.
    harness: OnceCell<Arc<HarnessBackend>>,
    merged: bool,
}

pub struct AgentRegistry {
    runtimes: Mutex<HashMap<String, Arc<SessionRuntime>>>,
    completion: Mutex<Option<Arc<dyn Fn(&str, Option<&str>) + Send + Sync>>>,
    max_subagents: Mutex<usize>,
    pub config: Config,
    pub roots: Arc<AllowedRoots>,
    pub client: reqwest::Client,
    merged: bool,
}

impl AgentRegistry {
    pub fn new(config: Config, roots: Arc<AllowedRoots>) -> Self {
        // The merged engine is the only in-process backend. `pi-rpc`/`pi`
        // select the subprocess backend in the server state before this
        // registry is used.
        Self::with_backend(config, roots, true)
    }

    pub fn with_backend(config: Config, roots: Arc<AllowedRoots>, merged: bool) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            runtimes: Mutex::new(HashMap::new()),
            completion: Mutex::new(None),
            max_subagents: Mutex::new(10),
            config,
            roots,
            client,
            merged,
        }
    }

    pub fn get(&self, id: &str) -> Option<Arc<SessionRuntime>> {
        self.runtimes.lock().get(id).cloned()
    }

    pub fn running_ids(&self) -> Vec<String> {
        self.runtimes
            .lock()
            .iter()
            .filter(|(_, runtime)| runtime.running.load(Ordering::SeqCst))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Every session the registry still holds a runtime for.
    pub fn ids(&self) -> Vec<String> {
        self.runtimes.lock().keys().cloned().collect()
    }

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
        let (mut thinking, model_ref) = session_settings(&session.entries, None);
        let leaf_id = session.leaf_id();
        if !session
            .entries
            .iter()
            .any(|entry| entry_type(entry) == Some("thinking_level_change"))
        {
            if let Some(level) = crate::settings::read_settings(&self.config)
                .get("defaultThinkingLevel")
                .and_then(Value::as_str)
            {
                thinking = level.to_string();
            }
        }
        let model = model_ref
            .and_then(|model| {
                find_model(&self.config, &model.provider, &model.model_id)
                    .ok()
                    .flatten()
            })
            .or_else(|| {
                load_models(&self.config).ok().and_then(|data| {
                    data.default_model.and_then(|model| {
                        find_model(&self.config, &model.provider, &model.model_id)
                            .ok()
                            .flatten()
                    })
                })
            });
        let active_tools = tools::names();
        let system_prompt =
            build_system_prompt(&self.config, Path::new(session.cwd()), &active_tools);
        let runtime = Arc::new(SessionRuntime {
            id: id.clone(),
            path: path.to_path_buf(),
            cwd: PathBuf::from(session.cwd()),
            config: self.config.clone(),
            roots: self.roots.clone(),
            client: self.client.clone(),
            registry: Arc::downgrade(self),
            max_subagents: Mutex::new(*self.max_subagents.lock()),
            pending_extension_ui: Mutex::new(HashMap::new()),
            state: Mutex::new(LiveState {
                entries: session.entries,
                leaf_id,
                model,
                thinking,
                auto_compaction: true,
                active_tools,
                is_streaming: false,
                is_prompt_running: false,
                is_compacting: false,
                system_prompt,
                last_usage: json!({}),
                pending: Vec::new(),
            }),
            sender: broadcast::channel(512).0,
            running: AtomicBool::new(false),
            cancel: Mutex::new(None),
            last_activity: Mutex::new(Instant::now()),
            subagent_slots: Mutex::new(0),
            mcp: tokio::sync::Mutex::new(None),
            mcp_tool_list: Mutex::new(Vec::new()),
            started_at: Instant::now(),
            harness: OnceCell::new(),
            merged: self.merged,
        });
        self.runtimes.lock().insert(id, runtime.clone());
        Ok(runtime)
    }

    pub fn create(self: &Arc<Self>, cwd: &Path) -> Result<Arc<SessionRuntime>> {
        self.roots.add(cwd);
        let path = create_session_file(&self.config, cwd)?;
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

    /// Idle sessions are closed, never deleted: the JSONL stays readable and
    /// the next request opens it again.
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
        let runtime_ids: Vec<String> = self.runtimes.lock().keys().cloned().collect();
        for id in runtime_ids {
            if let Some(runtime) = self.get(&id) {
                *runtime.max_subagents.lock() = max.max(1);
            }
        }
    }
}

impl SessionRuntime {
    pub async fn subscribe(&self) -> Result<broadcast::Receiver<Value>> {
        if self.merged {
            return Ok(self.harness().await?.subscribe());
        }
        Ok(self.sender.subscribe())
    }

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

    pub fn is_running(&self) -> bool {
        if let Some(harness) = self.harness.get() {
            return harness.is_running();
        }
        self.running.load(Ordering::SeqCst)
    }

    /// The transcript the wrapper holds, which may be newer than the file.
    pub fn entries_snapshot(&self) -> Vec<Value> {
        if let Some(harness) = self.harness.get() {
            return harness.entries_snapshot();
        }
        self.state.lock().entries.clone()
    }

    /// Replaces the wrapper's transcript with a fresh read of the session file
    /// (a mount/refresh read, `?force=1`).
    pub fn replace_entries(&self, entries: Vec<Value>) {
        self.state.lock().entries = entries;
    }

    pub fn state_json(&self, include_prompt: bool) -> Value {
        if let Some(harness) = self.harness.get() {
            return harness.state_json(include_prompt);
        }
        let state = self.state.lock();
        let model = state.model.as_ref().map(|model| {
            json!({
                "provider": model.provider,
                "id": model.id,
            })
        });
        let context_window = state
            .model
            .as_ref()
            .and_then(|m| m.context_window)
            .unwrap_or(0);
        let mut value = json!({
            "running": true,
            "model": model,
            "thinkingLevel": state.thinking,
            "isStreaming": state.is_streaming,
            "isPromptRunning": state.is_prompt_running,
            "isBashRunning": false,
            "isCompacting": state.is_compacting,
            "autoCompactionEnabled": state.auto_compaction,
            "extensionStatuses": [],
            "extensionWidgets": [],
            "queuedMessages": {
                "steering": state.pending.iter().filter(|p| p.streaming_behavior.as_deref() == Some("steer")).map(|p| p.message.clone()).collect::<Vec<_>>(),
                "followUp": state.pending.iter().filter(|p| p.streaming_behavior.as_deref() != Some("steer")).map(|p| p.message.clone()).collect::<Vec<_>>(),
            },
            "contextUsage": {
                "percent": Value::Null,
                "contextWindow": context_window,
                "tokens": Value::Null,
            },
            "totalActiveMs": total_active_ms(&state.entries),
            "lastUsage": state.last_usage,
        });
        if include_prompt {
            value["systemPrompt"] = Value::String(state.system_prompt.clone());
        }
        value
    }

    pub fn tool_entries(&self) -> Value {
        let state = self.state.lock();
        let mut entries: Vec<Value> = tools::schemas()
            .into_iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "active": state.active_tools.iter().any(|name| name == &tool.name),
                    "parameters": tool.parameters,
                })
            })
            .collect();
        for (name, description, parameters) in self.mcp_tool_list.lock().iter() {
            entries.push(json!({
                "name": name,
                "description": description,
                "active": true,
                "parameters": parameters,
            }));
        }
        Value::Array(entries)
    }

    pub fn session_stats(&self) -> Value {
        let state = self.state.lock();
        let stats = compute_stats(&state.entries);
        json!({
            "sessionFile": self.path.to_string_lossy(),
            "sessionId": self.id,
            "userMessages": stats.user_messages,
            "assistantMessages": stats.assistant_messages,
            "toolCalls": stats.tool_calls,
            "toolResults": stats.tool_results,
            "totalMessages": stats.total_messages,
            "tokens": stats.tokens,
            "cost": stats.cost,
            "totalActiveMs": total_active_ms(&state.entries),
        })
    }

    fn is_child_session(&self) -> bool {
        crate::session::read_header(&self.path)
            .ok()
            .and_then(|header| header.parent_session)
            .is_some()
    }

    pub fn session_name(&self) -> Option<String> {
        if let Some(harness) = self.harness.get() {
            return harness.session_name();
        }
        let state = self.state.lock();
        state
            .entries
            .iter()
            .rev()
            .find(|entry| entry_type(entry) == Some("session_info"))
            .and_then(|entry| entry.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    pub fn last_assistant_text(&self) -> Option<String> {
        let state = self.state.lock();
        state
            .entries
            .iter()
            .rev()
            .find(|entry| {
                entry_type(entry) == Some("message")
                    && entry.pointer("/message/role").and_then(Value::as_str) == Some("assistant")
            })
            .map(|entry| message_text(&entry.pointer("/message").cloned().unwrap_or(Value::Null)))
    }

    pub fn abort(&self) -> Value {
        if let Some(harness) = self.harness.get() {
            let harness = harness.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _ = harness.send(&json!({"type": "abort"})).await;
                });
            }
            return json!({ "steering": [], "followUp": [] });
        }
        if let Some(cancel) = self.cancel.lock().take() {
            cancel.cancel();
        }
        let mut state = self.state.lock();
        let steering = state
            .pending
            .iter()
            .filter(|p| p.streaming_behavior.as_deref() == Some("steer"))
            .map(|p| p.message.clone())
            .collect::<Vec<_>>();
        let follow_up = state
            .pending
            .iter()
            .filter(|p| p.streaming_behavior.as_deref() != Some("steer"))
            .map(|p| p.message.clone())
            .collect::<Vec<_>>();
        state.pending.clear();
        json!({ "steering": steering, "followUp": follow_up })
    }

    pub fn clear_queue(&self) -> Value {
        let mut state = self.state.lock();
        let steering = state
            .pending
            .iter()
            .filter(|p| p.streaming_behavior.as_deref() == Some("steer"))
            .map(|p| p.message.clone())
            .collect::<Vec<_>>();
        let follow_up = state
            .pending
            .iter()
            .filter(|p| p.streaming_behavior.as_deref() != Some("steer"))
            .map(|p| p.message.clone())
            .collect::<Vec<_>>();
        state.pending.clear();
        json!({ "steering": steering, "followUp": follow_up })
    }

    pub async fn send(self: &Arc<Self>, command: &Value) -> Result<Value> {
        if self.merged {
            return self.harness().await?.send(command).await;
        }
        self.send_legacy(command)
    }

    fn send_legacy(self: &Arc<Self>, command: &Value) -> Result<Value> {
        self.touch();
        let command_type = command.get("type").and_then(Value::as_str).unwrap_or("");
        match command_type {
            "get_state" => Ok(self.state_json(true)),
            "get_tools" => Ok(self.tool_entries()),
            "get_commands" => Ok(json!({ "commands": [] })),
            "get_session_stats" => Ok(self.session_stats()),
            "get_last_assistant_text" => Ok(json!({ "text": self.last_assistant_text() })),
            "prompt" => {
                let message = prompt_text(command);
                let images = prompt_images(command);
                let request = PromptRequest {
                    message,
                    images,
                    streaming_behavior: command
                        .get("streamingBehavior")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                };
                if self.is_running() {
                    self.state.lock().pending.push(request);
                    return Ok(json!({ "queued": true }));
                }
                self.start_run(vec![request]);
                Ok(json!({ "queued": false }))
            }
            "abort" => Ok(self.abort()),
            "clear_queue" => Ok(self.clear_queue()),
            "set_model" => {
                let provider = command
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let model_id = command.get("modelId").and_then(Value::as_str).unwrap_or("");
                let Some(model) = find_model(&self.config, provider, model_id)? else {
                    return Err(anyhow!("Model not found: {provider}/{model_id}"));
                };
                self.append_entry(
                    "model_change",
                    json!({
                        "provider": model.provider,
                        "modelId": model.id,
                    }),
                )?;
                self.state.lock().model = Some(model.clone());
                Ok(json!({ "provider": model.provider, "id": model.id }))
            }
            "set_thinking_level" => {
                let level = command
                    .get("level")
                    .and_then(Value::as_str)
                    .unwrap_or("off");
                self.append_entry("thinking_level_change", json!({ "thinkingLevel": level }))?;
                self.state.lock().thinking = level.to_string();
                Ok(json!({ "level": level }))
            }
            "set_auto_compaction" => {
                let enabled = command
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                self.state.lock().auto_compaction = enabled;
                Ok(json!({ "enabled": enabled }))
            }
            "set_session_name" => {
                let name = command
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                self.append_entry("session_info", json!({ "name": name }))?;
                Ok(json!({ "name": name }))
            }
            "compact" => self.compact(),
            "abort_compaction" => {
                self.state.lock().is_compacting = false;
                Ok(json!({ "cancelled": true }))
            }
            "navigate_tree" => {
                let target = command
                    .get("targetId")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if target.is_empty() {
                    return Err(anyhow!("targetId is required"));
                }
                self.state.lock().leaf_id = Some(target.to_string());
                Ok(json!({ "cancelled": false }))
            }
            "set_tools" => {
                let names = command
                    .get("toolNames")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_else(tools::names);
                self.state.lock().active_tools = names;
                Ok(json!({ "sessionId": self.id, "recreated": false }))
            }
            "reload" => Ok(self.state_json(true)),
            "fork" | "clone" => self.fork(),
            "bash" => self.run_bash(command),
            "abort_bash" => Ok(json!({ "cancelled": true })),
            "extension_ui_response" | "extension_ui_input" => {
                let id = command.get("id").and_then(Value::as_str).unwrap_or("");
                let response = command
                    .get("response")
                    .cloned()
                    .unwrap_or_else(|| command.clone());
                if let Some(pending) = self.pending_extension_ui.lock().remove(id) {
                    if let Some(sender) = pending.sender {
                        let _ = sender.send(response);
                    }
                }
                Ok(Value::Null)
            }
            other => Err(anyhow!("unknown command: {other}")),
        }
    }

    fn append_entry(&self, kind: &str, extra: Value) -> Result<Value> {
        let mut state = self.state.lock();
        let extra = extra.as_object().cloned().unwrap_or_default();
        let entry = new_entry(kind, state.leaf_id.as_deref(), extra);
        append_lines(&self.path, &[entry.clone()])?;
        state.leaf_id = entry_id(&entry);
        state.entries.push(entry.clone());
        Ok(entry)
    }

    fn emit(&self, event: Value) {
        self.touch();
        let _ = self.sender.send(event);
    }

    pub fn pending_extension_ui_ids(&self) -> Vec<String> {
        self.pending_extension_ui.lock().keys().cloned().collect()
    }

    /// Ask the browser a blocking extension question. The caller gets the
    /// response, or None when the deadline passes or the request is abandoned.
    pub async fn request_extension_ui(&self, request: Value, timeout: Duration) -> Option<Value> {
        let Some(id) = request.get("id").and_then(Value::as_str).map(str::to_owned) else {
            return None;
        };
        let (sender, receiver) = oneshot::channel();
        self.pending_extension_ui.lock().insert(
            id.clone(),
            PendingExtensionUi {
                sender: Some(sender),
            },
        );
        self.emit(json!({
            "type": "extension_ui_request",
            "id": id,
            "method": request.get("method").cloned().unwrap_or(Value::Null),
            "request": request,
        }));
        let response = tokio::select! {
            value = receiver => value.ok(),
            _ = tokio::time::sleep(timeout) => None,
        };
        self.pending_extension_ui.lock().remove(&id);
        response
    }

    pub fn touch(&self) {
        *self.last_activity.lock() = Instant::now();
    }

    pub fn idle_for(&self) -> Duration {
        self.last_activity.lock().elapsed()
    }

    fn start_run(self: &Arc<Self>, prompts: Vec<PromptRequest>) {
        if self.running.swap(true, Ordering::SeqCst) {
            let mut state = self.state.lock();
            state.pending.extend(prompts);
            return;
        }
        let runtime = self.clone();
        tokio::spawn(async move {
            let mut queue = prompts;
            loop {
                let Some(prompt) = queue.first().cloned() else {
                    break;
                };
                queue.remove(0);
                let cancel = CancellationToken::new();
                *runtime.cancel.lock() = Some(cancel.clone());
                let result = runtime.run_prompt(prompt, cancel.clone()).await;
                let aborted = cancel.is_cancelled();
                if let Err(error) = result {
                    runtime.emit(json!({
                        "type": "prompt_error",
                        "errorMessage": error.to_string(),
                    }));
                }
                let mut state = runtime.state.lock();
                state.is_streaming = false;
                state.is_prompt_running = false;
                if aborted {
                    state.pending.clear();
                }
                if queue.is_empty() {
                    queue = std::mem::take(&mut state.pending);
                }
                drop(state);
                runtime.emit(json!({
                    "type": "agent_end",
                }));
                runtime.emit(json!({
                    "type": "agent_settled",
                    "aborted": aborted,
                    "subagentsActive": false,
                }));
                runtime.emit(json!({
                    "type": "prompt_done",
                    "aborted": aborted,
                    "subagentsActive": false,
                }));
                if !aborted && !runtime.is_child_session() {
                    let callback = runtime
                        .registry
                        .upgrade()
                        .and_then(|registry| registry.completion.lock().clone());
                    if let Some(callback) = callback {
                        let name = runtime.session_name();
                        callback(&runtime.id, name.as_deref());
                    }
                }
            }
            *runtime.cancel.lock() = None;
            runtime.running.store(false, Ordering::SeqCst);
        });
    }

    async fn run_prompt(&self, prompt: PromptRequest, cancel: CancellationToken) -> Result<()> {
        self.emit(json!({ "type": "agent_start" }));
        {
            let mut state = self.state.lock();
            state.is_prompt_running = true;
            state.is_streaming = true;
        }
        let user_entry = self.append_entry(
            "message",
            json!({
                "message": {
                    "role": "user",
                    "content": prompt_content(&prompt),
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                }
            }),
        )?;
        self.emit(json!({ "type": "message_start", "message": { "role": "user" } }));
        self.emit(json!({
            "type": "message_end",
            "message": user_entry.get("message").cloned().unwrap_or(Value::Null),
        }));

        self.ensure_mcp().await;
        for _ in 0..24 {
            if cancel.is_cancelled() {
                return Ok(());
            }
            let mcp_schemas = self.mcp_tool_schemas().await;
            let (model, system, messages, tool_schemas) = {
                let state = self.state.lock();
                let Some(model) = state.model.clone() else {
                    return Err(anyhow!(
                        "No model selected. Configure a provider in Settings › Models."
                    ));
                };
                let messages = provider_messages(&state.entries);
                let mut tool_schemas = tools::schemas()
                    .into_iter()
                    .filter(|tool| state.active_tools.iter().any(|name| name == &tool.name))
                    .map(|tool| ToolSchema {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.parameters,
                    })
                    .collect::<Vec<_>>();
                for (name, description, parameters) in mcp_schemas.clone() {
                    tool_schemas.push(ToolSchema {
                        name,
                        description,
                        parameters,
                    });
                }
                (model, state.system_prompt.clone(), messages, tool_schemas)
            };
            let max_tokens = model.max_tokens.unwrap_or(8192);
            let mut stream = provider::stream_chat(
                self.client.clone(),
                model.clone(),
                system,
                messages,
                tool_schemas,
                max_tokens,
            );
            let mut blocks: BTreeMap<usize, AssistantBlock> = BTreeMap::new();
            let mut usage = Usage::default();
            let mut error_message: Option<String> = None;
            self.emit(json!({
                "type": "message_start",
                "message": {
                    "role": "assistant",
                    "content": [],
                    "provider": model.provider,
                    "model": model.id,
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                }
            }));
            loop {
                let delta = tokio::select! {
                    _ = cancel.cancelled() => break,
                    item = stream.next() => item,
                };
                let Some(delta) = delta else { break };
                match delta {
                    ProviderDelta::TextStart { index } => {
                        blocks
                            .entry(index)
                            .or_insert_with(|| AssistantBlock::Text(String::new()));
                        self.emit(json!({
                            "type": "message_update",
                            "assistantMessageEvent": { "type": "text_start", "contentIndex": index }
                        }));
                    }
                    ProviderDelta::TextDelta { index, text } => {
                        if let Some(AssistantBlock::Text(current)) = blocks.get_mut(&index) {
                            current.push_str(&text);
                        }
                        self.emit(json!({
                            "type": "message_update",
                            "assistantMessageEvent": { "type": "text_delta", "contentIndex": index, "delta": text }
                        }));
                    }
                    ProviderDelta::TextEnd { index, text } => {
                        blocks.insert(index, AssistantBlock::Text(text.clone()));
                        self.emit(json!({
                            "type": "message_update",
                            "assistantMessageEvent": { "type": "text_end", "contentIndex": index, "content": text }
                        }));
                    }
                    ProviderDelta::ThinkingStart { index } => {
                        blocks
                            .entry(index)
                            .or_insert_with(|| AssistantBlock::Thinking(String::new()));
                        self.emit(json!({
                            "type": "message_update",
                            "assistantMessageEvent": { "type": "thinking_start", "contentIndex": index }
                        }));
                    }
                    ProviderDelta::ThinkingDelta { index, text } => {
                        if let Some(AssistantBlock::Thinking(current)) = blocks.get_mut(&index) {
                            current.push_str(&text);
                        }
                        self.emit(json!({
                            "type": "message_update",
                            "assistantMessageEvent": { "type": "thinking_delta", "contentIndex": index, "delta": text }
                        }));
                    }
                    ProviderDelta::ThinkingEnd { index, text } => {
                        blocks.insert(index, AssistantBlock::Thinking(text.clone()));
                        self.emit(json!({
                            "type": "message_update",
                            "assistantMessageEvent": { "type": "thinking_end", "contentIndex": index, "content": text }
                        }));
                    }
                    ProviderDelta::ToolCallStart { index, id, name } => {
                        blocks.insert(
                            index,
                            AssistantBlock::ToolCall {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: String::new(),
                            },
                        );
                        self.emit(json!({
                            "type": "message_update",
                            "assistantMessageEvent": { "type": "toolcall_start", "contentIndex": index, "id": id, "toolName": name }
                        }));
                    }
                    ProviderDelta::ToolCallDelta { index, delta } => {
                        if let Some(AssistantBlock::ToolCall { arguments, .. }) =
                            blocks.get_mut(&index)
                        {
                            arguments.push_str(&delta);
                        }
                        self.emit(json!({
                            "type": "message_update",
                            "assistantMessageEvent": { "type": "toolcall_delta", "contentIndex": index, "delta": delta }
                        }));
                    }
                    ProviderDelta::ToolCallEnd {
                        index,
                        id,
                        name,
                        arguments,
                    } => {
                        let parsed =
                            provider::parse_arguments(&arguments).unwrap_or_else(|_| json!({}));
                        blocks.insert(
                            index,
                            AssistantBlock::ToolCall {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: arguments.clone(),
                            },
                        );
                        self.emit(json!({
                            "type": "message_update",
                            "assistantMessageEvent": {
                                "type": "toolcall_end",
                                "contentIndex": index,
                                "toolCall": { "id": id, "name": name, "arguments": parsed }
                            }
                        }));
                    }
                    ProviderDelta::Usage {
                        input,
                        output,
                        cache_read,
                        cache_write,
                        ..
                    } => {
                        usage.input = input;
                        usage.output = output;
                        usage.cache_read = cache_read;
                        usage.cache_write = cache_write;
                    }
                    ProviderDelta::Error { message } => {
                        error_message = Some(message);
                    }
                    ProviderDelta::Done => {}
                }
            }
            if let Some(message) = error_message {
                self.emit(json!({ "type": "prompt_error", "errorMessage": message }));
                return Err(anyhow!(message));
            }
            if cancel.is_cancelled() {
                self.emit(json!({ "type": "agent_end" }));
                return Ok(());
            }
            let content = blocks_to_content(&blocks);
            let tool_calls = blocks
                .values()
                .filter_map(|block| match block {
                    AssistantBlock::ToolCall {
                        id,
                        name,
                        arguments,
                    } => Some((id.clone(), name.clone(), arguments.clone())),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let mut message = json!({
                "role": "assistant",
                "content": content,
                "provider": model.provider,
                "model": model.id,
                "timestamp": chrono::Utc::now().timestamp_millis(),
                "stopReason": if tool_calls.is_empty() { "end" } else { "toolUse" },
                "usage": usage.to_json(&model),
            });
            if crate::transcript_notices::enabled(&self.config, &self.cwd) {
                let notices = {
                    let state = self.state.lock();
                    crate::transcript_notices::live_assistant_notices(&state.entries, &message)
                };
                if !notices.is_empty() {
                    message["transcriptNotices"] = Value::Array(notices);
                }
            }
            let entry = self.append_entry("message", json!({ "message": message }))?;
            self.emit(json!({
                "type": "message_end",
                "message": entry.get("message").cloned().unwrap_or(Value::Null),
            }));
            {
                let mut state = self.state.lock();
                state.last_usage = usage.to_json(&model);
                state.is_streaming = false;
            }
            if tool_calls.is_empty() {
                return Ok(());
            }
            for (id, name, arguments) in tool_calls {
                if cancel.is_cancelled() {
                    return Ok(());
                }
                self.execute_tool(&id, &name, &arguments).await?;
            }
        }
        Err(anyhow!("agent exceeded the maximum number of tool turns"))
    }

    async fn execute_tool(&self, id: &str, name: &str, arguments: &str) -> Result<()> {
        let args: Value = provider::parse_arguments(arguments).unwrap_or_else(|_| json!({}));
        self.emit(json!({
            "type": "tool_execution_start",
            "toolCallId": id,
            "toolName": name,
        }));
        let output = if name == "subagent" {
            self.execute_subagent(id, name, &args).await
        } else if name.starts_with("mcp__") {
            self.execute_mcp_tool(name, &args).await
        } else {
            tools::execute(&self.roots, &self.cwd, name, &args, |partial| {
                self.emit(json!({
                    "type": "tool_execution_update",
                    "toolCallId": id,
                    "toolName": name,
                    "partialResult": {"content":[{"type":"text","text":partial}]},
                }));
            })
            .await
        };
        let entry = self.append_entry(
            "message",
            json!({
                "message": {
                    "role": "toolResult",
                    "toolCallId": id,
                    "toolName": name,
                    "content": [{"type":"text","text": output.content}],
                    "isError": output.is_error,
                    "details": output.details,
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                }
            }),
        )?;
        self.emit(json!({
            "type": "tool_execution_end",
            "toolCallId": id,
            "toolName": name,
            "isError": output.is_error,
        }));
        self.emit(json!({ "type": "message_start", "message": { "role": "toolResult" } }));
        self.emit(json!({
            "type": "message_end",
            "message": entry.get("message").cloned().unwrap_or(Value::Null),
        }));
        Ok(())
    }

    async fn ensure_mcp(&self) {
        let mut runtime = self.mcp.lock().await;
        if runtime.is_none() {
            let connected = crate::mcp_runtime::McpRuntime::connect(&self.config, &self.cwd).await;
            *self.mcp_tool_list.lock() = connected.tool_schemas();
            *runtime = Some(connected);
        }
    }

    async fn mcp_tool_schemas(&self) -> Vec<(String, String, Value)> {
        let mut runtime = self.mcp.lock().await;
        if runtime.is_none() {
            let connected = crate::mcp_runtime::McpRuntime::connect(&self.config, &self.cwd).await;
            *self.mcp_tool_list.lock() = connected.tool_schemas();
            *runtime = Some(connected);
        }
        runtime
            .as_ref()
            .map(crate::mcp_runtime::McpRuntime::tool_schemas)
            .unwrap_or_default()
    }

    async fn execute_mcp_tool(&self, name: &str, args: &Value) -> tools::ToolOutput {
        let mut runtime = self.mcp.lock().await;
        if runtime.is_none() {
            *runtime = Some(crate::mcp_runtime::McpRuntime::connect(&self.config, &self.cwd).await);
        }
        match runtime.as_mut() {
            Some(runtime) => match runtime.call(name, args.clone()).await {
                Ok(content) => tools::ToolOutput {
                    content,
                    is_error: false,
                    details: json!({"mcp": true}),
                },
                Err(error) => tools::ToolOutput {
                    content: error.to_string(),
                    is_error: true,
                    details: json!({"mcp": true}),
                },
            },
            None => tools::ToolOutput {
                content: "MCP is not available".into(),
                is_error: true,
                details: json!({}),
            },
        }
    }

    async fn execute_subagent(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        args: &Value,
    ) -> tools::ToolOutput {
        let description = args
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("Subagent")
            .chars()
            .take(200)
            .collect::<String>();
        let prompt = args
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if prompt.is_empty() {
            return tools::ToolOutput {
                content: "prompt is required".into(),
                is_error: true,
                details: json!({}),
            };
        }
        let Some(registry) = self.registry.upgrade() else {
            return tools::ToolOutput {
                content: "the agent registry is unavailable".into(),
                is_error: true,
                details: json!({}),
            };
        };
        {
            let max = *self.max_subagents.lock();
            let mut active = self.subagent_slots.lock();
            if *active >= max {
                return tools::ToolOutput {
                    content: format!("too many subagents are already running (max {max})"),
                    is_error: true,
                    details: json!({}),
                };
            }
            *active += 1;
        }
        let _slot = SubagentSlot(&self.subagent_slots);
        let path = match crate::session::create_session_file_with_parent(
            &self.config,
            &self.cwd,
            Some(&self.path),
        ) {
            Ok(path) => path,
            Err(error) => {
                return tools::ToolOutput {
                    content: error.to_string(),
                    is_error: true,
                    details: json!({}),
                };
            }
        };
        let session_id = match crate::session::read_session(&path) {
            Ok(session) => session.header.id.clone(),
            Err(error) => {
                return tools::ToolOutput {
                    content: error.to_string(),
                    is_error: true,
                    details: json!({}),
                };
            }
        };
        let meta = crate::session::new_entry(
            "custom",
            None,
            [
                (
                    "customType".to_string(),
                    Value::String("pi-web:subagent".into()),
                ),
                (
                    "data".to_string(),
                    json!({
                        "parentSessionId": self.id,
                        "profile": "default",
                        "description": description,
                        "status": "running",
                    }),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let _ = crate::session::append_lines(&path, &[meta]);
        let child = match registry.open_path(&path) {
            Ok(child) => child,
            Err(error) => {
                return tools::ToolOutput {
                    content: error.to_string(),
                    is_error: true,
                    details: json!({}),
                };
            }
        };
        let Ok(mut receiver) = child.subscribe().await else {
            return tools::ToolOutput {
                content: "The subagent session could not be started.".to_string(),
                is_error: true,
                details: json!({}),
            };
        };
        let _ = child
            .send(&json!({
                "type": "prompt",
                "message": prompt,
            }))
            .await;
        let mut streamed = String::new();
        let deadline = tokio::time::sleep(std::time::Duration::from_secs(600));
        tokio::pin!(deadline);
        let mut finished = false;
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                event = receiver.recv() => {
                    let Ok(event) = event else { break };
                    match event.get("type").and_then(Value::as_str) {
                        Some("message_update") => {
                            if let Some(delta) = event.pointer("/assistantMessageEvent/delta").and_then(Value::as_str) {
                                streamed.push_str(delta);
                                if streamed.len() > 4000 {
                                    let mut cut = streamed.len() - 4000;
                                    while !streamed.is_char_boundary(cut) { cut += 1; }
                                    streamed.drain(..cut);
                                }
                                self.emit(json!({
                                    "type": "tool_execution_update",
                                    "toolCallId": tool_call_id,
                                    "toolName": tool_name,
                                    "partialResult": {"content":[{"type":"text","text":streamed}]},
                                }));
                            }
                        }
                        Some("prompt_done") => {
                            finished = true;
                            break;
                        }
                        Some("prompt_error") => {
                            finished = false;
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
        let text = child
            .last_assistant_text()
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| {
                if finished {
                    "(subagent finished without a text reply)".into()
                } else {
                    "(subagent did not finish)".into()
                }
            });
        let status = if finished { "completed" } else { "failed" };
        let status_entry = crate::session::new_entry(
            "custom",
            None,
            [
                (
                    "customType".to_string(),
                    Value::String("pi-web:subagent-status".into()),
                ),
                (
                    "data".to_string(),
                    json!({"parentSessionId": self.id, "status": status}),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let _ = crate::session::append_lines(&path, &[status_entry]);
        tools::ToolOutput {
            content: format!("[{description}] {text}"),
            is_error: !finished,
            details: json!({
                "subagentSessionId": session_id,
                "description": description,
                "status": status,
            }),
        }
    }

    fn compact(&self) -> Result<Value> {
        let state = self.state.lock();
        let tokens_before = compute_stats(&state.entries).tokens.total;
        let summary = format!(
            "Conversation compacted. The previous {} entries are summarized here. Continue from the most recent user request.",
            state.entries.len()
        );
        let first_kept = state.leaf_id.clone().unwrap_or_default();
        drop(state);
        let entry = self.append_entry(
            "compaction",
            json!({
                "summary": summary,
                "firstKeptEntryId": first_kept,
                "tokensBefore": tokens_before,
            }),
        )?;
        let tokens_after = self.state.lock().entries.len() as f64;
        Ok(json!({
            "reason": "manual",
            "tokensBefore": tokens_before,
            "estimatedTokensAfter": tokens_after,
            "entry": entry,
        }))
    }

    fn fork(&self) -> Result<Value> {
        let state = self.state.lock();
        let leaf = state.leaf_id.clone();
        let context = build_context(
            &self.id,
            &state.entries,
            leaf.as_deref(),
            10_000,
            None,
            false,
            false,
        );
        let ids = context.entry_ids;
        let source_entries: Vec<Value> = state
            .entries
            .iter()
            .filter(|entry| entry_id(entry).is_some_and(|id| ids.contains(&id)))
            .cloned()
            .collect();
        drop(state);
        let path = create_session_file(&self.config, &self.cwd)?;
        let new_session = read_session(&path)?;
        let new_id = new_session.id().to_string();
        append_lines(&path, &source_entries)?;
        Ok(json!({ "cancelled": false, "newSessionId": new_id }))
    }

    fn run_bash(&self, command: &Value) -> Result<Value> {
        let script = command.get("command").and_then(Value::as_str).unwrap_or("");
        if script.is_empty() {
            return Err(anyhow!("command is required"));
        }
        let output =
            std::process::Command::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()))
                .arg("-lc")
                .arg(script)
                .current_dir(&self.cwd)
                .output()?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr);
        let entry = self.append_entry(
            "message",
            json!({
                "message": {
                    "role": "bashExecution",
                    "command": script,
                    "output": text,
                    "exitCode": output.status.code(),
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                }
            }),
        )?;
        self.emit(json!({ "type": "message_start", "message": { "role": "bashExecution" } }));
        self.emit(json!({
            "type": "message_end",
            "message": entry.get("message").cloned().unwrap_or(Value::Null),
        }));
        Ok(json!({ "output": text, "exitCode": output.status.code() }))
    }

    pub fn debug_snapshot(&self) -> Value {
        if let Some(harness) = self.harness.get() {
            return harness.debug_snapshot();
        }
        let state = self.state.lock();
        json!({
            "sessionId": self.id,
            "sessionFile": self.path.to_string_lossy(),
            "cwd": self.cwd.to_string_lossy(),
            "running": self.running.load(Ordering::SeqCst),
            "entries": state.entries.len(),
            "model": state.model.as_ref().map(|model| json!({"provider": model.provider, "id": model.id})),
            "thinkingLevel": state.thinking,
            "activeTools": state.active_tools,
            "uptimeMs": self.started_at.elapsed().as_millis() as u64,
        })
    }
}

struct SubagentSlot<'a>(&'a parking_lot::Mutex<usize>);

impl Drop for SubagentSlot<'_> {
    fn drop(&mut self) {
        let mut active = self.0.lock();
        *active = active.saturating_sub(1);
    }
}

#[derive(Debug, Clone)]
enum AssistantBlock {
    Text(String),
    Thinking(String),
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
}

#[derive(Debug, Clone, Default)]
struct Usage {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

impl Usage {
    fn to_json(&self, model: &ModelDef) -> Value {
        let cost = estimate_cost(
            model,
            self.input,
            self.output,
            self.cache_read,
            self.cache_write,
        );
        json!({
            "input": self.input,
            "output": self.output,
            "cacheRead": self.cache_read,
            "cacheWrite": self.cache_write,
            "cost": {
                "input": 0,
                "output": 0,
                "cacheRead": 0,
                "cacheWrite": 0,
                "total": cost,
            }
        })
    }
}

fn blocks_to_content(blocks: &BTreeMap<usize, AssistantBlock>) -> Value {
    Value::Array(
        blocks
            .values()
            .map(|block| match block {
                AssistantBlock::Text(text) => json!({ "type": "text", "text": text }),
                AssistantBlock::Thinking(thinking) => {
                    json!({ "type": "thinking", "thinking": thinking })
                }
                AssistantBlock::ToolCall {
                    id,
                    name,
                    arguments,
                } => json!({
                    "type": "toolCall",
                    "toolCallId": id,
                    "toolName": name,
                    "input": provider::parse_arguments(arguments).unwrap_or_else(|_| json!({})),
                }),
            })
            .collect(),
    )
}

fn provider_messages(entries: &[Value]) -> Vec<ProviderMessage> {
    let mut messages = Vec::new();
    for entry in entries {
        if entry_type(entry) != Some("message") {
            continue;
        }
        let Some(message) = entry.get("message") else {
            continue;
        };
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        if role == "system" || role == "bashExecution" {
            continue;
        }
        let mut blocks = Vec::new();
        let mut tool_call_id = None;
        match role {
            "user" | "custom" => {
                for block in content_blocks(message) {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => blocks.push(ProviderBlock::Text(
                            block
                                .get("text")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string(),
                        )),
                        Some("image") => {
                            let source = block.get("source").cloned().unwrap_or(Value::Null);
                            if source.get("type").and_then(Value::as_str) == Some("base64") {
                                if let Some(data) = source.get("data").and_then(Value::as_str) {
                                    blocks.push(ProviderBlock::Image {
                                        media_type: source
                                            .get("media_type")
                                            .and_then(Value::as_str)
                                            .unwrap_or("image/png")
                                            .to_string(),
                                        data: data.to_string(),
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            "assistant" => {
                for block in content_blocks(message) {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => blocks.push(ProviderBlock::Text(
                            block
                                .get("text")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string(),
                        )),
                        Some("thinking") => blocks.push(ProviderBlock::Thinking(
                            block
                                .get("thinking")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string(),
                        )),
                        Some("toolCall") => {
                            let id = block
                                .get("toolCallId")
                                .or_else(|| block.get("id"))
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let name = block
                                .get("toolName")
                                .or_else(|| block.get("name"))
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let arguments =
                                block.get("input").cloned().unwrap_or_else(|| json!({}));
                            blocks.push(ProviderBlock::ToolCall {
                                id,
                                name,
                                arguments,
                            });
                        }
                        _ => {}
                    }
                }
            }
            "toolResult" => {
                tool_call_id = message
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let content = content_blocks(message)
                    .iter()
                    .filter_map(|block| {
                        if block.get("type").and_then(Value::as_str) == Some("text") {
                            block.get("text").and_then(Value::as_str).map(str::to_owned)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                blocks.push(ProviderBlock::ToolResult {
                    id: tool_call_id.clone().unwrap_or_default(),
                    content,
                    is_error: message
                        .get("isError")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
            }
            _ => {}
        }
        if !blocks.is_empty() {
            messages.push(ProviderMessage {
                role: role.to_string(),
                content: blocks,
                tool_call_id,
            });
        }
    }
    messages
}

fn content_blocks(message: &Value) -> Vec<Value> {
    match message.get("content") {
        Some(Value::String(text)) => vec![json!({"type":"text","text":text})],
        Some(Value::Array(blocks)) => blocks.clone(),
        _ => Vec::new(),
    }
}

pub(crate) fn prompt_text(command: &Value) -> String {
    match command.get("message") {
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

fn prompt_content(prompt: &PromptRequest) -> Value {
    let mut content = Vec::new();
    if !prompt.message.is_empty() {
        content.push(json!({ "type": "text", "text": prompt.message }));
    }
    for (mime, data) in &prompt.images {
        content.push(json!({
            "type": "image",
            "source": { "type": "base64", "media_type": mime, "data": data }
        }));
    }
    Value::Array(content)
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

pub fn new_session_id() -> String {
    short_id()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn merged_backend_serves_get_state_through_the_harness() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config::new(Some(temp.path().join("agent")), None).unwrap();
        std::fs::create_dir_all(&config.agent_dir).unwrap();
        std::fs::write(
            config.models_path(),
            json!({
                "providers": {
                    "faux": {
                        "models": [{
                            "id": "faux-model",
                            "name": "Faux",
                            "api": "faux",
                            "contextWindow": 1000,
                            "maxTokens": 100
                        }]
                    }
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            config.settings_path(),
            json!({"defaultProvider": "faux", "defaultModel": "faux-model"}).to_string(),
        )
        .unwrap();
        let cwd = temp.path().join("proj");
        std::fs::create_dir_all(&cwd).unwrap();
        let registry = Arc::new(AgentRegistry::with_backend(
            config.clone(),
            Arc::new(AllowedRoots::new()),
            true,
        ));
        let runtime = registry.create(&cwd).unwrap();
        let state = runtime.send(&json!({"type": "get_state"})).await.unwrap();
        assert_eq!(state["model"]["provider"], "faux");
        assert_eq!(state["model"]["id"], "faux-model");
        assert!(
            state["systemPrompt"]
                .as_str()
                .is_some_and(|prompt| prompt.contains("coding agent"))
        );
        let _receiver = runtime.subscribe().await.unwrap();
        assert!(!runtime.is_running());
    }
}
