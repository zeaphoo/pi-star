//! AgentHarness-backed runtime (the "merged" backend).
//!
//! This is the new engine for pi-star: it drives `pi_star_harness::AgentHarness`
//! over the pi v3 session backend and translates the harness's `AgentEvent`s
//! into the JSON events pi-web already consumes.
//!
//! It is intentionally separate from [`super::SessionRuntime`] while the route
//! wiring is migrated: the façade keeps its command/state contract and will
//! delegate here once `PI_STAR_AGENT_BACKEND=merged` is enabled.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

use crate::agent::AgentRegistry;
use crate::agent::harness_env::AllowedEnv;
use crate::agent::session_store::{V3SessionRepo, message_to_v3};
use crate::config::Config;
use crate::file_access::AllowedRoots;
use crate::models::{ModelDef, find_model, load_models};
use crate::session::{read_header, read_session, session_settings};

/// Shared event buffer size; mirrors the legacy `SessionRuntime` channel.
const EVENT_BUFFER: usize = 512;

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
    system_prompt: String,
}

impl HarnessBackend {
    /// Open an existing session file with the configured/default model.
    pub async fn open(
        config: &Config,
        roots: Arc<AllowedRoots>,
        path: &Path,
        registry: Weak<AgentRegistry>,
    ) -> Result<Arc<Self>> {
        let header = read_header(path)?;
        let session_file = read_session(path)?;
        let cwd = PathBuf::from(&header.cwd);
        roots.add(&cwd);
        let model = resolve_model(config, &session_file.entries).ok_or_else(|| {
            anyhow!("No model selected. Configure a provider in Settings › Models.")
        })?;
        let providers = build_providers(config, &cwd)?;
        Self::build(
            config, roots, path, cwd, header.id, model, providers, None, registry,
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

        let tools = build_tools(&cwd, roots.clone(), config, path, registry).await;
        let tool_names: Vec<String> = tools
            .iter()
            .map(|tool| tool.tool.schema().name.clone())
            .collect();
        let system_prompt =
            system_prompt.unwrap_or_else(|| super::build_system_prompt(config, &cwd, &tool_names));
        let stored_prompt = system_prompt.clone();
        let thinking = read_session(path)
            .ok()
            .and_then(|file| {
                file.entries
                    .iter()
                    .any(|entry| {
                        entry.get("type").and_then(Value::as_str) == Some("thinking_level_change")
                    })
                    .then(|| session_settings(&file.entries, None).0)
            })
            .unwrap_or_else(|| resolve_thinking(config, &cwd));
        let model = model_from_def(&model_def);
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        let emitter = Arc::new(WireEmitter {
            events: events.clone(),
        });

        let options = AgentHarnessOptions {
            model,
            thinking_level: parse_thinking(&thinking),
            active_tool_names: tool_names,
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
            system_prompt: stored_prompt,
        }))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.events.subscribe()
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
            "autoCompactionEnabled": true,
            "extensionStatuses": [],
            "extensionWidgets": [],
            "queuedMessages": { "steering": [], "followUp": [] },
            "contextUsage": { "percent": Value::Null, "contextWindow": 0, "tokens": Value::Null },
            "totalActiveMs": 0,
            "lastUsage": {},
        });
        if include_prompt {
            value["systemPrompt"] = Value::String(self.system_prompt.clone());
        }
        value
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
            "get_state" => Ok(self.state_json(true)),
            "get_session_stats" => {
                let stats = self
                    .lane
                    .get_stats()
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
                Ok(json!({
                    "sessionFile": self.path.to_string_lossy(),
                    "sessionId": self.id,
                    "userMessages": stats.message_count,
                    "assistantMessages": 0,
                    "toolCalls": 0,
                    "toolResults": 0,
                    "totalMessages": stats.message_count,
                    "tokens": stats.total_tokens,
                    "cost": stats.cost_total,
                    "totalActiveMs": 0,
                }))
            }
            "get_last_assistant_text" => {
                let entries = read_session(&self.path)
                    .map(|session| session.entries)
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
                    None => self
                        .lane
                        .get_active_tools()
                        .await
                        .map_err(|error| anyhow!(error.to_string()))?,
                };
                self.lane
                    .set_active_tools(names)
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
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
            "set_auto_compaction" => Ok(json!({
                "enabled": command.get("enabled").and_then(Value::as_bool).unwrap_or(true),
            })),
            "abort_compaction" => Ok(json!({ "cancelled": true })),
            "reload" => Ok(self.state_json(true)),
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
        load_models(config).ok().and_then(|data| {
            data.default_model.and_then(|model| {
                find_model(config, &model.provider, &model.model_id)
                    .ok()
                    .flatten()
            })
        })
    })
}

fn resolve_thinking(config: &Config, cwd: &Path) -> String {
    crate::settings::read_settings(config)
        .get("defaultThinkingLevel")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let _ = cwd;
            "off".to_string()
        })
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

async fn build_tools(
    cwd: &Path,
    roots: Arc<AllowedRoots>,
    config: &Config,
    parent_path: &Path,
    registry: Weak<AgentRegistry>,
) -> Vec<HarnessTool> {
    let env = Arc::new(AllowedEnv::new(cwd.to_path_buf(), roots));
    let context = ExecutionToolContext::new(env.clone(), Some(env.clone()));
    let mut tools = vec![
        HarnessTool::new(pi_star_tools::create_read_tool(&context, None)),
        HarnessTool::new(pi_star_tools::create_write_tool(&context)),
        HarnessTool::new(pi_star_tools::create_edit_tool(&context)),
        HarnessTool::new(pi_star_tools::create_bash_tool(&context, None)),
        HarnessTool::new(pi_star_tools::create_find_tool(&context, None)),
        HarnessTool::new(pi_star_tools::create_grep_tool(&context, None)),
        HarnessTool::new(pi_star_tools::create_ls_tool(&context, None)),
    ];

    let bridge = Arc::new(McpBridge::new(config.clone(), cwd.to_path_buf()));
    for (name, description, parameters) in bridge.tools().await {
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

    tools.push(HarnessTool::new(Arc::new(SubagentTool::new(
        config.clone(),
        cwd.to_path_buf(),
        parent_path.to_path_buf(),
        registry,
    ))));
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
// Subagent tool
// ---------------------------------------------------------------------------

struct SubagentTool {
    schema: Tool,
    config: Config,
    cwd: PathBuf,
    parent_path: PathBuf,
    registry: Weak<AgentRegistry>,
    slots: Arc<AtomicUsize>,
}

impl SubagentTool {
    fn new(
        config: Config,
        cwd: PathBuf,
        parent_path: PathBuf,
        registry: Weak<AgentRegistry>,
    ) -> Self {
        Self {
            schema: Tool {
                name: "subagent".into(),
                description: "Run a child coding agent and return its final answer.".into(),
                parameters: Schema::new(json!({
                    "type": "object",
                    "properties": {
                        "description": {
                            "type": "string",
                            "description": "Short label shown in the session tree"
                        },
                        "prompt": {
                            "type": "string",
                            "description": "Complete instruction for the child agent"
                        }
                    },
                    "required": ["description", "prompt"]
                })),
                constrained_sampling: None,
            },
            config,
            cwd,
            parent_path,
            registry,
            slots: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait::async_trait]
impl AgentTool for SubagentTool {
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
        signal: tokio_util::sync::CancellationToken,
        _on_update: Arc<dyn Fn(pi_star_agent::types::ToolResultPartial) + Send + Sync>,
    ) -> Result<AgentToolResult, AgentError> {
        let description = params
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("Subagent")
            .chars()
            .take(200)
            .collect::<String>();
        let prompt = params
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if prompt.is_empty() {
            return Err(AgentError::tool("prompt is required"));
        }
        let registry = self
            .registry
            .upgrade()
            .ok_or_else(|| AgentError::tool("the agent registry is unavailable"))?;
        let max = registry.max_subagents();
        let active = self.slots.fetch_add(1, Ordering::SeqCst);
        if active >= max {
            self.slots.fetch_sub(1, Ordering::SeqCst);
            return Err(AgentError::tool(format!(
                "too many subagents are already running (max {max})"
            )));
        }
        let _slot = SlotGuard(self.slots.clone());

        let path = crate::session::create_session_file_with_parent(
            &self.config,
            &self.cwd,
            Some(&self.parent_path),
        )
        .map_err(|error| AgentError::tool(error.to_string()))?;
        let child = crate::session::read_session(&path)
            .map_err(|error| AgentError::tool(error.to_string()))?;
        let parent_id = crate::session::read_session(&self.parent_path)
            .map(|session| session.header.id.clone())
            .unwrap_or_default();
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
                        "parentSessionId": parent_id,
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
        let child_id = child.header.id.clone();

        let runtime = registry
            .open_path(&path)
            .map_err(|error| AgentError::tool(error.to_string()))?;
        let mut receiver = runtime
            .subscribe()
            .await
            .map_err(|error| AgentError::tool(error.to_string()))?;
        runtime
            .send(&json!({"type": "prompt", "message": prompt}))
            .await
            .map_err(|error| AgentError::tool(error.to_string()))?;

        let deadline = tokio::time::sleep(std::time::Duration::from_secs(600));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = signal.cancelled() => {
                    runtime.abort();
                    return Err(AgentError::Abort);
                }
                _ = &mut deadline => {
                    runtime.abort();
                    return Err(AgentError::tool("subagent timed out"));
                }
                event = receiver.recv() => {
                    let Ok(event) = event else { break };
                    if event.get("type").and_then(Value::as_str) == Some("prompt_done") {
                        break;
                    }
                }
            }
        }

        let text = last_assistant_text(&path).unwrap_or_default();
        Ok(AgentToolResult {
            content: vec![TextContentOrImage::Text(TextContent {
                kind: Default::default(),
                text,
                text_signature: None,
            })],
            details: json!({ "subagentSessionId": child_id }),
            usage: None,
            added_tool_names: Vec::new(),
            terminate: false,
        })
    }
}

struct SlotGuard(Arc<AtomicUsize>);

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn last_assistant_text(path: &Path) -> Option<String> {
    let session = crate::session::read_session(path).ok()?;
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
                        .filter_map(|block| {
                            block.get("text").and_then(Value::as_str).map(str::to_owned)
                        })
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default()
        })
    })
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn prompt_streams_pi_events_and_persists_v3() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::new(Some(dir.path().join("agent")), None).unwrap();
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

        let fork = backend.send(&json!({"type": "fork"})).await.unwrap();
        let fork_id = fork["newSessionId"].as_str().unwrap();
        assert!(
            crate::config::session_file_for_id(&config.sessions_dir, fork_id).is_some(),
            "fork must create a new v3 session file"
        );
    }
}
