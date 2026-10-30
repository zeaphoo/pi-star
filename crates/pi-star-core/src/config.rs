use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Runtime configuration shared by the HTTP server and embedded callers.
#[derive(Debug, Clone)]
pub struct Config {
    /// pi's agent directory (`$PI_CODING_AGENT_DIR` or `~/.pi/agent`).
    pub agent_dir: PathBuf,
    /// pi's session files live here.
    pub sessions_dir: PathBuf,
    /// Directory containing the built UI (`index.html` + assets).
    pub ui_dir: PathBuf,
}

impl Config {
    pub fn new(agent_dir: Option<PathBuf>, ui_dir: Option<PathBuf>) -> Result<Self> {
        let agent_dir = match agent_dir {
            Some(path) => path,
            None => default_agent_dir()?,
        };
        let sessions_dir = agent_dir.join("sessions");
        let ui_dir = ui_dir.unwrap_or_else(|| PathBuf::from("ui/dist"));
        Ok(Self {
            agent_dir,
            sessions_dir,
            ui_dir,
        })
    }

    /// `pi-web-session-state.json` beside pi's own files.
    pub fn ui_state_path(&self) -> PathBuf {
        self.agent_dir.join("pi-web-session-state.json")
    }

    pub fn settings_path(&self) -> PathBuf {
        self.agent_dir.join("settings.json")
    }

    pub fn models_path(&self) -> PathBuf {
        self.agent_dir.join("models.json")
    }
}

pub fn default_agent_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("PI_CODING_AGENT_DIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    let home = home_dir().context("cannot determine the home directory")?;
    Ok(home.join(".pi").join("agent"))
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

/// Encode a cwd the way pi names a session directory:
/// `/home/me/repo` -> `--home-me-repo--`.
pub fn encode_cwd(cwd: &Path) -> String {
    let text = cwd.to_string_lossy().replace('\\', "/");
    let trimmed = text.trim_matches('/');
    let mut encoded = String::from("--");
    encoded.push_str(&trimmed.replace('/', "-"));
    encoded.push_str("--");
    encoded
}

/// Find a session file by id without trusting the caller's path.
pub fn session_file_for_id(sessions_dir: &Path, id: &str) -> Option<PathBuf> {
    // pi ids are UUID-like, but older files may use another opaque id.
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        return None;
    }
    let Ok(projects) = std::fs::read_dir(sessions_dir) else {
        return None;
    };
    for project in projects.flatten() {
        if !project.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let Ok(files) = std::fs::read_dir(project.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem == id || stem.ends_with(&format!("_{id}")) {
                    return Some(path);
                }
            }
        }
    }
    None
}
