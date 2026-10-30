use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use anyhow::Result;
use pi_star_core::Config;
use pi_star_core::agent::AgentRegistry;
use pi_star_core::file_access::AllowedRoots;
use pi_star_core::models::{ModelsData, load_models};
use pi_star_core::push::PushStore;
use pi_star_core::session_index::SessionIndex;
use pi_star_core::terminal::TerminalManager;
use pi_star_core::ui_state::UiStateStore;

use crate::debug_streams::DebugStreams;

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
    pub roots: Arc<AllowedRoots>,
    pub agents: Arc<AgentRegistry>,
    pub terminals: Arc<TerminalManager>,
    pub ui_state: Arc<UiStateStore>,
    pub session_index: Arc<SessionIndex>,
    pub push: Arc<PushStore>,
    pub models_cache: Mutex<Option<(Instant, ModelsData)>>,
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
        ui_dir: PathBuf,
        hostname: &str,
    ) -> Result<Arc<Self>> {
        Self::new_with_password(agent_dir, ui_dir, hostname, None).await
    }

    pub(crate) async fn new_with_password(
        agent_dir: Option<PathBuf>,
        ui_dir: PathBuf,
        _hostname: &str,
        password: Option<String>,
    ) -> Result<Arc<Self>> {
        let config = Config::new(agent_dir, Some(ui_dir))?;
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
        agents.set_max_subagents(
            pi_star_core::settings::subagent_settings(&config).max_concurrent as usize,
        );
        {
            let push = push.clone();
            agents.set_completion_callback(Arc::new(move |session_id, session_name| {
                let push = push.clone();
                let session_id = session_id.to_string();
                let session_name = session_name.map(str::to_owned);
                tokio::spawn(async move {
                    push.notify_session_complete(&session_id, session_name.as_deref())
                        .await;
                });
            }));
        }
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
            roots,
            agents,
            terminals,
            ui_state,
            session_index,
            push,
            models_cache: Mutex::new(None),
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
        self.terminals.kill_all();
    }
}
