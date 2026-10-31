use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use anyhow::{Result, anyhow};
use pi_star_core::Config;
use pi_star_core::agent::AgentRegistry;
use pi_star_core::agent::backend::{self, BackendKind};
use pi_star_core::file_access::AllowedRoots;
use pi_star_core::model_catalog::CatalogEntry;
use pi_star_core::models::{ModelsData, load_models};
use pi_star_core::push::PushStore;
use pi_star_core::session_index::SessionIndex;
use pi_star_core::terminal::TerminalManager;
use pi_star_core::ui_state::UiStateStore;

use crate::debug_streams::DebugStreams;
use crate::pi_rpc::PiRpcRegistry;
use crate::web_assets::WebAssets;

#[derive(Default)]
pub struct AuthThrottle {
    failures: u32,
    blocked_until: Option<Instant>,
}

impl AuthThrottle {
    /// Seconds a caller must wait, or None when it may try.
    pub fn retry_after(&self) -> Option<u64> {
        let until = self.blocked_until?;
        let now = Instant::now();
        if until <= now {
            None
        } else {
            Some((until - now).as_secs().max(1))
        }
    }

    pub fn fail(&mut self) -> u64 {
        self.failures = self.failures.saturating_add(1);
        let delay = Duration::from_secs((1u64 << self.failures.min(5)).min(60));
        self.blocked_until = Some(Instant::now() + delay);
        delay.as_secs()
    }

    pub fn succeed(&mut self) {
        self.failures = 0;
        self.blocked_until = None;
    }
}

pub(crate) struct PendingOAuth {
    pub sender: Option<tokio::sync::oneshot::Sender<String>>,
}

pub struct AppState {
    pub config: Config,
    /// 前端静态资源来源（release 内置 / debug 磁盘 / `--ui-dir` 覆盖）。
    pub web_assets: WebAssets,
    pub roots: Arc<AllowedRoots>,
    pub agents: Arc<AgentRegistry>,
    /// Standard pi sessions run through `pi --mode rpc`; QuickPI sessions stay
    /// in `agents`. The per-session choice lives in the session file.
    pub pi_rpc: Arc<PiRpcRegistry>,
    pub terminals: Arc<TerminalManager>,
    pub ui_state: Arc<UiStateStore>,
    pub session_index: Arc<SessionIndex>,
    pub push: Arc<PushStore>,
    pub models_cache: Mutex<Option<(Instant, ModelsData)>>,
    pub catalog_cache: Mutex<Option<(Instant, Vec<CatalogEntry>)>>,
    pub update_cache: Mutex<Option<(Instant, serde_json::Value)>>,
    pub started: Instant,
    /// Process start on the wall clock, for the Debug panel (epoch ms).
    pub started_epoch_ms: i64,
    /// `PI_WEB_IDLE_TIMEOUT_MS` in effect; 0 means disabled.
    pub idle_timeout_ms: u64,
    pub debug_streams: Arc<DebugStreams>,
    pub password: Option<String>,
    pub auth_token: String,
    pub allowed_hosts: Vec<String>,
    pub auth_throttle: Mutex<AuthThrottle>,
    pub(crate) oauth_flows: Mutex<HashMap<String, PendingOAuth>>,
    pub(crate) mcp_signins: Mutex<HashMap<String, crate::routes::mcp_oauth::McpSignInFlow>>,
    server_port: AtomicU16,
}

impl AppState {
    pub async fn new(
        agent_dir: Option<PathBuf>,
        ui_dir: Option<PathBuf>,
        hostname: &str,
    ) -> Result<Arc<Self>> {
        Self::new_with_password(agent_dir, ui_dir, hostname, None).await
    }

    pub(crate) async fn new_with_password(
        agent_dir: Option<PathBuf>,
        ui_dir: Option<PathBuf>,
        _hostname: &str,
        password: Option<String>,
    ) -> Result<Arc<Self>> {
        let web_assets = WebAssets::resolve(ui_dir.as_deref()).map_err(anyhow::Error::msg)?;
        let config = Config::new(agent_dir, ui_dir)?;
        std::fs::create_dir_all(&config.agent_dir).ok();
        let roots = Arc::new(AllowedRoots::new());
        let session_index = Arc::new(SessionIndex::new(&config));
        for session in session_index.scan(&config, true) {
            roots.add(&session.cwd);
            if let Some(project_root) = &session.project_root {
                roots.add(project_root);
            }
        }
        let push = Arc::new(PushStore::load(config.agent_dir.join("web-push.json")));
        let agents = Arc::new(AgentRegistry::new(config.clone(), roots.clone()));
        let pi_rpc = Arc::new(PiRpcRegistry::new(config.clone()));
        agents.set_max_subagents(
            pi_star_core::settings::subagent_settings(&config).max_concurrent as usize,
        );
        let completion: Arc<dyn Fn(&str, Option<&str>) + Send + Sync> = {
            let push = push.clone();
            Arc::new(move |session_id, session_name| {
                let push = push.clone();
                let session_id = session_id.to_string();
                let session_name = session_name.map(str::to_owned);
                tokio::spawn(async move {
                    push.notify_session_complete(&session_id, session_name.as_deref())
                        .await;
                });
            })
        };
        agents.set_completion_callback(completion.clone());
        pi_rpc.set_completion_callback(completion);
        let ui_state = Arc::new(UiStateStore::new(config.ui_state_path()));
        let terminals = Arc::new(TerminalManager::new());
        let password = password
            .or_else(|| std::env::var("PI_WEB_PASSWORD").ok())
            .filter(|value| !value.is_empty());
        let mut allowed_hosts = vec![
            "127.0.0.1".to_string(),
            "localhost".to_string(),
            "::1".to_string(),
            "[::1]".to_string(),
        ];
        if let Ok(extra) = std::env::var("PI_WEB_ALLOWED_HOSTS") {
            allowed_hosts.extend(
                extra
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned),
            );
        }
        let idle_timeout_ms = std::env::var("PI_WEB_IDLE_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(600_000);
        Ok(Arc::new(Self {
            config,
            web_assets,
            roots,
            agents,
            pi_rpc,
            terminals,
            ui_state,
            session_index,
            push,
            models_cache: Mutex::new(None),
            catalog_cache: Mutex::new(None),
            update_cache: Mutex::new(None),
            started: Instant::now(),
            started_epoch_ms: crate::debug_streams::now_ms(),
            idle_timeout_ms,
            debug_streams: Arc::new(DebugStreams::new()),
            password,
            auth_token: uuid::Uuid::new_v4().to_string(),
            allowed_hosts,
            auth_throttle: Mutex::new(AuthThrottle::default()),
            oauth_flows: Mutex::new(HashMap::new()),
            mcp_signins: Mutex::new(HashMap::new()),
            server_port: AtomicU16::new(
                std::env::var("PORT")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(30151),
            ),
        }))
    }

    /// The backend persisted for `id`; missing sessions and missing entries
    /// both mean the standard Node.js pi backend.
    pub fn backend_for(&self, id: &str) -> BackendKind {
        self.session_path(id)
            .map(|path| backend::session_backend(&path))
            .unwrap_or_default()
    }

    pub fn session_path(&self, id: &str) -> Option<PathBuf> {
        pi_star_core::session::resolve_session_path(&self.config, id)
    }

    /// The registry a live wrapper belongs to, if one exists. Subagent
    /// sessions have no persisted choice of their own, so the live runtime is
    /// what says which backend owns them.
    pub fn active_backend(&self, id: &str) -> Option<BackendKind> {
        if self.pi_rpc.get(id).is_some() {
            Some(BackendKind::Rpc)
        } else if self.agents.get(id).is_some() {
            Some(BackendKind::Quick)
        } else {
            None
        }
    }

    /// The backend that owns this session now: a live wrapper first, then the
    /// persisted choice, then the standard pi default.
    pub fn route_backend(&self, id: &str) -> BackendKind {
        self.active_backend(id)
            .unwrap_or_else(|| self.backend_for(id))
    }

    /// Whether either registry has work under way for this session.
    pub fn session_running(&self, id: &str) -> bool {
        self.agents
            .get(id)
            .is_some_and(|runtime| runtime.is_running())
            || self
                .pi_rpc
                .get(id)
                .is_some_and(|session| session.is_running())
    }

    /// Persist a new backend for a session that is not running, dropping any
    /// idle wrapper so the next command starts in the selected registry.
    pub fn set_backend(&self, id: &str, kind: BackendKind) -> Result<BackendKind> {
        if self.session_running(id) {
            return Err(anyhow!(
                "Cannot change the agent backend while the session is running"
            ));
        }
        let path = self
            .session_path(id)
            .ok_or_else(|| anyhow!("Session not found"))?;
        backend::set_session_backend(&path, kind)?;
        self.agents.remove(id);
        self.pi_rpc.remove(id);
        Ok(kind)
    }

    pub fn set_server_port(&self, port: u16) {
        self.server_port.store(port, Ordering::Relaxed);
    }

    pub fn server_port(&self) -> u16 {
        self.server_port.load(Ordering::Relaxed)
    }

    pub fn models(&self) -> anyhow::Result<ModelsData> {
        {
            let cache = self.models_cache.lock();
            if let Some((at, data)) = cache.as_ref() {
                if at.elapsed() < Duration::from_secs(5) {
                    return Ok(data.clone());
                }
            }
        }
        let data = load_models(&self.config)?;
        *self.models_cache.lock() = Some((Instant::now(), data.clone()));
        Ok(data)
    }

    pub fn invalidate_models(&self) {
        *self.models_cache.lock() = None;
    }

    pub fn shutdown(&self) {
        self.agents.shutdown();
        self.pi_rpc.shutdown();
        self.terminals.kill_all();
    }
}
