use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::Config;
use crate::session::{self, SessionInfo};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedInfo {
    size: u64,
    mtime_ms: u64,
    info: SessionInfo,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct IndexFile {
    version: u32,
    entries: HashMap<String, CachedInfo>,
}

/// pi-web's session index: unchanged JSONL files reuse their previous row so a
/// sidebar poll does not parse every transcript again. The index is a cache —
/// a missing, corrupt or stale file only costs a re-read.
pub struct SessionIndex {
    path: PathBuf,
    entries: Mutex<HashMap<String, CachedInfo>>,
    dirty: AtomicBool,
    last_persist: Mutex<SystemTime>,
}

impl SessionIndex {
    pub fn new(config: &Config) -> Self {
        let path = config.agent_dir.join("pi-star-session-index.json");
        let entries = load_index(&path);
        Self {
            path,
            entries: Mutex::new(entries),
            dirty: AtomicBool::new(false),
            last_persist: Mutex::new(UNIX_EPOCH),
        }
    }

    pub fn scan(&self, config: &Config, summary: bool) -> Vec<SessionInfo> {
        let files = session_files(&config.sessions_dir);
        let cached = self.entries.lock().clone();
        let mut next = HashMap::with_capacity(files.len());
        let mut changed = false;

        // Phase 1: decide which files need parsing. Doing the stat calls before
        // the parallel parse keeps the common "nothing changed" path cheap.
        let mut todo = Vec::new();
        for path in &files {
            let key = path.to_string_lossy().into_owned();
            let fingerprint = fingerprint(path);
            if let (Some(cached), Some(fingerprint)) = (cached.get(&key), fingerprint) {
                let reusable = if summary {
                    true
                } else {
                    cached.size == fingerprint.0
                        && cached.mtime_ms == fingerprint.1
                        && cached.info.details_pending != Some(true)
                };
                if reusable {
                    next.insert(key, cached.clone());
                    continue;
                }
            }
            todo.push((key, path.clone()));
        }

        let worker_count = std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(4)
            .min(8)
            .max(1);
        let next_index = std::sync::atomic::AtomicUsize::new(0);
        let parsed = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for _ in 0..worker_count {
                scope.spawn(|| {
                    loop {
                        let index = next_index.fetch_add(1, Ordering::Relaxed);
                        let Some((key, path)) = todo.get(index) else {
                            break;
                        };
                        let fingerprint = fingerprint(&path);
                        let info = if summary {
                            session::read_header_only_info(&path)
                        } else {
                            session::read_session(&path)
                                .ok()
                                .map(|session| session.info())
                        };
                        if let (Some(info), Some((size, mtime_ms))) = (info, fingerprint) {
                            parsed.lock().push((
                                key.clone(),
                                CachedInfo {
                                    size,
                                    mtime_ms,
                                    info,
                                },
                            ));
                        }
                    }
                });
            }
        });
        for (key, entry) in parsed.into_inner() {
            next.insert(key, entry);
            changed = true;
        }

        // Drop entries whose file disappeared.
        let live: HashSet<String> = files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        let before = next.len();
        next.retain(|key, _| live.contains(key));
        if before != next.len() {
            changed = true;
        }

        let mut sessions: Vec<SessionInfo> =
            next.values().map(|entry| entry.info.clone()).collect();
        sessions.sort_by(|a, b| b.modified.cmp(&a.modified));
        *self.entries.lock() = next;
        if changed {
            self.dirty.store(true, Ordering::Relaxed);
        }
        self.maybe_persist();
        sessions
    }

    pub fn invalidate(&self, path: &Path) {
        self.entries.lock().remove(path.to_string_lossy().as_ref());
        self.dirty.store(true, Ordering::Relaxed);
        self.maybe_persist();
    }

    pub fn clear(&self) {
        self.entries.lock().clear();
        self.dirty.store(true, Ordering::Relaxed);
        self.maybe_persist();
    }

    fn maybe_persist(&self) {
        if !self.dirty.load(Ordering::Relaxed) {
            return;
        }
        let mut last = self.last_persist.lock();
        if last.elapsed().unwrap_or(Duration::MAX) < Duration::from_secs(5) {
            return;
        }
        *last = SystemTime::now();
        let file = IndexFile {
            version: 1,
            entries: self.entries.lock().clone(),
        };
        if let Ok(bytes) = serde_json::to_vec(&file) {
            let temp = self.path.with_file_name(format!(
                ".{}-{}.tmp",
                self.path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("index"),
                uuid::Uuid::new_v4()
            ));
            if std::fs::create_dir_all(self.path.parent().unwrap_or(Path::new("."))).is_ok()
                && std::fs::write(&temp, bytes).is_ok()
            {
                let _ = std::fs::rename(&temp, &self.path);
                self.dirty.store(false, Ordering::Relaxed);
            }
        }
    }
}

fn load_index(path: &Path) -> HashMap<String, CachedInfo> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(file) = serde_json::from_str::<IndexFile>(&text) else {
        return HashMap::new();
    };
    if file.version != 1 {
        return HashMap::new();
    }
    file.entries
}

fn session_files(sessions_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(projects) = std::fs::read_dir(sessions_dir) else {
        return files;
    };
    for project in projects.flatten() {
        if !project.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(project.path()) else {
            continue;
        };
        for file in entries.flatten() {
            let path = file.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
                files.push(path);
            }
        }
    }
    files
}

fn fingerprint(path: &Path) -> Option<(u64, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    let mtime = metadata.modified().ok()?;
    let mtime_ms = mtime.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64;
    Some((metadata.len(), mtime_ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rescans_only_changed_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let agent = temp.path().join("agent");
        let config = Config::new(Some(agent.clone()), None).unwrap();
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let dir = config.sessions_dir.join(crate::config::encode_cwd(&cwd));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("2026-01-01T00-00-00.000Z_test.jsonl");
        let header = json!({"type":"session","version":3,"id":"test","timestamp":"2026-01-01T00:00:00.000Z","cwd":cwd.to_string_lossy()});
        let first = json!({"type":"message","id":"u1","parentId":null,"timestamp":"2026-01-01T00:00:00.000Z","message":{"role":"user","content":"first"}});
        std::fs::write(&path, format!("{}\n{}\n", header, first)).unwrap();
        let index = SessionIndex::new(&config);
        let sessions = index.scan(&config, false);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].message_count, 1);
        assert_eq!(sessions[0].first_message, "first");

        let second = json!({"type":"message","id":"a1","parentId":"u1","timestamp":"2026-01-01T00:00:01.000Z","message":{"role":"assistant","content":"second"}});
        std::fs::write(&path, format!("{}\n{}\n{}\n", header, first, second)).unwrap();
        let sessions = index.scan(&config, false);
        assert_eq!(sessions[0].message_count, 2);
        assert!(config.agent_dir.join("pi-star-session-index.json").exists());
    }
}
