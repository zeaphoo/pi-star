use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const SESSION_UI_STATE_VERSION: u32 = 1;
const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUiFamilyState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUiProjectState {
    pub pinned_at: f64,
    pub root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUiState {
    pub version: u32,
    pub revision: u64,
    #[serde(default)]
    pub sessions: BTreeMap<String, SessionUiFamilyState>,
    #[serde(default)]
    pub projects: BTreeMap<String, SessionUiProjectState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub project_order: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub project_names: BTreeMap<String, String>,
    /// Unknown fields are preserved for newer pi-star builds.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for SessionUiState {
    fn default() -> Self {
        Self {
            version: SESSION_UI_STATE_VERSION,
            revision: 0,
            sessions: BTreeMap::new(),
            projects: BTreeMap::new(),
            project_order: Vec::new(),
            project_names: BTreeMap::new(),
            extra: Map::new(),
        }
    }
}

impl SessionUiState {
    fn normalize(mut self) -> Self {
        self.version = SESSION_UI_STATE_VERSION;
        self.sessions.retain(|id, state| {
            !id.is_empty() && (state.pinned_at.is_some() || state.archived_at.is_some())
        });
        self.projects
            .retain(|key, state| !key.is_empty() && state.pinned_at.is_finite());
        self.project_order
            .retain(|key| !key.is_empty() && key != "__proto__");
        let mut seen = BTreeSet::new();
        self.project_order.retain(|key| seen.insert(key.clone()));
        self.project_names.retain(|key, name| {
            !key.is_empty()
                && key != "__proto__"
                && !name.trim().is_empty()
                && name.chars().count() <= 80
        });
        self
    }
}

pub struct UiStateStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl UiStateStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    pub fn read(&self) -> SessionUiState {
        match self.load() {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(path = %self.path.display(), %error, "could not read session UI state");
                SessionUiState::default()
            }
        }
    }

    pub fn revision(&self) -> u64 {
        self.read().revision
    }

    pub fn apply(&self, request: &Value) -> Result<SessionUiState> {
        let _guard = self.lock.lock();
        let mut state = self.load_for_write()?;
        let changed = apply_request(&mut state, request)?;
        if changed {
            state.revision = state.revision.saturating_add(1);
            self.write(&state)?;
        }
        Ok(state)
    }

    fn load(&self) -> Result<SessionUiState> {
        match fs::metadata(&self.path) {
            Ok(metadata) if metadata.len() > MAX_BYTES => {
                return Err(anyhow!("session UI state is too large"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SessionUiState::default());
            }
            Err(error) => return Err(error.into()),
        }
        let text = fs::read_to_string(&self.path)?;
        let value: Value = serde_json::from_str(&text).context("parse session UI state")?;
        let state: SessionUiState =
            serde_json::from_value(value).context("read session UI state")?;
        if state.version != SESSION_UI_STATE_VERSION {
            return Ok(SessionUiState::default());
        }
        Ok(state.normalize())
    }

    fn load_for_write(&self) -> Result<SessionUiState> {
        match self.load() {
            Ok(state) => Ok(state),
            Err(error) => {
                tracing::warn!(path = %self.path.display(), %error, "setting corrupt UI state aside");
                let backup = self
                    .path
                    .with_extension(format!("corrupt-{}", chrono::Utc::now().timestamp_millis()));
                let _ = fs::rename(&self.path, backup);
                Ok(SessionUiState::default())
            }
        }
    }

    fn write(&self, state: &SessionUiState) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = self.path.with_file_name(format!(
            ".{}-{}.tmp",
            self.path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("state"),
            uuid::Uuid::new_v4()
        ));
        let bytes = serde_json::to_vec_pretty(state)?;
        {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
            }
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
        }
        fs::rename(&temp, &self.path)
            .with_context(|| format!("replace {}", self.path.display()))?;
        Ok(())
    }
}

fn apply_request(state: &mut SessionUiState, request: &Value) -> Result<bool> {
    let action = request
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("action is required"))?;
    let now = chrono::Utc::now().timestamp_millis() as f64;
    match action {
        "set" => {
            let ids = request
                .get("ids")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("ids must be an array"))?;
            let pinned = request.get("pinned").and_then(Value::as_bool);
            let archived = request.get("archived").and_then(Value::as_bool);
            if pinned.is_none() && archived.is_none() {
                return Err(anyhow!("pinned or archived is required"));
            }
            for id in ids {
                let Some(id) = id.as_str() else { continue };
                if id.is_empty() || id.contains('/') || id.contains('\\') {
                    continue;
                }
                let entry = state.sessions.entry(id.to_string()).or_default();
                if let Some(pinned) = pinned {
                    entry.pinned_at = pinned.then_some(now);
                }
                if let Some(archived) = archived {
                    entry.archived_at = archived.then_some(now);
                }
            }
            state
                .sessions
                .retain(|_, entry| entry.pinned_at.is_some() || entry.archived_at.is_some());
            Ok(true)
        }
        "restore" => {
            let entries = request
                .get("entries")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("entries must be an array"))?;
            for entry in entries {
                let Some(id) = entry.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let pinned_at = entry.get("pinnedAt").and_then(Value::as_f64);
                let archived_at = entry.get("archivedAt").and_then(Value::as_f64);
                if pinned_at.is_none() && archived_at.is_none() {
                    state.sessions.remove(id);
                } else {
                    state.sessions.insert(
                        id.to_string(),
                        SessionUiFamilyState {
                            pinned_at,
                            archived_at,
                        },
                    );
                }
            }
            Ok(true)
        }
        "pin-project" => {
            let key = request
                .get("projectKey")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("projectKey is required"))?;
            let root = request
                .get("root")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("root is required"))?;
            let pinned = request
                .get("pinned")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if pinned {
                state.projects.insert(
                    key.to_string(),
                    SessionUiProjectState {
                        pinned_at: now,
                        root: root.to_string(),
                    },
                );
            } else {
                state.projects.remove(key);
            }
            Ok(true)
        }
        "add-projects" => {
            let keys = request
                .get("keys")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("keys must be an array"))?;
            let existing: BTreeSet<String> = state.project_order.iter().cloned().collect();
            let mut front = Vec::new();
            for key in keys {
                let Some(key) = key.as_str() else { continue };
                if !existing.contains(key)
                    && key != "__proto__"
                    && !front.iter().any(|k: &String| k == key)
                {
                    front.push(key.to_string());
                }
            }
            front.extend(state.project_order.iter().cloned());
            state.project_order = front;
            Ok(!state.project_order.is_empty())
        }
        "move-project" => {
            let key = request
                .get("projectKey")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("projectKey is required"))?;
            let anchor = request
                .get("anchorKey")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("anchorKey is required"))?;
            let position = request
                .get("position")
                .and_then(Value::as_str)
                .unwrap_or("before");
            let add = request
                .get("add")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !state.project_order.iter().any(|k| k == key) {
                state.project_order.push(key.to_string());
            }
            for extra in add.iter().filter_map(Value::as_str) {
                if !state.project_order.iter().any(|k| k == extra) {
                    state.project_order.push(extra.to_string());
                }
            }
            state.project_order.retain(|k| k != key);
            let anchor_index = state.project_order.iter().position(|k| k == anchor);
            let index = match (anchor_index, position) {
                (Some(index), "after") => index + 1,
                (Some(index), _) => index,
                (None, _) => state.project_order.len(),
            };
            state
                .project_order
                .insert(index.min(state.project_order.len()), key.to_string());
            Ok(true)
        }
        "rename-project" => {
            let key = request
                .get("projectKey")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("projectKey is required"))?;
            let name = request
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if name.is_empty() {
                state.project_names.remove(key);
            } else {
                state
                    .project_names
                    .insert(key.to_string(), name.chars().take(80).collect());
            }
            Ok(true)
        }
        _ => Err(anyhow!("unknown action: {action}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pins_and_revision() {
        let temp = tempfile::tempdir().unwrap();
        let store = UiStateStore::new(temp.path().join("state.json"));
        let state = store
            .apply(&json!({"action":"set","ids":["abc"],"pinned":true}))
            .unwrap();
        assert_eq!(state.revision, 1);
        assert!(state.sessions.get("abc").unwrap().pinned_at.is_some());
        let state = store
            .apply(&json!({"action":"set","ids":["abc"],"pinned":false}))
            .unwrap();
        assert!(state.sessions.get("abc").is_none());
        assert_eq!(state.revision, 2);
    }
}
