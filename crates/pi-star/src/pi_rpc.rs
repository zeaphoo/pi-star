//! Pi CLI RPC backend: one long-lived `pi --mode rpc` child per active session.
//! The Rust server keeps the web/control plane; pi owns the Agent and extension
//! host. The boundary is pi's official JSONL RPC protocol.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, oneshot};

use pi_star_core::Config;
use pi_star_core::session::{
    append_lines, create_session_file, new_entry, read_session, resolve_session_path,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_RPC_LINE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct PiSpawnOptions {
    pub provider: Option<String>,
    pub model_id: Option<String>,
    pub thinking_level: Option<String>,
    pub tool_names: Option<Vec<String>>,
    pub name: Option<String>,
}

struct PiIdentity {
    id: String,
    path: PathBuf,
}

struct PiState {
    running: bool,
    state: Value,
    pending_ui: HashSet<String>,
}

struct Pending {
    sender: oneshot::Sender<Result<Value, String>>,
    command: String,
}

struct PiControlState {
    pending: Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>,
    writer: tokio::sync::Mutex<Option<tokio::net::tcp::OwnedWriteHalf>>,
    next_id: AtomicU64,
}

pub struct PiControl {
    state: Arc<PiControlState>,
    token: String,
}

impl PiControl {
    async fn start(token: String) -> Result<(Arc<Self>, std::net::SocketAddr)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let control = Arc::new(Self {
            state: Arc::new(PiControlState {
                pending: Mutex::new(HashMap::new()),
                writer: tokio::sync::Mutex::new(None),
                next_id: AtomicU64::new(1),
            }),
            token,
        });
        let accepted = control.clone();
        tokio::spawn(async move {
            let _ = accepted.accept_loop(listener).await;
        });
        Ok((control, addr))
    }

    async fn accept_loop(self: Arc<Self>, listener: TcpListener) -> Result<()> {
        loop {
            let (stream, _) = listener.accept().await?;
            let (read, write) = stream.into_split();
            let mut lines = BufReader::new(read).lines();
            let Some(hello) = lines.next_line().await? else {
                continue;
            };
            let hello: Value = serde_json::from_str(&hello).unwrap_or(Value::Null);
            if hello.get("token").and_then(Value::as_str) != Some(self.token.as_str()) {
                continue;
            }
            *self.state.writer.lock().await = Some(write);
            while let Some(line) = lines.next_line().await? {
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if value.get("type").and_then(Value::as_str) != Some("response") {
                    continue;
                }
                let Some(id) = value.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(sender) = self.state.pending.lock().remove(id) else {
                    continue;
                };
                if value.get("success").and_then(Value::as_bool) == Some(true) {
                    let _ = sender.send(Ok(value.get("data").cloned().unwrap_or(Value::Null)));
                } else {
                    let _ = sender.send(Err(value
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("pi-star agent extension command failed")
                        .to_string()));
                }
            }
            *self.state.writer.lock().await = None;
        }
    }

    fn next_id(&self) -> String {
        format!(
            "control-{}",
            self.state.next_id.fetch_add(1, Ordering::Relaxed)
        )
    }

    fn register(&self, id: &str) -> oneshot::Receiver<Result<Value, String>> {
        let (sender, receiver) = oneshot::channel();
        self.state.pending.lock().insert(id.to_string(), sender);
        receiver
    }

    fn cancel(&self, id: &str) {
        self.state.pending.lock().remove(id);
    }
}

pub struct PiRpcSession {
    identity: Mutex<PiIdentity>,
    pub cwd: PathBuf,
    child: Mutex<Option<Child>>,
    stdin: tokio::sync::Mutex<Option<tokio::process::ChildStdin>>,
    pending: Mutex<HashMap<String, Pending>>,
    next_id: AtomicU64,
    aborted: AtomicBool,
    events: broadcast::Sender<Value>,
    state: Mutex<PiState>,
    last_activity: Mutex<Instant>,
    control: Arc<PiControl>,
    extension_available: bool,
    completion: Mutex<Option<Arc<dyn Fn(&str, Option<&str>) + Send + Sync>>>,
    rekey: Mutex<Option<Arc<dyn Fn(&str, &str, &Path) + Send + Sync>>>,
    started_at: Instant,
    config: Config,
}

impl PiRpcSession {
    fn new(
        id: String,
        path: PathBuf,
        cwd: PathBuf,
        child: Child,
        stdin: tokio::process::ChildStdin,
        stdout: tokio::process::ChildStdout,
        stderr: tokio::process::ChildStderr,
        control: Arc<PiControl>,
        extension_available: bool,
        config: Config,
    ) -> Arc<Self> {
        let (events, _) = broadcast::channel(2048);
        let session = Arc::new(Self {
            identity: Mutex::new(PiIdentity { id, path }),
            cwd,
            child: Mutex::new(Some(child)),
            stdin: tokio::sync::Mutex::new(Some(stdin)),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            aborted: AtomicBool::new(false),
            events,
            state: Mutex::new(PiState {
                running: false,
                state: json!({}),
                pending_ui: HashSet::new(),
            }),
            last_activity: Mutex::new(Instant::now()),
            control,
            extension_available,
            completion: Mutex::new(None),
            rekey: Mutex::new(None),
            started_at: Instant::now(),
            config,
        });
        let reader_session = session.clone();
        tokio::spawn(async move { reader_session.read_loop(stdout).await });
        let stderr_session = session.clone();
        tokio::spawn(async move { stderr_session.stderr_loop(stderr).await });
        session
    }

    async fn stderr_loop(&self, stderr: tokio::process::ChildStderr) {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                tracing::debug!(session = %self.id(), "pi rpc stderr: {line}");
            }
        }
    }

    async fn read_loop(&self, stdout: tokio::process::ChildStdout) {
        let mut lines = BufReader::new(stdout).lines();
        let mut bytes = 0usize;
        while let Ok(Some(line)) = lines.next_line().await {
            bytes = bytes.saturating_add(line.len() + 1);
            if bytes > MAX_RPC_LINE_BYTES {
                tracing::warn!(session = %self.id(), "pi rpc line limit exceeded");
                bytes = 0;
                continue;
            }
            let Ok(record) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            *self.last_activity.lock() = Instant::now();
            match record.get("type").and_then(Value::as_str) {
                Some("response") => self.handle_response(record),
                Some("extension_ui_request") => {
                    if let Some(id) = record.get("id").and_then(Value::as_str) {
                        self.state.lock().pending_ui.insert(id.to_string());
                    }
                    let _ = self.events.send(record);
                }
                Some("extension_ui_closed") => {
                    if let Some(id) = record.get("id").and_then(Value::as_str) {
                        self.state.lock().pending_ui.remove(id);
                    }
                    let _ = self.events.send(record);
                }
                _ => self.handle_event(record),
            }
        }
        self.fail_all("pi rpc process exited");
        self.state.lock().running = false;
        let _ = self.events.send(json!({"type":"session_shutdown"}));
    }

    fn handle_response(&self, record: Value) {
        let id = record.get("id").and_then(Value::as_str).map(str::to_owned);
        let Some(id) = id else { return };
        let Some(pending) = self.pending.lock().remove(&id) else {
            return;
        };
        if record.get("success").and_then(Value::as_bool) == Some(true) {
            let data = record.get("data").cloned().unwrap_or(Value::Null);
            if pending.command == "get_state" {
                self.set_state(data.clone());
            }
            let _ = pending.sender.send(Ok(data));
        } else {
            let error = record
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("pi RPC command failed")
                .to_string();
            let _ = pending.sender.send(Err(error));
        }
    }

    fn handle_event(&self, event: Value) {
        match event.get("type").and_then(Value::as_str) {
            Some("agent_start") | Some("turn_start") => {
                let mut state = self.state.lock();
                state.running = true;
                if let Some(object) = state.state.as_object_mut() {
                    object.insert("isStreaming".into(), Value::Bool(true));
                    object.insert("isPromptRunning".into(), Value::Bool(true));
                }
            }
            Some("agent_settled") => {
                let aborted = event
                    .get("aborted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    || self.aborted.swap(false, Ordering::SeqCst);
                {
                    let mut state = self.state.lock();
                    state.running = false;
                    if let Some(object) = state.state.as_object_mut() {
                        object.insert("isStreaming".into(), Value::Bool(false));
                        object.insert("isPromptRunning".into(), Value::Bool(false));
                    }
                }
                if !aborted {
                    let callback = self.completion.lock().clone();
                    if let Some(callback) = callback {
                        let name = self
                            .state_json()
                            .get("sessionName")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                        callback(&self.id(), name.as_deref());
                    }
                }
                let _ = self.events.send(json!({
                    "type": "prompt_done",
                    "aborted": aborted,
                }));
            }
            Some("queue_update") => {
                let mut state = self.state.lock();
                if let Some(object) = state.state.as_object_mut() {
                    object.insert(
                        "queuedMessages".into(),
                        json!({
                            "steering": event.get("steering").cloned().unwrap_or_else(|| json!([])),
                            "followUp": event.get("followUp").cloned().unwrap_or_else(|| json!([])),
                        }),
                    );
                }
            }
            Some("session_info_changed") => {
                let mut state = self.state.lock();
                if let Some(object) = state.state.as_object_mut() {
                    match event.get("name").and_then(Value::as_str) {
                        Some(name) => {
                            object.insert("sessionName".into(), Value::String(name.to_string()));
                        }
                        None => {
                            object.remove("sessionName");
                        }
                    }
                }
            }
            _ => {}
        }
        let _ = self.events.send(event);
    }

    fn fail_all(&self, message: &str) {
        let pending = std::mem::take(&mut *self.pending.lock());
        for (_, pending) in pending {
            let _ = pending.sender.send(Err(message.to_string()));
        }
    }

    fn set_state(&self, data: Value) {
        let identity_change = {
            let session_id = data.get("sessionId").and_then(Value::as_str);
            let session_file = data.get("sessionFile").and_then(Value::as_str);
            if let (Some(session_id), Some(session_file)) = (session_id, session_file) {
                let mut identity = self.identity.lock();
                if identity.id != session_id || identity.path != PathBuf::from(session_file) {
                    let old = identity.id.clone();
                    identity.id = session_id.to_string();
                    identity.path = PathBuf::from(session_file);
                    Some((old, session_id.to_string(), PathBuf::from(session_file)))
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some((old, new, path)) = identity_change {
            let callback = self.rekey.lock().clone();
            if let Some(callback) = callback {
                callback(&old, &new, &path);
            }
        }
        let mut state = self.state.lock();
        let mut mapped = data.clone();
        let streaming = data
            .get("isStreaming")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        state.running = streaming;
        if let Some(object) = mapped.as_object_mut() {
            object.insert("running".into(), Value::Bool(true));
            object.insert("isPromptRunning".into(), Value::Bool(streaming));
            object.insert("isBashRunning".into(), Value::Bool(false));
            object
                .entry("extensionStatuses")
                .or_insert_with(|| json!([]));
            object
                .entry("extensionWidgets")
                .or_insert_with(|| json!([]));
            object
                .entry("queuedMessages")
                .or_insert_with(|| json!({"steering":[],"followUp":[]}));
            object
                .entry("contextUsage")
                .or_insert_with(|| json!({"percent":null,"contextWindow":0,"tokens":null}));
        }
        state.state = mapped;
    }

    pub fn id(&self) -> String {
        self.identity.lock().id.clone()
    }

    pub fn path(&self) -> PathBuf {
        self.identity.lock().path.clone()
    }

    /// Persist the active tool selection in the same `pi-web:tool-selection`
    /// custom entry pi-web uses. `None` records a cleared selection so a later
    /// reopen follows settings.json `defaultTools` again.
    pub fn append_tool_selection(&self, tools: Option<&[String]>) {
        let path = self.path();
        let Ok(session) = read_session(&path) else {
            return;
        };
        let data = match tools {
            Some(tools) => json!({"version": 1, "tools": tools}),
            None => json!({"version": 1, "cleared": true}),
        };
        let entry = new_entry(
            "custom",
            session.leaf_id().as_deref(),
            [
                (
                    "customType".to_string(),
                    Value::String("pi-web:tool-selection".into()),
                ),
                ("data".to_string(), data),
            ]
            .into_iter()
            .collect(),
        );
        let _ = append_lines(&path, &[entry]);
    }

    pub fn set_rekey(&self, callback: Arc<dyn Fn(&str, &str, &Path) + Send + Sync>) {
        *self.rekey.lock() = Some(callback);
    }

    pub fn is_running(&self) -> bool {
        self.state.lock().running
    }

    pub fn state_json(&self) -> Value {
        let state = self.state.lock();
        state.state.clone()
    }

    pub fn touch(&self) {
        *self.last_activity.lock() = Instant::now();
    }

    pub fn idle_for(&self) -> Duration {
        self.last_activity.lock().elapsed()
    }

    pub fn pending_extension_ui_ids(&self) -> Vec<String> {
        self.state.lock().pending_ui.iter().cloned().collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.events.subscribe()
    }

    pub async fn request(&self, mut command: Value, timeout: Duration) -> Result<Value, String> {
        self.touch();
        if command.get("id").is_none() {
            let id = format!("pi-star-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
            command["id"] = Value::String(id);
        }
        let id = command
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let command_name = command
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().insert(
            id.clone(),
            Pending {
                sender,
                command: command_name.clone(),
            },
        );
        let mut line = command.to_string();
        line.push('\n');
        {
            let mut stdin = self.stdin.lock().await;
            let Some(stdin) = stdin.as_mut() else {
                self.pending.lock().remove(&id);
                return Err("pi RPC stdin is closed".into());
            };
            if let Err(error) = stdin.write_all(line.as_bytes()).await {
                self.pending.lock().remove(&id);
                return Err(error.to_string());
            }
            if let Err(error) = stdin.flush().await {
                self.pending.lock().remove(&id);
                return Err(error.to_string());
            }
        }
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("pi RPC process exited".into()),
            Err(_) => {
                self.pending.lock().remove(&id);
                Err("pi RPC command timed out".into())
            }
        }
    }

    pub async fn send(&self, command: &Value) -> Result<Value, String> {
        let kind = command.get("type").and_then(Value::as_str).unwrap_or("");
        if kind == "prompt" {
            self.aborted.store(false, Ordering::SeqCst);
        } else if kind == "abort" {
            self.aborted.store(true, Ordering::SeqCst);
        }
        let result = match kind {
            "extension_ui_response" => self.send_extension_ui_response(command.clone()).await,
            "get_tools" => self.host_command("get_tools", json!({})).await,
            "set_tools" => {
                let requested = command.get("toolNames").and_then(Value::as_array);
                let names: Vec<String> = match requested {
                    Some(items) => items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect(),
                    // No selection means "back to settings.json defaults", not
                    // "turn every tool off": resolve the same default loadout
                    // pi does and record a cleared marker for future opens.
                    None => pi_star_core::prompt::default_tool_names(&self.config, &self.cwd),
                };
                let result = self
                    .host_command("set_tools", json!({"toolNames": names}))
                    .await;
                if result.is_ok() {
                    if requested.is_some() {
                        self.append_tool_selection(Some(&names));
                    } else {
                        self.append_tool_selection(None);
                    }
                }
                result
            }
            "navigate_tree" => self.host_command("navigate_tree", command.clone()).await,
            "fork_branch" => self.host_command("fork_branch", command.clone()).await,
            "reload" => self.host_command("reload", json!({})).await,
            "abort_compaction" => self.host_command("abort_compaction", json!({})).await,
            _ => self.request(command.clone(), REQUEST_TIMEOUT).await,
        }?;
        if matches!(
            kind,
            "fork" | "fork_branch" | "clone" | "new_session" | "switch_session"
        ) {
            let _ = self
                .request(json!({"type": "get_state"}), REQUEST_TIMEOUT)
                .await;
            if matches!(kind, "fork" | "fork_branch") {
                let mut result = result;
                if let Some(object) = result.as_object_mut() {
                    object.insert("newSessionId".into(), Value::String(self.id()));
                }
                return Ok(result);
            }
        }
        Ok(result)
    }

    async fn host_command(&self, method: &str, params: Value) -> Result<Value, String> {
        if !self.extension_available {
            return Err(format!(
                "pi-star agent extension is required for {method}; set PI_STAR_AGENT_EXTENSION"
            ));
        }
        let id = self.control.next_id();
        let receiver = self.control.register(&id);
        let request = json!({"id": id, "method": method, "params": params});
        let prompt = format!("/pi-star-host {}", request);
        let response = self
            .request(
                json!({"type": "prompt", "message": prompt}),
                REQUEST_TIMEOUT,
            )
            .await;
        if let Err(error) = response {
            self.control.cancel(&id);
            return Err(error);
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("pi-star agent extension disconnected".into()),
            Err(_) => {
                self.control.cancel(&id);
                Err(format!("{method} timed out"))
            }
        }
    }

    async fn send_extension_ui_response(&self, mut response: Value) -> Result<Value, String> {
        response["type"] = Value::String("extension_ui_response".into());
        if let Some(id) = response.get("id").and_then(Value::as_str) {
            self.state.lock().pending_ui.remove(id);
        }
        let mut line = response.to_string();
        line.push('\n');
        let mut stdin = self.stdin.lock().await;
        let Some(stdin) = stdin.as_mut() else {
            return Err("pi RPC stdin is closed".into());
        };
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        stdin.flush().await.map_err(|error| error.to_string())?;
        Ok(Value::Null)
    }

    pub fn set_completion_callback(&self, callback: Arc<dyn Fn(&str, Option<&str>) + Send + Sync>) {
        *self.completion.lock() = Some(callback);
    }

    pub fn debug_snapshot(&self) -> Value {
        json!({
            "sessionId": self.id(),
            "sessionFile": self.path().to_string_lossy(),
            "cwd": self.cwd.to_string_lossy(),
            "running": self.is_running(),
            "backend": "pi-rpc",
            "uptimeMs": self.started_at.elapsed().as_millis() as u64,
        })
    }

    pub fn shutdown(&self) {
        if let Some(mut child) = self.child.lock().take() {
            let _ = child.start_kill();
        }
        let _ = self.stdin.try_lock().map(|mut stdin| stdin.take());
    }
}

fn agent_extension_path() -> Option<PathBuf> {
    if let Ok(value) = std::env::var("PI_STAR_AGENT_EXTENSION") {
        let path = PathBuf::from(value);
        if path.exists() {
            return Some(path);
        }
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("agent-extension").join("index.ts"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("agent-extension").join("index.ts"));
    }
    if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR") {
        candidates.push(
            PathBuf::from(manifest)
                .join("..")
                .join("..")
                .join("agent-extension")
                .join("index.ts"),
        );
    }
    candidates.into_iter().find(|path| path.exists())
}

/// The newest valid `pi-web:tool-selection` entry, or `None` when the session
/// follows settings.json defaults (no entry, or a later cleared marker).
fn persisted_tool_selection(path: &Path) -> Option<Vec<String>> {
    let session = read_session(path).ok()?;
    for entry in session.entries.iter().rev() {
        if entry.get("type").and_then(Value::as_str) != Some("custom")
            || entry.get("customType").and_then(Value::as_str) != Some("pi-web:tool-selection")
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
        if !tools.iter().all(Value::is_string) {
            continue;
        }
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

pub struct PiRpcRegistry {
    pub config: Config,
    sessions: Arc<Mutex<HashMap<String, Arc<PiRpcSession>>>>,
    cli: String,
    extension: Option<PathBuf>,
    extra_env: Vec<(String, String)>,
    max_sessions: usize,
    completion: Mutex<Option<Arc<dyn Fn(&str, Option<&str>) + Send + Sync>>>,
}

impl PiRpcRegistry {
    pub fn new(config: Config) -> Self {
        let max_sessions = std::env::var("PI_STAR_PI_RPC_MAX_SESSIONS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(8);
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            cli: std::env::var("PI_STAR_PI_CLI").unwrap_or_else(|_| "pi".to_string()),
            extension: agent_extension_path(),
            extra_env: Vec::new(),
            max_sessions,
            completion: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub fn new_with_cli(config: Config, cli: impl Into<String>) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            cli: cli.into(),
            extension: agent_extension_path(),
            extra_env: Vec::new(),
            max_sessions: 8,
            completion: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub fn new_with_options(
        config: Config,
        cli: impl Into<String>,
        extension: Option<PathBuf>,
        extra_env: Vec<(String, String)>,
        max_sessions: usize,
    ) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            cli: cli.into(),
            extension,
            extra_env,
            max_sessions,
            completion: Mutex::new(None),
        }
    }

    pub fn get(&self, id: &str) -> Option<Arc<PiRpcSession>> {
        self.sessions.lock().get(id).cloned()
    }

    pub fn ids(&self) -> Vec<String> {
        self.sessions.lock().keys().cloned().collect()
    }

    pub fn running_ids(&self) -> Vec<String> {
        self.sessions
            .lock()
            .values()
            .filter(|session| session.is_running())
            .map(|session| session.id())
            .collect()
    }

    pub fn awaiting_input_ids(&self) -> Vec<String> {
        self.sessions
            .lock()
            .values()
            .filter(|session| !session.pending_extension_ui_ids().is_empty())
            .map(|session| session.id())
            .collect()
    }

    pub fn remove(&self, id: &str) {
        if let Some(session) = self.sessions.lock().remove(id) {
            session.shutdown();
        }
    }

    pub fn set_completion_callback(&self, callback: Arc<dyn Fn(&str, Option<&str>) + Send + Sync>) {
        *self.completion.lock() = Some(callback.clone());
        let sessions: Vec<Arc<PiRpcSession>> = self.sessions.lock().values().cloned().collect();
        for session in sessions {
            session.set_completion_callback(callback.clone());
        }
    }

    fn make_room(&self) -> Result<()> {
        if self.sessions.lock().len() < self.max_sessions {
            return Ok(());
        }
        let mut idle: Vec<(String, Duration)> = self
            .sessions
            .lock()
            .iter()
            .filter(|(_, session)| {
                !session.is_running() && session.pending_extension_ui_ids().is_empty()
            })
            .map(|(id, session)| (id.clone(), session.idle_for()))
            .collect();
        idle.sort_by(|a, b| b.1.cmp(&a.1));
        for (id, _) in idle {
            if self.sessions.lock().len() < self.max_sessions {
                break;
            }
            self.remove(&id);
        }
        if self.sessions.lock().len() >= self.max_sessions {
            return Err(anyhow!(
                "pi RPC session limit reached ({}); close idle sessions or raise PI_STAR_PI_RPC_MAX_SESSIONS",
                self.max_sessions
            ));
        }
        Ok(())
    }

    pub fn shutdown(&self) {
        let sessions: Vec<Arc<PiRpcSession>> = self.sessions.lock().values().cloned().collect();
        for session in sessions {
            session.shutdown();
        }
    }

    pub fn reap_idle(&self, timeout: Duration) -> Vec<String> {
        if timeout.is_zero() {
            return Vec::new();
        }
        let idle: Vec<String> = self
            .sessions
            .lock()
            .iter()
            .filter(|(_, session)| !session.is_running() && session.idle_for() >= timeout)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &idle {
            self.remove(id);
        }
        idle
    }

    #[allow(dead_code)] // used by the RPC integration tests
    pub async fn create(&self, cwd: &Path, options: PiSpawnOptions) -> Result<Arc<PiRpcSession>> {
        let path = create_session_file(&self.config, cwd)?;
        self.open_path(&path, options).await
    }

    pub async fn open(&self, id: &str, options: PiSpawnOptions) -> Result<Arc<PiRpcSession>> {
        let path =
            resolve_session_path(&self.config, id).ok_or_else(|| anyhow!("Session not found"))?;
        self.open_path(&path, options).await
    }

    pub async fn open_or_create(&self, id: &str) -> Result<Arc<PiRpcSession>> {
        if let Some(session) = self.get(id) {
            return Ok(session);
        }
        self.open(id, PiSpawnOptions::default()).await
    }

    pub(crate) async fn open_path(
        &self,
        path: &Path,
        options: PiSpawnOptions,
    ) -> Result<Arc<PiRpcSession>> {
        let requested_tools = options.tool_names.clone();
        self.make_room()?;
        let session = read_session(path)?;
        let id = session.id().to_string();
        let lock = self
            .config
            .agent_dir
            .join("pi-star-subagent-locks")
            .join(format!("{id}.json"));
        if lock.is_file() {
            return Err(anyhow!(
                "Session {id} is owned by a running subagent process"
            ));
        }
        if let Some(existing) = self.get(&id) {
            return Ok(existing);
        }
        let cwd = PathBuf::from(session.cwd());
        let control_token = uuid::Uuid::new_v4().to_string();
        let (control, control_addr) = PiControl::start(control_token.clone())
            .await
            .map_err(|error| anyhow!("start pi-star control channel: {error}"))?;
        let extension = self.extension.clone();
        let mut command = Command::new(&self.cli);
        command
            .arg("--mode")
            .arg("rpc")
            .arg("--session")
            .arg(path)
            .current_dir(&cwd)
            .env("PI_STAR_CONTROL_ADDR", control_addr.to_string())
            .env("PI_STAR_CONTROL_TOKEN", control_token)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (key, value) in &self.extra_env {
            command.env(key, value);
        }
        if let Some(extension) = &extension {
            command.arg("--extension").arg(extension);
        }
        if let Some(name) = &options.name {
            command.arg("--name").arg(name);
        }
        if let Some(tools) = &options.tool_names {
            if tools.is_empty() {
                // A Chat-only composer sends an empty list, not "use the default".
                command.arg("--no-tools");
            } else {
                command.arg("--tools").arg(tools.join(","));
            }
        }
        let mut child = command
            .spawn()
            .map_err(|error| anyhow!("start pi rpc: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("pi rpc stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("pi rpc stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("pi rpc stderr unavailable"))?;
        let session = PiRpcSession::new(
            id.clone(),
            path.to_path_buf(),
            cwd,
            child,
            stdin,
            stdout,
            stderr,
            control,
            extension.is_some(),
            self.config.clone(),
        );
        if let Some(provider) = options.provider {
            if let Some(model_id) = options.model_id {
                let _ = session
                    .request(
                        json!({"type":"set_model","provider":provider,"modelId":model_id}),
                        REQUEST_TIMEOUT,
                    )
                    .await;
            }
        }
        if let Some(level) = options.thinking_level {
            let _ = session
                .request(
                    json!({"type":"set_thinking_level","level":level}),
                    REQUEST_TIMEOUT,
                )
                .await;
        }
        if let Some(callback) = self.completion.lock().clone() {
            session.set_completion_callback(callback);
        }
        let sessions = self.sessions.clone();
        session.set_rekey(Arc::new(move |old, new, _path| {
            let mut sessions = sessions.lock();
            if let Some(session) = sessions.remove(old) {
                sessions.insert(new.to_string(), session);
            }
        }));
        let _ = session
            .request(json!({"type":"get_state"}), REQUEST_TIMEOUT)
            .await;
        if let Some(tools) = requested_tools {
            session.append_tool_selection(Some(&tools));
        } else if let Some(selection) = persisted_tool_selection(path) {
            // Apply the pi-web custom entry to the live loadout. The entry may
            // be an empty list, which pi-web stores for a Chat-only session.
            let _ = session
                .host_command("set_tools", json!({"toolNames": selection}))
                .await;
        }
        let id = session.id();
        self.sessions.lock().insert(id, session.clone());
        Ok(session)
    }
}
