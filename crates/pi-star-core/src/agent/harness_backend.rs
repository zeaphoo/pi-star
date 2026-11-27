//! AgentHarness-backed runtime: the QuickPI (`quick`) agent backend.
//!
//! This is the in-process engine for pi-star: it drives
//! `pi_star_harness::AgentHarness` over the pi v3 session backend and
//! translates the harness's `AgentEvent`s into the JSON events pi-web already
//! consumes. [`super::SessionRuntime`] keeps the command/state façade and
//! delegates here; the standard Node.js pi (`rpc`) backend lives in the
//! `pi-star` crate's `pi_rpc` module.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use tokio::sync::broadcast;

use pi_star_agent::agent_tool::AgentTool;
use pi_star_agent::error::AgentError;
use pi_star_agent::events::{AgentEmitter, AgentEvent};
use pi_star_agent::message::{AgentMessage, CustomMessage};
use pi_star_agent::types::{AgentToolResult, TextContentOrImage};
use pi_star_ai::model::Model;
use pi_star_ai::provider::Provider;
use pi_star_ai::providers::anthropic::AnthropicProvider;
use pi_star_ai::providers::faux::{FauxProvider, FauxScript};
use pi_star_ai::providers::openai_completions::OpenAiCompletionsProvider;
use pi_star_ai::providers::openai_responses::OpenAiResponsesProvider;
use pi_star_ai::types::{ImageContent, Schema, TextContent, ThinkingLevel, Tool, ToolCall};
use pi_star_harness::agent_harness::{AgentHarness, AgentLane};
use pi_star_harness::session::Session;
use pi_star_harness::session::types::SessionMetadata;
use pi_star_harness::types::{
    AgentHarnessOptions, AgentHarnessResources, AgentHarnessStreamOptions, CompactionSettings,
    DrivingMode, HarnessTool, HarnessToolExecution, RetryPolicy,
};
use pi_star_tools::tools::ExecutionToolContext;

use crate::agent::harness_env::AllowedEnv;
use crate::agent::session_store::{V3SessionRepo, message_to_v3};
use crate::agent::{AgentRegistry, SessionRuntime};
use crate::config::Config;
use crate::file_access::AllowedRoots;
use crate::models::{ModelDef, find_model, load_models};
use crate::session::{read_header, read_session, session_settings};

/// Shared event buffer size; mirrors the legacy `SessionRuntime` channel.
const EVENT_BUFFER: usize = 512;

/// Internal marker for a loadout with no tools: the harness treats an
/// empty `active_tool_names` as "all tools".
const NO_TOOLS: &str = "__pi_star_no_tools__";

/// What a subagent's profile changes about a child's prompt and tools. The
/// default is a plain session: no profile text, the skill catalog, MCP on.
#[derive(Debug, Clone, Default)]
pub struct HarnessResources {
    pub prompt: crate::prompt::PromptResources,
    /// `mcp: off` skips the MCP tool bridge entirely.
    pub mcp: bool,
}

impl HarnessResources {
    pub fn new(prompt: crate::prompt::PromptResources, mcp: bool) -> Self {
        Self { prompt, mcp }
    }
}

pub struct HarnessBackend {
    pub id: String,
    pub path: PathBuf,
    pub cwd: PathBuf,
    pub config: Config,
    pub roots: Arc<AllowedRoots>,
    lane: Arc<dyn AgentLane>,
    harness: AgentHarness,
    session: Session,
    events: broadcast::Sender<Value>,
    running: AtomicBool,
    model: parking_lot::RwLock<Option<ModelDef>>,
    thinking: parking_lot::RwLock<String>,
    system_prompt: parking_lot::RwLock<String>,
    queue: parking_lot::RwLock<(Vec<String>, Vec<String>)>,
    auto_compaction: AtomicBool,
    /// The registry that owns this runtime, for the subagent controller (Stop
    /// propagation) without taking it through every command.
    registry: Weak<AgentRegistry>,
    resources: HarnessResources,
}

impl HarnessBackend {
    /// Open an existing session file with the configured/default model.
    pub async fn open(
        config: &Config,
        roots: Arc<AllowedRoots>,
        path: &Path,
        registry: Weak<AgentRegistry>,
    ) -> Result<Arc<Self>> {
        Self::open_with(config, roots, path, registry, HarnessResources::default()).await
    }

    pub async fn open_with(
        config: &Config,
        roots: Arc<AllowedRoots>,
        path: &Path,
        registry: Weak<AgentRegistry>,
        resources: HarnessResources,
    ) -> Result<Arc<Self>> {
        let read_path = path.to_path_buf();
        let (header, session_file) = tokio::task::spawn_blocking(move || -> Result<_> {
            let header = read_header(&read_path)?;
            let session_file = read_session(&read_path)?;
            Ok((header, session_file))
        })
        .await
        .map_err(|error| anyhow!("session read task failed: {error}"))??;
        let cwd = PathBuf::from(&header.cwd);
        roots.add(&cwd);
        let model_config = config.clone();
        let providers_cwd = cwd.clone();
        let (model, providers) = tokio::task::spawn_blocking(move || -> Result<_> {
            let model = resolve_model(&model_config, &session_file.entries).ok_or_else(|| {
                anyhow!("No model selected. Configure a provider in Settings › Models.")
            })?;
            let providers = build_providers(&model_config, &providers_cwd)?;
            Ok((model, providers))
        })
        .await
        .map_err(|error| anyhow!("session model task failed: {error}"))??;
        Self::build(
            config, roots, path, cwd, header.id, model, providers, None, registry, resources,
        )
        .await
    }

    /// Create a new v3 session in `cwd` and open it.
    pub async fn create(
        config: &Config,
        roots: Arc<AllowedRoots>,
        cwd: &Path,
        registry: Weak<AgentRegistry>,
    ) -> Result<Arc<Self>> {
        roots.add(cwd);
        let repo = V3SessionRepo::new(config.clone(), cwd);
        use pi_star_harness::session::types::SessionRepo;
        let storage = repo
            .create(&pi_star_harness::session::types::SessionCreateOptions::default())
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        let metadata = storage.metadata();
        use pi_star_harness::session::types::SessionStorage;
        drop(storage);
        let path = crate::config::session_file_for_id(&config.sessions_dir, &metadata.id)
            .ok_or_else(|| anyhow!("created session file was not found"))?;
        Self::open(config, roots, &path, registry).await
    }

    /// Build from explicit model + providers (used by tests and, later, by the
    /// registry when it already resolved the model).
    #[allow(clippy::too_many_arguments)]
    pub async fn build(
        config: &Config,
        roots: Arc<AllowedRoots>,
        path: &Path,
        cwd: PathBuf,
        id: String,
        model_def: ModelDef,
        providers: Vec<Arc<dyn Provider>>,
        system_prompt: Option<String>,
        registry: Weak<AgentRegistry>,
        resources: HarnessResources,
    ) -> Result<Arc<Self>> {
        let repo = V3SessionRepo::new(config.clone(), &cwd);
        use pi_star_harness::session::types::SessionRepo;
        let metadata = SessionMetadata {
            id: id.clone(),
            created_at: 0,
            parent_session_id: None,
        };
        let storage = repo
            .open(&metadata)
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        let session = Session::new(Arc::new(storage), None);

        let tools = build_tools(
            &cwd,
            roots.clone(),
            config,
            path,
            registry.clone(),
            resources.mcp,
        )
        .await;
        let tool_names: Vec<String> = tools
            .iter()
            .map(|tool| tool.tool.schema().name.clone())
            .collect();
        let selection_path = path.to_path_buf();
        let selected_tools =
            tokio::task::spawn_blocking(move || stored_tool_selection(&selection_path))
                .await
                .ok()
                .flatten();
        let has_explicit_selection = selected_tools.is_some();
        let visible_tools: Vec<String> = match selected_tools {
            Some(selected) => selected,
            None => {
                let config = config.clone();
                let cwd = cwd.clone();
                tokio::task::spawn_blocking(move || {
                    crate::prompt::default_tool_names(&config, &cwd)
                })
                .await
                .unwrap_or_default()
            }
        }
        .into_iter()
        .filter(|name| tool_names.iter().any(|known| known == name))
        .collect();
        // The subagent tools are active on registration when the feature is on,
        // as pi-web's `Agent` is: the model does not have to be told to switch
        // them on first. An explicit tool selection still wins.
        let mut visible_tools: Vec<String> = visible_tools;
        if !has_explicit_selection && crate::settings::subagent_settings(config).built_in_enabled {
            for name in ["subagent", "get_subagent_result", "steer_subagent"] {
                if tool_names.iter().any(|known| known == name)
                    && !visible_tools.iter().any(|known| known == name)
                {
                    visible_tools.push(name.to_string());
                }
            }
        }
        let active_tool_names = harness_tool_names(&visible_tools);
        let system_prompt = match system_prompt {
            Some(prompt) => prompt,
            None => {
                let prompt_config = config.clone();
                let prompt_cwd = cwd.clone();
                let prompt_tools = visible_tools.clone();
                let prompt_resources = resources.prompt.clone();
                tokio::task::spawn_blocking(move || {
                    crate::prompt::build_system_prompt_with(
                        &prompt_config,
                        &prompt_cwd,
                        &prompt_tools,
                        &prompt_resources,
                    )
                })
                .await
                .unwrap_or_default()
            }
        };
        let stored_prompt = system_prompt.clone();
        let thinking_path = path.to_path_buf();
        let thinking_config = config.clone();
        let thinking_cwd = cwd.clone();
        let thinking = tokio::task::spawn_blocking(move || {
            read_session(&thinking_path)
                .ok()
                .and_then(|file| {
                    file.entries
                        .iter()
                        .any(|entry| {
                            entry.get("type").and_then(Value::as_str)
                                == Some("thinking_level_change")
                        })
                        .then(|| session_settings(&file.entries, None).0)
                })
                .unwrap_or_else(|| resolve_thinking(&thinking_config, &thinking_cwd))
        })
        .await
        .unwrap_or_default();
        let model = model_from_def(&model_def);
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        let emitter = Arc::new(WireEmitter {
            events: events.clone(),
        });

        let options = AgentHarnessOptions {
            model,
            thinking_level: parse_thinking(&thinking),
            active_tool_names,
            tools,
            system_prompt: Some(system_prompt),
            resources: AgentHarnessResources::default(),
            stream_options: AgentHarnessStreamOptions::default(),
            retry: RetryPolicy::default(),
            compaction: CompactionSettings::default(),
            steering_mode: Default::default(),
            follow_up_mode: Default::default(),
            tool_execution: HarnessToolExecution::default(),
            drive: DrivingMode::default(),
            session,
            allow_existing_session: true,
            models: providers,
            to_provider_messages: None,
            entry_projectors: BTreeMap::new(),
            agent_emitter: Some(emitter),
            before_tool_call: None,
            after_tool_call: None,
            transform_context: None,
            entry_transforms: Vec::new(),
            provider_hooks: None,
        };
        let harness = AgentHarness::create(options)
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        let lane = harness.lane("main");
        let session = harness.session().clone();
        Ok(Arc::new(Self {
            id,
            path: path.to_path_buf(),
            cwd,
            config: config.clone(),
            roots,
            lane,
            harness,
            session,
            events,
            running: AtomicBool::new(false),
            model: parking_lot::RwLock::new(Some(model_def)),
            thinking: parking_lot::RwLock::new(thinking),
            system_prompt: parking_lot::RwLock::new(stored_prompt),
            queue: parking_lot::RwLock::new((Vec::new(), Vec::new())),
            auto_compaction: AtomicBool::new(true),
            registry,
            resources,
        }))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.events.subscribe()
    }

    /// The lane's queued steering and follow-up messages. A subagent report is
    /// only steered into a parent whose queue is empty, so it is never reordered
    /// behind something the user typed.
    pub async fn queued_messages(&self) -> Result<Value> {
        let queued = self
            .lane
            .queued_messages()
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        Ok(json!({
            "steering": queued.steering,
            "followUp": queued.follow_up,
        }))
    }

    /// Deliver a custom message to this session with pi's `deliverAs` semantics.
    /// Used for a background subagent's report: steer into a running parent, or
    /// a follow-up turn when it is idle.
    pub async fn send_custom_message(
        &self,
        custom_type: &str,
        text: &str,
        details: Value,
        deliver_as: pi_star_harness::types::MessageDelivery,
        trigger_turn: Option<bool>,
    ) -> Result<Value> {
        let message = AgentMessage::Custom(CustomMessage {
            role: custom_type.to_string(),
            content: vec![pi_star_ai::types::Content::text(text)],
            data: details,
            timestamp: now_ms(),
        });
        let result = self
            .lane
            .send_message(
                message,
                pi_star_harness::types::SendMessageOptions {
                    trigger_turn,
                    deliver_as,
                },
            )
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        Ok(match result {
            pi_star_harness::agent_harness::SendMessageResult::Appended { entry_id } => {
                json!({ "delivered": "appended", "entryId": entry_id })
            }
            pi_star_harness::agent_harness::SendMessageResult::Queued { entry_id, .. } => {
                json!({ "delivered": "queued", "entryId": entry_id })
            }
            pi_star_harness::agent_harness::SendMessageResult::Run(_) => {
                json!({ "delivered": "run" })
            }
        })
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn emit(&self, event: Value) {
        let _ = self.events.send(event);
    }

    pub fn entries_snapshot(&self) -> Vec<Value> {
        // Session reads are authoritative; the route refreshes from the file
        // anyway. Keeping this cheap avoids cloning the harness state.
        read_session(&self.path)
            .map(|session| session.entries)
            .unwrap_or_default()
    }

    fn context_usage(&self) -> Value {
        let Ok(file) = read_session(&self.path) else {
            return json!({ "percent": Value::Null, "contextWindow": 0, "tokens": Value::Null });
        };
        let mut entries = Vec::new();
        for (index, value) in file.entries.iter().enumerate() {
            if let Ok(entry) = crate::agent::session_store::v3_to_entry(value, index as u64 + 1) {
                entries.push(entry);
            }
        }
        let by_id: std::collections::HashMap<&str, &pi_star_harness::session::types::Entry> =
            entries.iter().map(|entry| (entry.id(), entry)).collect();
        let mut path = Vec::new();
        let mut current = entries.last().map(|entry| entry.id().to_string());
        while let Some(id) = current {
            let Some(entry) = by_id.get(id.as_str()) else {
                break;
            };
            path.push((*entry).clone());
            current = entry.parent_id().map(str::to_owned);
        }
        path.reverse();
        let context =
            pi_star_harness::session::context::build_session_context(&path, &Default::default());
        let estimate =
            pi_star_harness::compaction::tokens::estimate_context_tokens(&context.messages);
        let window = self
            .model
            .read()
            .as_ref()
            .and_then(|model| model.context_window)
            .unwrap_or(0);
        let percent = (window > 0)
            .then(|| ((estimate.tokens as f64 / window as f64) * 1000.0).round() / 10.0);
        json!({
            "percent": percent,
            "contextWindow": window,
            "tokens": estimate.tokens,
        })
    }

    pub fn state_json(&self, include_prompt: bool) -> Value {
        let model = self.model.read().clone();
        let thinking = self.thinking.read().clone();
        let running = self.is_running();
        let mut value = json!({
            "running": running,
            "sessionId": self.id,
            "sessionFile": self.path.to_string_lossy(),
            "sessionName": self.session_name(),
            "model": model.as_ref().map(|model| json!({
                "provider": model.provider,
                "id": model.id,
            })),
            "thinkingLevel": thinking,
            "isStreaming": running,
            "isPromptRunning": running,
            "isBashRunning": false,
            "isCompacting": false,
            "autoCompactionEnabled": self.auto_compaction.load(Ordering::SeqCst),
            "extensionStatuses": [],
            "extensionWidgets": [],
            "queuedMessages": {
                "steering": self.queue.read().0.clone(),
                "followUp": self.queue.read().1.clone(),
            },
            "contextUsage": self.context_usage(),
            "totalActiveMs": 0,
            "lastUsage": {},
        });
        if include_prompt {
            value["systemPrompt"] = Value::String(self.system_prompt.read().clone());
        }
        value
    }

    /// Append a pi-native `usage` entry (`cache_warm`, extension-reported usage,
    /// …). Returns the entry id.
    pub async fn append_usage(
        &self,
        kind: &str,
        provider: &str,
        model: &str,
        usage: pi_star_ai::types::Usage,
        note: Option<&str>,
    ) -> Result<String> {
        self.session
            .append_usage(kind, provider, model, usage, note)
            .await
            .map_err(|error| anyhow!(error.to_string()))
    }

    pub fn debug_snapshot(&self) -> Value {
        json!({
            "sessionId": self.id,
            "sessionFile": self.path.to_string_lossy(),
            "cwd": self.cwd.to_string_lossy(),
            "running": self.is_running(),
            "entries": read_session(&self.path).map(|session| session.entries.len()).unwrap_or(0),
            "model": self.model.read().as_ref().map(|model| json!({
                "provider": model.provider,
                "id": model.id,
            })),
            "thinkingLevel": self.thinking.read().clone(),
            "activeTools": [],
            "uptimeMs": 0,
        })
    }

    pub fn tool_entries(&self) -> Value {
        // Tool schemas are served by `get_tools` through the lane; the façade's
        // synchronous view is kept for compatibility until route wiring lands.
        Value::Array(Vec::new())
    }

    pub fn session_name(&self) -> Option<String> {
        read_session(&self.path)
            .ok()
            .and_then(|session| session.name())
    }

    async fn emit_queue(&self) {
        if let Ok(queued) = self.lane.queued_messages().await {
            *self.queue.write() = (queued.steering.clone(), queued.follow_up.clone());
            self.emit(json!({
                "type": "queue_update",
                "steering": queued.steering,
                "followUp": queued.follow_up,
            }));
        }
    }

    /// Handle one pi-web agent command.
    pub async fn send(self: &Arc<Self>, command: &Value) -> Result<Value> {
        let kind = command.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "get_state" => {
                let backend = self.clone();
                tokio::task::spawn_blocking(move || backend.state_json(true))
                    .await
                    .map_err(|error| anyhow!("state task failed: {error}"))
            }
            "get_session_stats" => {
                let stats = self
                    .lane
                    .get_stats()
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                let stats_path = self.path.clone();
                let entries = tokio::task::spawn_blocking(move || {
                    read_session(&stats_path)
                        .map(|session| session.entries)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
                let role = |entry: &Value| {
                    entry
                        .pointer("/message/role")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                };
                let user_messages = entries
                    .iter()
                    .filter(|entry| {
                        entry.get("type").and_then(Value::as_str) == Some("message")
                            && role(entry) == "user"
                    })
                    .count();
                let assistant_messages = entries
                    .iter()
                    .filter(|entry| {
                        entry.get("type").and_then(Value::as_str) == Some("message")
                            && role(entry) == "assistant"
                    })
                    .count();
                let tool_results = entries
                    .iter()
                    .filter(|entry| {
                        entry.get("type").and_then(Value::as_str) == Some("message")
                            && role(entry) == "toolResult"
                    })
                    .count();
                let tool_calls: usize = entries
                    .iter()
                    .filter(|entry| role(entry) == "assistant")
                    .map(|entry| {
                        entry
                            .pointer("/message/content")
                            .and_then(Value::as_array)
                            .map(|blocks| {
                                blocks
                                    .iter()
                                    .filter(|block| {
                                        block.get("type").and_then(Value::as_str)
                                            == Some("toolCall")
                                    })
                                    .count()
                            })
                            .unwrap_or(0)
                    })
                    .sum();
                let mut tokens = stats.total_tokens;
                let mut cost = stats.cost_total;
                for entry in &entries {
                    let usage = match entry.get("type").and_then(Value::as_str) {
                        Some("usage" | "compaction" | "branch_summary") => entry.get("usage"),
                        Some("message") => entry.pointer("/message/usage"),
                        _ => None,
                    };
                    if let Some(usage) = usage {
                        tokens += usage
                            .get("totalTokens")
                            .and_then(Value::as_i64)
                            .unwrap_or(0);
                        cost += usage
                            .pointer("/cost/total")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0);
                    }
                }
                Ok(json!({
                    "sessionFile": self.path.to_string_lossy(),
                    "sessionId": self.id,
                    "userMessages": user_messages,
                    "assistantMessages": assistant_messages,
                    "toolCalls": tool_calls,
                    "toolResults": tool_results,
                    "totalMessages": user_messages + assistant_messages + tool_results,
                    "tokens": tokens,
                    "cost": cost,
                    "totalActiveMs": 0,
                }))
            }
            "get_last_assistant_text" => {
                let text_path = self.path.clone();
                let entries = tokio::task::spawn_blocking(move || {
                    read_session(&text_path)
                        .map(|session| session.entries)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
                let text = entries.iter().rev().find_map(|entry| {
                    (entry.get("type").and_then(Value::as_str) == Some("message")
                        && entry.pointer("/message/role").and_then(Value::as_str)
                            == Some("assistant"))
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
                });
                Ok(json!({ "text": text }))
            }
            "get_tools" => {
                let active = self
                    .lane
                    .get_active_tools()
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                let tools = self
                    .harness
                    .get_tools()
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                let active = visible_tool_names(&active);
                Ok(Value::Array(
                    tools
                        .iter()
                        .map(|tool| {
                            let schema = tool.tool.schema();
                            let mut value = serde_json::to_value(schema).unwrap_or(Value::Null);
                            if let Some(object) = value.as_object_mut() {
                                object.insert(
                                    "active".into(),
                                    Value::Bool(active.iter().any(|name| name == &schema.name)),
                                );
                                object.insert("declarationHidden".into(), Value::Bool(false));
                                let guidelines = super::prompt_tool_guidelines(&schema.name);
                                if !guidelines.is_empty() {
                                    object.insert(
                                        "promptGuidelines".into(),
                                        serde_json::to_value(guidelines).unwrap_or(Value::Null),
                                    );
                                }
                            }
                            value
                        })
                        .collect(),
                ))
            }
            "prompt" => {
                let text = super::prompt_text(command);
                let images = super::prompt_images(command)
                    .into_iter()
                    .map(|(mime, data)| ImageContent {
                        kind: Default::default(),
                        data,
                        mime_type: mime,
                    })
                    .collect::<Vec<_>>();
                let behavior = command
                    .get("streamingBehavior")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if self.is_running() {
                    let message =
                        AgentMessage::User(pi_star_ai::types::UserMessage::new(text, now_ms()));
                    let queued = if behavior == "steer" {
                        self.lane.steer(message).await
                    } else {
                        self.lane.follow_up(message).await
                    };
                    queued.map_err(|error| anyhow!(error.to_string()))?;
                    self.emit_queue().await;
                    return Ok(json!({ "queued": true }));
                }
                let this = self.clone();
                tokio::spawn(async move {
                    this.running.store(true, Ordering::SeqCst);
                    let result = this.lane.prompt_text(&text, images).await;
                    this.running.store(false, Ordering::SeqCst);
                    let aborted = result
                        .as_ref()
                        .err()
                        .map(|error| error.to_string().contains("abort"))
                        .unwrap_or(false);
                    if let Err(error) = result {
                        this.emit(json!({
                            "type": "prompt_error",
                            "errorMessage": error.to_string(),
                        }));
                    }
                    this.emit(json!({ "type": "agent_settled", "aborted": aborted }));
                    this.emit(json!({ "type": "prompt_done", "aborted": aborted }));
                });
                Ok(json!({ "queued": false }))
            }
            "abort" => {
                // A subagent is part of this agent's run: stopping the parent
                // stops its children too.
                if let Some(controller) = self
                    .registry
                    .upgrade()
                    .and_then(|registry| registry.subagent_controller().cloned())
                {
                    controller.abort_children_now(&self.id);
                }
                let result = self
                    .lane
                    .abort()
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                self.emit_queue().await;
                Ok(json!({
                    "steering": result.steer.iter().map(queued_text).collect::<Vec<_>>(),
                    "followUp": result.follow_up.iter().map(queued_text).collect::<Vec<_>>(),
                }))
            }
            "clear_queue" => {
                let queued = self
                    .lane
                    .clear_queue()
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                self.emit_queue().await;
                Ok(json!({
                    "steering": queued.steering,
                    "followUp": queued.follow_up,
                }))
            }
            "set_model" => {
                let provider = command
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let model_id = command.get("modelId").and_then(Value::as_str).unwrap_or("");
                let def = find_model(&self.config, provider, model_id)?
                    .ok_or_else(|| anyhow!("Model not found: {provider}/{model_id}"))?;
                let providers = build_providers(&self.config, &self.cwd)?;
                let model = model_from_def(&def);
                self.harness
                    .set_models(providers)
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                self.lane
                    .set_model(model)
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                *self.model.write() = Some(def.clone());
                Ok(json!({ "provider": def.provider, "id": def.id }))
            }
            "set_thinking_level" => {
                let level = command
                    .get("level")
                    .and_then(Value::as_str)
                    .unwrap_or("off");
                self.lane
                    .set_thinking_level(parse_thinking(level))
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                *self.thinking.write() = level.to_string();
                Ok(json!({ "level": level }))
            }
            "set_tools" => {
                let names = match command.get("toolNames").and_then(Value::as_array) {
                    Some(items) => items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>(),
                    None => crate::prompt::default_tool_names(&self.config, &self.cwd),
                };
                self.lane
                    .set_active_tools(harness_tool_names(&names))
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                let selection = if command.get("toolNames").is_some() {
                    json!({"version": 1, "tools": names})
                } else {
                    // A cleared marker lets an earlier pin be retracted; the
                    // session then follows settings.json `defaultTools` again.
                    json!({"version": 1, "cleared": true})
                };
                self.harness
                    .append_custom_entry("pi-star:tool-selection", Some(selection))
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                // A subagent's profile prompt (replace/append, skills, MCP) must
                // survive a tool-selection change; rebuild with its resources.
                let prompt = crate::prompt::build_system_prompt_with(
                    &self.config,
                    &self.cwd,
                    &names,
                    &self.resources.prompt,
                );
                self.harness
                    .set_system_prompt(Some(prompt.clone()))
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                *self.system_prompt.write() = prompt;
                Ok(json!({ "sessionId": self.id, "recreated": false }))
            }
            "set_session_name" => {
                let name = command.get("name").and_then(Value::as_str).unwrap_or("");
                self.session
                    .set_name(Some(name))
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                self.emit(json!({ "type": "session_info_changed", "name": name }));
                Ok(json!({ "name": name }))
            }
            "compact" => {
                self.lane
                    .compact(None)
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                Ok(json!({ "ok": true }))
            }
            "navigate_tree" => {
                let target = command
                    .get("targetId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("targetId is required"))?;
                self.lane
                    .navigate_tree(Some(target), false, None, None)
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                Ok(json!({ "cancelled": false }))
            }
            "set_auto_compaction" => {
                let enabled = command
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                self.auto_compaction.store(enabled, Ordering::SeqCst);
                Ok(json!({ "enabled": enabled }))
            }
            "get_commands" => Ok(json!({ "commands": [] })),
            "abort_compaction" => Ok(json!({ "cancelled": true })),
            "reload" => {
                let backend = self.clone();
                tokio::task::spawn_blocking(move || backend.state_json(true))
                    .await
                    .map_err(|error| anyhow!("state task failed: {error}"))
            }
            "extension_ui_response" | "extension_ui_input" => Ok(Value::Null),
            "fork" | "clone" => {
                use pi_star_harness::session::types::{
                    ForkOptions, SessionCreateOptions, SessionMetadata, SessionRepo, SessionStorage,
                };
                let header = read_header(&self.path)?;
                let repo = V3SessionRepo::new(self.config.clone(), &self.cwd);
                let metadata = SessionMetadata {
                    id: header.id.clone(),
                    created_at: 0,
                    parent_session_id: None,
                };
                let storage = repo
                    .fork(
                        &metadata,
                        &SessionCreateOptions::default(),
                        &ForkOptions::Branch {
                            entry_id: None,
                            position: None,
                        },
                    )
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                let new_id = storage.metadata().id.clone();
                Ok(json!({ "cancelled": false, "newSessionId": new_id }))
            }
            "bash" => {
                let script = command
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if script.is_empty() {
                    return Err(anyhow!("command is required"));
                }
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
                let output = tokio::process::Command::new(shell)
                    .arg("-lc")
                    .arg(&script)
                    .current_dir(&self.cwd)
                    .output()
                    .await?;
                let text = String::from_utf8_lossy(&output.stdout).into_owned()
                    + &String::from_utf8_lossy(&output.stderr);
                let exit_code = output.status.code();
                let message = AgentMessage::Custom(CustomMessage {
                    role: "bashExecution".into(),
                    content: Vec::new(),
                    data: json!({
                        "command": script,
                        "output": text,
                        "exitCode": exit_code,
                    }),
                    timestamp: now_ms(),
                });
                self.lane
                    .append_message(message.clone())
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                self.emit(
                    json!({ "type": "message_start", "message": { "role": "bashExecution" } }),
                );
                self.emit(json!({
                    "type": "message_end",
                    "message": {
                        "role": "bashExecution",
                        "command": script,
                        "output": text,
                        "exitCode": exit_code,
                        "timestamp": now_ms(),
                    },
                }));
                Ok(json!({ "output": text, "exitCode": exit_code }))
            }
            "abort_bash" => Ok(json!({ "cancelled": true })),
            other => Err(anyhow!("unknown command: {other}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Providers / models
// ---------------------------------------------------------------------------

fn resolve_model(config: &Config, entries: &[Value]) -> Option<ModelDef> {
    let (_, model_ref) = session_settings(entries, None);
    let explicit = model_ref.and_then(|model| {
        find_model(config, &model.provider, &model.model_id)
            .ok()
            .flatten()
    });
    explicit.or_else(|| {
        load_models(config, false).ok().and_then(|data| {
            data.default_model.and_then(|model| {
                find_model(config, &model.provider, &model.model_id)
                    .ok()
                    .flatten()
            })
        })
    })
}

fn resolve_thinking(config: &Config, cwd: &Path) -> String {
    let _ = cwd;
    // Matches pi's own DEFAULT_THINKING_LEVEL when settings.json has no value.
    crate::settings::read_settings(config)
        .get("defaultThinkingLevel")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| "medium".to_string())
}

fn parse_api(api: &str) -> pi_star_ai::types::Api {
    serde_json::from_value(Value::String(api.to_string()))
        .unwrap_or_else(|_| pi_star_ai::types::Api::Other(api.to_string()))
}

fn parse_thinking(level: &str) -> ThinkingLevel {
    serde_json::from_value(Value::String(level.to_string())).unwrap_or(ThinkingLevel::Off)
}

fn model_from_def(def: &ModelDef) -> Model {
    let input = if def.input.is_empty() {
        json!(["text"])
    } else {
        json!(def.input)
    };
    let mut value = json!({
        "id": def.id,
        "name": def.name,
        "api": def.api,
        "provider": def.provider,
        "baseUrl": def.base_url,
        "reasoning": def.reasoning,
        "input": input,
        "cost": def.cost,
        "contextWindow": def.context_window.unwrap_or(0),
        "maxTokens": def.max_tokens.unwrap_or(0),
    });
    if !def.thinking_level_map.is_empty() {
        value["thinkingLevelMap"] = json!(def.thinking_level_map);
    }
    if !def.compat.is_null() {
        value["compat"] = def.compat.clone();
    }
    serde_json::from_value(value).unwrap_or_else(|_| {
        Model::new(
            def.id.clone(),
            def.name.clone(),
            parse_api(&def.api),
            def.provider.clone(),
            def.base_url.clone(),
        )
    })
}

/// Provider wrapper that reports the pi-star provider id instead of the wire
/// implementation's fixed id (AnthropicProvider always reports "anthropic").
struct NamedProvider {
    id: String,
    models: Vec<Model>,
    inner: Arc<dyn Provider>,
}

#[async_trait::async_trait]
impl Provider for NamedProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn models(&self) -> &[Model] {
        &self.models
    }
    async fn stream_simple(
        &self,
        model: &Model,
        ctx: &pi_star_ai::types::Context,
        opts: &pi_star_ai::provider::SimpleStreamOptions,
    ) -> pi_star_ai::event_stream::AssistantMessageEventStream {
        self.inner.stream_simple(model, ctx, opts).await
    }
}

fn build_providers(config: &Config, _cwd: &Path) -> Result<Vec<Arc<dyn Provider>>> {
    let defs = crate::models::model_defs(config)?;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;
    let mut grouped: BTreeMap<String, Vec<ModelDef>> = BTreeMap::new();
    for def in defs.values() {
        grouped
            .entry(def.provider.clone())
            .or_default()
            .push(def.clone());
    }
    let mut out: Vec<Arc<dyn Provider>> = Vec::new();
    for (provider, models) in grouped {
        let api_key = models.iter().find_map(|def| def.api_key.clone());
        let api = models
            .iter()
            .find(|def| def.api != "faux")
            .map(|def| def.api.clone())
            .unwrap_or_else(|| models[0].api.clone());
        let model_list = models.iter().map(model_from_def).collect::<Vec<_>>();
        let inner: Arc<dyn Provider> = match api.as_str() {
            "anthropic-messages" => Arc::new(AnthropicProvider::with_models_without_env_api_key(
                api_key,
                http.clone(),
                model_list.clone(),
            )),
            "openai-completions" => Arc::new(OpenAiCompletionsProvider::with_models(
                provider.clone(),
                api_key,
                http.clone(),
                model_list.clone(),
            )),
            "openai-responses" => Arc::new(OpenAiResponsesProvider::with_models(
                provider.clone(),
                api_key,
                http.clone(),
                model_list.clone(),
            )),
            // Tests and offline harness fixtures can use the faux wire.
            "faux" => FauxProvider::with_models(FauxScript::new(), model_list.clone())
                as Arc<dyn Provider>,
            _ => continue,
        };
        out.push(Arc::new(NamedProvider {
            id: provider,
            models: model_list,
            inner,
        }));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

fn harness_tool_names(names: &[String]) -> Vec<String> {
    if names.is_empty() {
        vec![NO_TOOLS.to_string()]
    } else {
        names.to_vec()
    }
}

fn visible_tool_names(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|name| name.as_str() != NO_TOOLS)
        .cloned()
        .collect()
}

fn stored_tool_selection(path: &Path) -> Option<Vec<String>> {
    let session = read_session(path).ok()?;
    for entry in session.entries.iter().rev() {
        if entry.get("type").and_then(Value::as_str) != Some("custom")
            || entry.get("customType").and_then(Value::as_str) != Some("pi-star:tool-selection")
        {
            continue;
        }
        let Some(data) = entry.get("data") else {
            continue;
        };
        if data.get("version").and_then(Value::as_u64) != Some(1) {
            continue;
        }
        if data.get("cleared").and_then(Value::as_bool) == Some(true) {
            return None;
        }
        let Some(tools) = data.get("tools").and_then(Value::as_array) else {
            continue;
        };
        return Some(
            tools
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        );
    }
    None
}

async fn build_tools(
    cwd: &Path,
    roots: Arc<AllowedRoots>,
    config: &Config,
    parent_path: &Path,
    registry: Weak<AgentRegistry>,
    mcp: bool,
) -> Vec<HarnessTool> {
    let env = Arc::new(AllowedEnv::new(cwd.to_path_buf(), roots));
    let context = ExecutionToolContext::new(env.clone(), Some(env.clone()));
    let mut tools = vec![
        HarnessTool::new(pi_star_tools::create_read_tool(&context, None)),
        HarnessTool::new(pi_star_tools::create_bash_tool(&context, None)),
        HarnessTool::new(pi_star_tools::create_edit_tool(&context)),
        HarnessTool::new(pi_star_tools::create_write_tool(&context)),
        HarnessTool::new(pi_star_tools::create_grep_tool(&context, None)),
        HarnessTool::new(pi_star_tools::create_find_tool(&context, None)),
        HarnessTool::new(pi_star_tools::create_ls_tool(&context, None)),
    ];

    let bridge = Arc::new(McpBridge::new(config.clone(), cwd.to_path_buf()));
    let mcp_tools = if mcp {
        bridge.tools().await
    } else {
        Vec::new()
    };
    for (name, description, parameters) in mcp_tools {
        tools.push(HarnessTool::new(Arc::new(McpTool {
            schema: Tool {
                name,
                description,
                parameters: Schema::new(parameters),
                constrained_sampling: None,
            },
            bridge: bridge.clone(),
        })));
    }

    // Same gate as the RPC extension and pi-web: no built-in subagent tools
    // while the feature switch is off (profiles alone do not turn it on).
    if crate::settings::subagent_settings(config).built_in_enabled {
        tools.push(HarnessTool::new(Arc::new(AgentToolRs::new(
            registry.clone(),
            parent_path.to_path_buf(),
        ))));
        tools.push(HarnessTool::new(Arc::new(GetSubagentResultTool::new(
            registry.clone(),
            parent_path.to_path_buf(),
        ))));
        tools.push(HarnessTool::new(Arc::new(SteerSubagentTool::new(
            registry,
            parent_path.to_path_buf(),
        ))));
    }
    tools
}

// ---------------------------------------------------------------------------
// MCP tools
// ---------------------------------------------------------------------------

struct McpBridge {
    config: Config,
    cwd: PathBuf,
    runtime: tokio::sync::Mutex<Option<crate::mcp_runtime::McpRuntime>>,
}

impl McpBridge {
    fn new(config: Config, cwd: PathBuf) -> Self {
        Self {
            config,
            cwd,
            runtime: tokio::sync::Mutex::new(None),
        }
    }

    async fn ensure(&self) {
        let mut guard = self.runtime.lock().await;
        if guard.is_none() {
            *guard = Some(crate::mcp_runtime::McpRuntime::connect(&self.config, &self.cwd).await);
        }
    }

    async fn tools(&self) -> Vec<(String, String, Value)> {
        if !crate::mcp_runtime::has_configured_servers(&self.config, &self.cwd) {
            return Vec::new();
        }
        self.ensure().await;
        let guard = self.runtime.lock().await;
        guard
            .as_ref()
            .map(crate::mcp_runtime::McpRuntime::tool_schemas)
            .unwrap_or_default()
    }

    async fn call(&self, name: &str, arguments: Value) -> Result<String> {
        self.ensure().await;
        let mut guard = self.runtime.lock().await;
        match guard.as_mut() {
            Some(runtime) => runtime.call(name, arguments).await,
            None => Err(anyhow!("MCP is not available")),
        }
    }
}

struct McpTool {
    schema: Tool,
    bridge: Arc<McpBridge>,
}

#[async_trait::async_trait]
impl AgentTool for McpTool {
    fn schema(&self) -> &Tool {
        &self.schema
    }

    fn label(&self) -> &str {
        &self.schema.name
    }

    async fn execute(
        &self,
        _tool_call_id: &str,
        params: Value,
        _signal: tokio_util::sync::CancellationToken,
        _on_update: Arc<dyn Fn(pi_star_agent::types::ToolResultPartial) + Send + Sync>,
    ) -> Result<AgentToolResult, AgentError> {
        let text = self
            .bridge
            .call(&self.schema.name, params)
            .await
            .map_err(|error| AgentError::tool(error.to_string()))?;
        Ok(AgentToolResult {
            content: vec![TextContentOrImage::Text(TextContent {
                kind: Default::default(),
                text,
                text_signature: None,
            })],
            details: json!({ "mcp": true }),
            usage: None,
            added_tool_names: Vec::new(),
            terminate: false,
        })
    }
}

// ---------------------------------------------------------------------------
// Subagent tools (QuickPI)
// ---------------------------------------------------------------------------

/// Holds what a subagent tool needs: the parent runtime (created lazily, since
/// `build_tools` runs before the runtime exists) and the shared controller.
struct SubagentToolBase {
    registry: Weak<AgentRegistry>,
    parent_path: PathBuf,
}

impl SubagentToolBase {
    async fn parent(&self) -> Result<Arc<SessionRuntime>, AgentError> {
        let registry = self
            .registry
            .upgrade()
            .ok_or_else(|| AgentError::tool("the agent registry is unavailable"))?;
        registry
            .open_path(&self.parent_path)
            .map_err(|error| AgentError::tool(error.to_string()))
    }

    fn controller(&self) -> Result<Arc<crate::subagent_runtime::SubagentController>, AgentError> {
        let registry = self
            .registry
            .upgrade()
            .ok_or_else(|| AgentError::tool("the agent registry is unavailable"))?;
        registry
            .subagent_controller()
            .cloned()
            .ok_or_else(|| AgentError::tool("subagents are not available in this build"))
    }
}

struct AgentToolRs {
    schema: Tool,
    base: SubagentToolBase,
}

impl AgentToolRs {
    fn new(registry: Weak<AgentRegistry>, parent_path: PathBuf) -> Self {
        Self {
            schema: Tool {
                name: "subagent".into(),
                description: "Delegate a focused task to a subagent. Each subagent runs as a separate inspectable Pi session. Use background mode for independent work and foreground mode when the result is needed immediately.".into(),
                parameters: Schema::new(json!({
                    "type": "object",
                    "properties": {
                        "subagent_type": {
                            "type": "string",
                            "description": "Configured agent profile name. Default: general-purpose."
                        },
                        "prompt": {
                            "type": "string",
                            "description": "The complete task for the subagent."
                        },
                        "description": {
                            "type": "string",
                            "description": "Short activity label shown in the UI."
                        },
                        "run_in_background": {
                            "type": "boolean",
                            "description": "Return immediately and notify this session when complete."
                        },
                        "wait_timeout": {
                            "type": "number",
                            "description": "Seconds to wait for a foreground subagent. Default: 1800."
                        },
                        "model": {
                            "type": "string",
                            "description": "Optional provider/model override."
                        }
                    },
                    "required": ["prompt", "description"]
                })),
                constrained_sampling: None,
            },
            base: SubagentToolBase {
                registry,
                parent_path,
            },
        }
    }
}

#[async_trait::async_trait]
impl AgentTool for AgentToolRs {
    fn schema(&self) -> &Tool {
        &self.schema
    }

    fn label(&self) -> &str {
        "subagent"
    }

    async fn execute(
        &self,
        _tool_call_id: &str,
        params: Value,
        _signal: tokio_util::sync::CancellationToken,
        _on_update: Arc<dyn Fn(pi_star_agent::types::ToolResultPartial) + Send + Sync>,
    ) -> Result<AgentToolResult, AgentError> {
        let prompt = params
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if prompt.is_empty() {
            return Err(AgentError::tool("prompt is required"));
        }
        let controller = self.base.controller()?;
        let parent = self.base.parent().await?;
        let wait_ms = params
            .get("wait_timeout")
            .and_then(Value::as_f64)
            .filter(|value| *value > 0.0)
            .map(|seconds| (seconds * 1000.0) as u64)
            .unwrap_or(crate::subagent_runtime::DEFAULT_WAIT_MS);
        let request = crate::subagent_runtime::StartRequest {
            profile: params
                .get("subagent_type")
                .and_then(Value::as_str)
                .unwrap_or("general-purpose")
                .to_string(),
            task: prompt,
            description: params
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("Subagent")
                .to_string(),
            run_in_background: params.get("run_in_background").and_then(Value::as_bool),
            model: params
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_owned),
            wait_ms,
        };
        let info = controller
            .start(parent, request)
            .await
            .map_err(|error| AgentError::tool(error.to_string()))?;
        let text = info.final_text();
        let mut details = info.details();
        if info.run_in_background {
            if let Some(object) = details.as_object_mut() {
                object.insert(
                    "result".into(),
                    Value::String(
                        "Its report arrives on its own when it completes; no need to poll.".into(),
                    ),
                );
            }
        }
        if info.status == crate::subagent_runtime::RunStatus::Failed {
            return Err(AgentError::tool(text));
        }
        Ok(AgentToolResult {
            content: vec![TextContentOrImage::Text(TextContent {
                kind: Default::default(),
                text,
                text_signature: None,
            })],
            details,
            usage: None,
            added_tool_names: Vec::new(),
            terminate: false,
        })
    }
}

struct GetSubagentResultTool {
    schema: Tool,
    base: SubagentToolBase,
}

impl GetSubagentResultTool {
    fn new(registry: Weak<AgentRegistry>, parent_path: PathBuf) -> Self {
        Self {
            schema: Tool {
                name: "get_subagent_result".into(),
                description: "Retrieve a subagent session's latest result, such as a report no longer in the conversation. A background run's report arrives on its own when it completes.".into(),
                parameters: Schema::new(json!({
                    "type": "object",
                    "properties": {
                        "agent_id": {
                            "type": "string",
                            "description": "Subagent session ID."
                        },
                        "wait": {
                            "type": "boolean",
                            "description": "Wait for completion."
                        },
                        "wait_timeout": {
                            "type": "number",
                            "description": "Seconds to wait. Default: 1800."
                        }
                    },
                    "required": ["agent_id"]
                })),
                constrained_sampling: None,
            },
            base: SubagentToolBase {
                registry,
                parent_path,
            },
        }
    }
}

#[async_trait::async_trait]
impl AgentTool for GetSubagentResultTool {
    fn schema(&self) -> &Tool {
        &self.schema
    }

    fn label(&self) -> &str {
        "get_subagent_result"
    }

    async fn execute(
        &self,
        _tool_call_id: &str,
        params: Value,
        _signal: tokio_util::sync::CancellationToken,
        _on_update: Arc<dyn Fn(pi_star_agent::types::ToolResultPartial) + Send + Sync>,
    ) -> Result<AgentToolResult, AgentError> {
        let id = params
            .get("agent_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            return Err(AgentError::tool("agent_id is required"));
        }
        let controller = self.base.controller()?;
        let wait = params.get("wait").and_then(Value::as_bool).unwrap_or(false);
        let wait_ms = params
            .get("wait_timeout")
            .and_then(Value::as_f64)
            .filter(|value| *value > 0.0)
            .map(|seconds| (seconds * 1000.0) as u64)
            .unwrap_or(crate::subagent_runtime::DEFAULT_WAIT_MS);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(wait_ms.max(1));
        let info = loop {
            if let Some(info) = controller.get(&id) {
                if info.status.is_terminal() || !wait || std::time::Instant::now() >= deadline {
                    break Some(info);
                }
            } else {
                break None;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        };
        let Some(info) = info else {
            return Err(AgentError::tool(format!("Subagent not found: {id}")));
        };
        if info.status.is_terminal() {
            controller.mark_consumed(&info);
        }
        let text = info.final_text();
        if info.status == crate::subagent_runtime::RunStatus::Failed {
            return Err(AgentError::tool(text));
        }
        Ok(AgentToolResult {
            content: vec![TextContentOrImage::Text(TextContent {
                kind: Default::default(),
                text,
                text_signature: None,
            })],
            details: info.details(),
            usage: None,
            added_tool_names: Vec::new(),
            terminate: false,
        })
    }
}

struct SteerSubagentTool {
    schema: Tool,
    base: SubagentToolBase,
}

impl SteerSubagentTool {
    fn new(registry: Weak<AgentRegistry>, parent_path: PathBuf) -> Self {
        Self {
            schema: Tool {
                name: "steer_subagent".into(),
                description: "Send a steering message to a currently running subagent session."
                    .into(),
                parameters: Schema::new(json!({
                    "type": "object",
                    "properties": {
                        "agent_id": {
                            "type": "string",
                            "description": "Subagent session ID."
                        },
                        "message": {
                            "type": "string",
                            "description": "Instruction to inject after the current tool execution."
                        }
                    },
                    "required": ["agent_id", "message"]
                })),
                constrained_sampling: None,
            },
            base: SubagentToolBase {
                registry,
                parent_path,
            },
        }
    }
}

#[async_trait::async_trait]
impl AgentTool for SteerSubagentTool {
    fn schema(&self) -> &Tool {
        &self.schema
    }

    fn label(&self) -> &str {
        "steer_subagent"
    }

    async fn execute(
        &self,
        _tool_call_id: &str,
        params: Value,
        _signal: tokio_util::sync::CancellationToken,
        _on_update: Arc<dyn Fn(pi_star_agent::types::ToolResultPartial) + Send + Sync>,
    ) -> Result<AgentToolResult, AgentError> {
        let id = params
            .get("agent_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let message = params
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if id.is_empty() || message.is_empty() {
            return Err(AgentError::tool("agent_id and message are required"));
        }
        let controller = self.base.controller()?;
        controller
            .steer(&id, &message)
            .await
            .map_err(|error| AgentError::tool(error.to_string()))?;
        Ok(AgentToolResult {
            content: vec![TextContentOrImage::Text(TextContent {
                kind: Default::default(),
                text: format!("Steering message sent to {id}."),
                text_signature: None,
            })],
            details: Value::Null,
            usage: None,
            added_tool_names: Vec::new(),
            terminate: false,
        })
    }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

struct WireEmitter {
    events: broadcast::Sender<Value>,
}

impl WireEmitter {
    fn send(&self, event: Value) {
        let _ = self.events.send(event);
    }
}

impl AgentEmitter for WireEmitter {
    fn emit(&self, event: AgentEvent) -> futures_util::future::BoxFuture<'static, ()> {
        if let Some(value) = agent_event_to_wire(&event) {
            self.send(value);
        }
        Box::pin(async {})
    }

    fn try_emit(&self, event: AgentEvent) {
        if let Some(value) = agent_event_to_wire(&event) {
            self.send(value);
        }
    }
}

fn agent_message_json(message: &AgentMessage) -> Value {
    message_to_v3(message)
}

fn assistant_json(message: &pi_star_ai::types::AssistantMessage) -> Value {
    serde_json::to_value(message).unwrap_or(Value::Null)
}

fn tool_result_json(result: &AgentToolResult) -> Value {
    json!({
        "content": result.content.iter().map(|block| match block {
            pi_star_agent::types::TextContentOrImage::Text(text) => json!({
                "type": "text",
                "text": text.text,
            }),
            pi_star_agent::types::TextContentOrImage::Image(image) => {
                serde_json::to_value(image).unwrap_or(Value::Null)
            }
        }).collect::<Vec<_>>(),
        "details": result.details,
        "usage": result.usage,
        "terminate": result.terminate,
    })
}

fn assistant_event_json(event: &pi_star_ai::types::AssistantMessageEvent) -> Value {
    use pi_star_ai::types::AssistantMessageEvent as E;
    match event {
        E::Start { partial } => json!({ "type": "start", "partial": assistant_json(partial) }),
        E::TextStart { content_index, .. } => {
            json!({ "type": "text_start", "contentIndex": content_index })
        }
        E::TextDelta {
            content_index,
            delta,
            ..
        } => json!({ "type": "text_delta", "contentIndex": content_index, "delta": delta }),
        E::TextEnd {
            content_index,
            content,
            ..
        } => json!({ "type": "text_end", "contentIndex": content_index, "content": content }),
        E::ThinkingStart { content_index, .. } => {
            json!({ "type": "thinking_start", "contentIndex": content_index })
        }
        E::ThinkingDelta {
            content_index,
            delta,
            ..
        } => json!({ "type": "thinking_delta", "contentIndex": content_index, "delta": delta }),
        E::ThinkingEnd {
            content_index,
            content,
            ..
        } => json!({
            "type": "thinking_end",
            "contentIndex": content_index,
            "content": content,
        }),
        E::ToolCallStart { content_index, .. } => {
            json!({ "type": "toolcall_start", "contentIndex": content_index })
        }
        E::ToolCallDelta {
            content_index,
            delta,
            ..
        } => json!({ "type": "toolcall_delta", "contentIndex": content_index, "delta": delta }),
        E::ToolCallEnd {
            content_index,
            tool_call,
            ..
        } => json!({
            "type": "toolcall_end",
            "contentIndex": content_index,
            "toolCall": tool_call_json(tool_call),
        }),
        E::Done { reason, message } => json!({
            "type": "done",
            "reason": reason.as_str(),
            "message": assistant_json(message),
        }),
        E::Error { reason, error } => json!({
            "type": "error",
            "reason": reason.as_str(),
            "error": assistant_json(error),
        }),
    }
}

fn tool_call_json(call: &ToolCall) -> Value {
    serde_json::to_value(call).unwrap_or(Value::Null)
}

fn agent_event_to_wire(event: &AgentEvent) -> Option<Value> {
    Some(match event {
        AgentEvent::AgentStart => json!({ "type": "agent_start" }),
        AgentEvent::TurnStart => json!({ "type": "turn_start" }),
        AgentEvent::TurnEnd {
            message,
            tool_results,
        } => json!({
            "type": "turn_end",
            "message": agent_message_json(message),
            "toolResults": tool_results
                .iter()
                .map(|result| serde_json::to_value(result).unwrap_or(Value::Null))
                .collect::<Vec<_>>(),
        }),
        AgentEvent::MessageStart { message } => json!({
            "type": "message_start",
            "message": agent_message_json(message),
        }),
        AgentEvent::MessageUpdate {
            message,
            assistant_message_event,
        } => json!({
            "type": "message_update",
            "message": assistant_json(message),
            "assistantMessageEvent": assistant_event_json(assistant_message_event),
        }),
        AgentEvent::MessageEnd { message } => json!({
            "type": "message_end",
            "message": agent_message_json(message),
        }),
        AgentEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            args,
        } => json!({
            "type": "tool_execution_start",
            "toolCallId": tool_call_id,
            "toolName": tool_name,
            "args": args,
        }),
        AgentEvent::ToolExecutionUpdate {
            tool_call_id,
            tool_name,
            args,
            partial_result,
        } => json!({
            "type": "tool_execution_update",
            "toolCallId": tool_call_id,
            "toolName": tool_name,
            "args": args,
            "partialResult": tool_result_json(partial_result),
        }),
        AgentEvent::ToolExecutionEnd {
            tool_call_id,
            tool_name,
            result,
            is_error,
        } => json!({
            "type": "tool_execution_end",
            "toolCallId": tool_call_id,
            "toolName": tool_name,
            "result": tool_result_json(result),
            "isError": is_error,
        }),
        AgentEvent::RetryScheduled {
            attempt,
            max_retries,
            delay_ms,
            error,
        } => json!({
            "type": "retry_scheduled",
            "attempt": attempt,
            "maxRetries": max_retries,
            "delayMs": delay_ms,
            "errorMessage": error,
        }),
        AgentEvent::AgentEnd { messages } => json!({
            "type": "agent_end",
            "messages": messages.iter().map(agent_message_json).collect::<Vec<_>>(),
        }),
    })
}

fn queued_text(message: &AgentMessage) -> String {
    match message {
        AgentMessage::User(user) => match &user.content {
            pi_star_ai::types::UserContent::Text(text) => text.clone(),
            pi_star_ai::types::UserContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|block| match block {
                    pi_star_ai::types::Content::Text(text) => Some(text.text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        },
        _ => String::new(),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi_star_ai::providers::faux::{FauxProvider, FauxScript, FauxStep};
    use pi_star_harness::session::types::{SessionCreateOptions, SessionRepo, SessionStorage};

    #[test]
    fn unset_reasoning_default_resolves_to_pis_medium() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::new(Some(dir.path().join("agent")), None).unwrap();
        let cwd = dir.path().join("proj");
        assert_eq!(resolve_thinking(&config, &cwd), "medium");

        std::fs::create_dir_all(&config.agent_dir).unwrap();
        std::fs::write(
            config.settings_path(),
            r#"{"defaultThinkingLevel":"low"}"#,
        )
        .unwrap();
        assert_eq!(resolve_thinking(&config, &cwd), "low");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn prompt_streams_pi_events_and_persists_v3() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::new(Some(dir.path().join("agent")), None).unwrap();
        // The subagent tools register only while the feature switch is on.
        std::fs::create_dir_all(config.agent_dir.join("agents")).unwrap();
        std::fs::write(
            config.agent_dir.join("agents").join("settings.json"),
            r#"{"version":1,"builtInEnabled":true}"#,
        )
        .unwrap();
        let cwd = dir.path().join("proj");
        std::fs::create_dir_all(&cwd).unwrap();
        let roots = Arc::new(AllowedRoots::new());

        let repo = V3SessionRepo::new(config.clone(), &cwd);
        let storage = repo.create(&SessionCreateOptions::default()).await.unwrap();
        let id = storage.metadata().id.clone();
        drop(storage);
        let path = crate::config::session_file_for_id(&config.sessions_dir, &id).unwrap();

        let script = FauxScript::new();
        script.append_responses(vec![FauxStep::text("hello there")]);
        let provider = FauxProvider::new(script);
        let model = provider.default_model().clone();
        let model_def = ModelDef {
            id: model.id.clone(),
            name: model.name.clone(),
            provider: model.provider.clone(),
            api: "faux".into(),
            base_url: model.base_url.clone(),
            api_key: None,
            reasoning: false,
            input: vec!["text".into()],
            context_window: Some(model.context_window),
            max_tokens: Some(model.max_tokens),
            cost: json!({}),
            thinking_level_map: BTreeMap::new(),
            compat: Value::Null,
        };

        let backend = HarnessBackend::build(
            &config,
            roots,
            &path,
            cwd,
            id,
            model_def,
            vec![provider as Arc<dyn Provider>],
            Some("test prompt".into()),
            Weak::new(),
            HarnessResources::default(),
        )
        .await
        .unwrap();

        let tools = backend.harness.get_tools().await.unwrap();
        let names: Vec<&str> = tools
            .iter()
            .map(|tool| tool.tool.schema().name.as_str())
            .collect();
        assert!(names.contains(&"read"));
        assert!(names.contains(&"bash"));
        assert!(names.contains(&"subagent"));
        assert!(!names.iter().any(|name| name.starts_with("mcp__")));

        let mut events = backend.subscribe();
        let response = backend
            .send(&json!({"type": "prompt", "message": "hi"}))
            .await
            .unwrap();
        assert_eq!(response["queued"], false);

        let mut seen = Vec::new();
        loop {
            let event = tokio::time::timeout(std::time::Duration::from_secs(10), events.recv())
                .await
                .expect("event timeout")
                .expect("event channel closed");
            let kind = event
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            seen.push(kind.clone());
            if kind == "prompt_done" {
                break;
            }
        }
        for expected in [
            "agent_start",
            "message_start",
            "message_update",
            "message_end",
            "turn_end",
            "agent_end",
            "prompt_done",
        ] {
            assert!(
                seen.iter().any(|kind| kind == expected),
                "missing {expected} in {seen:?}"
            );
        }

        let session = crate::session::read_session(&backend.path).unwrap();
        let role = |entry: &Value| {
            entry
                .pointer("/message/role")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let text = |entry: &Value, index: usize| match entry.pointer("/message/content") {
            Some(Value::String(text)) if index == 0 => text.clone(),
            _ => entry
                .pointer(&format!("/message/content/{index}/text"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        };
        let user = session
            .entries
            .iter()
            .find(|entry| role(entry) == "user")
            .expect("user entry persisted");
        assert_eq!(text(user, 0), "hi");
        let assistant = session
            .entries
            .iter()
            .find(|entry| role(entry) == "assistant")
            .expect("assistant entry persisted");
        assert_eq!(text(assistant, 0), "hello there");
        assert!(
            user.get("message").unwrap().get("kind").is_none(),
            "the v3 wire shape must not carry the runtime's `kind` tag"
        );
        assert!(user.get("timestamp").and_then(Value::as_str).is_some());
        assert!(
            assistant.get("timestamp").and_then(Value::as_str).is_some(),
            "entry timestamps are ISO strings in v3"
        );

        let bash = backend
            .send(&json!({"type": "bash", "command": "echo hello-bash"}))
            .await
            .unwrap();
        assert_eq!(bash["exitCode"], 0);
        assert!(
            bash["output"]
                .as_str()
                .is_some_and(|output| output.contains("hello-bash"))
        );
        let session = crate::session::read_session(&backend.path).unwrap();
        assert!(session.entries.iter().any(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("message")
                && entry.pointer("/message/role").and_then(Value::as_str) == Some("bashExecution")
        }));

        let usage = pi_star_ai::types::Usage {
            input: 1,
            output: 2,
            cache_read: 0,
            cache_write: 0,
            cache_write_1h: None,
            reasoning: None,
            total_tokens: 3,
            cost: Default::default(),
        };
        backend
            .append_usage("cache_warm", "faux", "faux-model", usage, Some("test"))
            .await
            .unwrap();
        let session = crate::session::read_session(&backend.path).unwrap();
        let usage_entry = session
            .entries
            .iter()
            .find(|entry| entry.get("type").and_then(Value::as_str) == Some("usage"))
            .expect("usage entry persisted");
        assert_eq!(usage_entry["kind"], "cache_warm");
        assert_eq!(usage_entry["provider"], "faux");
        assert_eq!(usage_entry["model"], "faux-model");
        assert_eq!(usage_entry["note"], "test");
        assert_eq!(usage_entry["usage"]["totalTokens"], 3);
        assert!(usage_entry["timestamp"].as_str().is_some());
        assert!(usage_entry["parentId"].as_str().is_some());

        let fork = backend.send(&json!({"type": "fork"})).await.unwrap();
        let fork_id = fork["newSessionId"].as_str().unwrap();
        assert!(
            crate::config::session_file_for_id(&config.sessions_dir, fork_id).is_some(),
            "fork must create a new v3 session file"
        );
    }
}
