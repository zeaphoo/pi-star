//! Channel sources: what a channel name subscribes to, and how its events are
//! produced. A source also builds the snapshot a fresh subscriber starts from.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::broadcast;

use pi_star_core::agent::backend::BackendKind;
use pi_star_core::terminal::{TerminalEvent, TerminalManager, TerminalSubscription};

use crate::routes::{agent, auth, blocking};
use crate::state::AppState;

use super::{Hub, SubError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    Agent,
    Terminal,
    File,
    Auth,
}

impl ChannelKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Terminal => "terminal",
            Self::File => "file",
            Self::Auth => "auth",
        }
    }
}

/// Where a subscription may resume from. The meaning depends on the channel:
/// terminal byte offset, agent sequence number or a file version string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cursor {
    Num(u64),
    Text(String),
}

type Snapshot = Vec<(Arc<str>, Cursor)>;
type SourceFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

pub trait Source: Send + Sync {
    fn kind(&self) -> ChannelKind;
    /// The frames a subscriber attaching without a cursor starts from.
    fn snapshot(&self) -> Snapshot;
    fn start(self: Arc<Self>, hub: Arc<Hub>, key: String) -> SourceFuture;
}

pub async fn open(state: &Arc<AppState>, key: &str) -> Result<Arc<dyn Source>, SubError> {
    let (kind, rest) = key.split_once(':').ok_or(SubError::UnknownChannel)?;
    if rest.is_empty() {
        return Err(SubError::UnknownChannel);
    }
    match kind {
        "agent" => agent_source(state, rest).await,
        "terminal" => terminal_source(state, rest),
        "file" => file_source(state, rest),
        "auth" => auth_source(state, rest).await,
        _ => Err(SubError::UnknownChannel),
    }
}

// ── Agent ──

struct AgentSource {
    receiver: Mutex<Option<broadcast::Receiver<Value>>>,
    snapshot: Option<Arc<str>>,
    session_path: std::path::PathBuf,
    config: pi_star_core::Config,
}

/// pi's transcript notices ride on the `message_end` of the message they
/// follow. Neither engine sends them over its wire, so the one funnel both
/// engines share adds them here, from the session file and the `notices.*`
/// switches (a project's `.pi/settings.json` overriding the global ones).
///
/// The entry the message was written to is the boundary: only the entries
/// before it are the "previous request" a cache miss is measured against.
async fn live_transcript_notices(
    config: pi_star_core::Config,
    path: std::path::PathBuf,
    message: &Value,
) -> Vec<Value> {
    const ATTEMPTS: u32 = 8;
    /// The switches for this session's folder, and the entries before the one
    /// the message was written to. `None` while that entry is not in the file
    /// yet.
    struct Read {
        notices: pi_star_core::settings::Notices,
        entries: Option<Vec<Value>>,
    }
    let read_once = |read_path: std::path::PathBuf, expected: Value, with_entries: bool| {
        let config = config.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                // The folder the switches are resolved against lives in the
                // header, one line; the entries are read only when some switch
                // is on, which keeps the default (all off) to one line per
                // message.
                let header = pi_star_core::session::read_header(&read_path).ok()?;
                let notices = pi_star_core::settings::effective_notices(
                    &config,
                    std::path::Path::new(&header.cwd),
                );
                if !notices.any() {
                    return Some(Read { notices, entries: None });
                }
                if !with_entries {
                    return Some(Read { notices, entries: None });
                }
                let session = pi_star_core::session::read_session(&read_path).ok()?;
                let entries = session
                    .entries
                    .iter()
                    .rposition(|entry| entry.get("message") == Some(&expected))
                    .map(|index| session.entries[..index].to_vec());
                Some(Read { notices, entries })
            })
            .await
            .ok()
            .flatten()
        }
    };

    // Gate on the switches first: pi's default is all off, and reading the
    // whole file for nothing is the expensive part.
    let enabled = match read_once(path.clone(), Value::Null, false).await {
        Some(read) => read.notices,
        None => return Vec::new(),
    };
    if !enabled.any() {
        return Vec::new();
    }

    // The response is written to the file around the event; wait for it as
    // `announce_user_message_entry` waits for a user message's entry id.
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let Some(read) = read_once(path.clone(), message.clone(), true).await else {
            return Vec::new();
        };
        if !read.notices.any() {
            return Vec::new();
        }
        if let Some(entries) = read.entries {
            return pi_star_core::transcript_notices::live_assistant_notices(
                &entries,
                message,
                read.notices,
            );
        }
    }
    Vec::new()
}

fn connected(streaming: bool, pending: Vec<String>) -> Arc<str> {
    json!({
        "type": "connected",
        "isStreaming": streaming,
        "pendingExtensionUiIds": pending,
    })
    .to_string()
    .into()
}

async fn agent_source(state: &Arc<AppState>, id: &str) -> Result<Arc<dyn Source>, SubError> {
    if state.route_backend_async(id).await == BackendKind::Rpc {
        let session = state
            .pi_rpc
            .open_or_create(id)
            .await
            .map_err(|_| SubError::Unavailable("Session not found".into()))?;
        let streaming = session
            .state_json()
            .get("isStreaming")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(Arc::new(AgentSource {
            receiver: Mutex::new(Some(session.subscribe())),
            snapshot: Some(connected(streaming, session.pending_extension_ui_ids())),
            session_path: session.path(),
            config: state.config.clone(),
        }))
    } else {
        let runtime = match state.agents.get(id) {
            Some(runtime) => Ok(runtime),
            None => {
                let state_for = state.clone();
                let open_id = id.to_string();
                blocking(move || state_for.agents.open_or_create(&open_id))
                    .await
                    .map_err(|_| SubError::Unavailable("Session not found".into()))?
            }
        }
        .map_err(|_| SubError::Unavailable("Session not found".into()))?;
        let receiver = runtime
            .subscribe()
            .await
            .map_err(|error| SubError::Unavailable(error.to_string()))?;
        let streaming = runtime
            .state_json(false)
            .get("isStreaming")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(Arc::new(AgentSource {
            receiver: Mutex::new(Some(receiver)),
            snapshot: Some(connected(streaming, runtime.pending_extension_ui_ids())),
            session_path: runtime.path.clone(),
            config: state.config.clone(),
        }))
    }
}

impl Source for AgentSource {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Agent
    }

    fn snapshot(&self) -> Snapshot {
        self.snapshot
            .clone()
            .map(|payload| vec![(payload, Cursor::Num(0))])
            .unwrap_or_default()
    }

    fn start(self: Arc<Self>, hub: Arc<Hub>, key: String) -> SourceFuture {
        Box::pin(async move {
            let Some(mut receiver) = self.receiver.lock().take() else {
                return;
            };
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        let user_message = (event.get("type").and_then(Value::as_str)
                            == Some("message_end"))
                        .then(|| event.get("message").cloned())
                        .flatten()
                        .filter(|message| {
                            message.get("role").and_then(Value::as_str) == Some("user")
                        });
                        if let Some(message) = user_message {
                            let path = self.session_path.clone();
                            let hub_for_entry = hub.clone();
                            let key_for_entry = key.clone();
                            tokio::spawn(async move {
                                announce_user_message_entry(
                                    hub_for_entry,
                                    key_for_entry,
                                    path,
                                    message,
                                )
                                .await;
                            });
                        }
                        // pi's notices for an assistant response: added before
                        // the event is filtered so the chat shows them on the
                        // message they follow, as the TUI does.
                        let mut event = event;
                        let assistant_message = (event.get("type").and_then(Value::as_str)
                            == Some("message_end"))
                        .then(|| event.get("message").cloned())
                        .flatten()
                        .filter(|message| {
                            message.get("role").and_then(Value::as_str) == Some("assistant")
                        });
                        if let Some(message) = assistant_message
                            && let Some(object) = event.as_object_mut()
                        {
                            let notices = live_transcript_notices(
                                self.config.clone(),
                                self.session_path.clone(),
                                &message,
                            )
                            .await;
                            if !notices.is_empty() {
                                object.insert("transcriptNotices".into(), Value::Array(notices));
                            }
                        }
                        if let Some(event) = agent::filter_event(event) {
                            hub.publish(&key, event.to_string().into(), Cursor::Num(0));
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // The subscriber fell behind the runtime's buffer; the
                        // client reloads the session snapshot on `resync`.
                        hub.publish(
                            &key,
                            json!({"type": "resync", "reason": "lagged"})
                                .to_string()
                                .into(),
                            Cursor::Num(0),
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }
}

/// pi appends a user message only after emitting its `message_end` and announces
/// the entry id to no one. The chat's New session on that message needs it
/// before the run's end reads the file, so read the file just written and send
/// the id back. Retries cover the short gap between the event and the write.
async fn announce_user_message_entry(
    hub: Arc<Hub>,
    key: String,
    path: std::path::PathBuf,
    message: Value,
) {
    const ATTEMPTS: u32 = 8;
    for _ in 0..ATTEMPTS {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let Ok(session) = pi_star_core::session::read_session(&path) else {
            continue;
        };
        let Some(entry) = session.entries.iter().rev().find(|entry| {
            pi_star_core::session::entry_type(entry) == Some("message")
                && entry
                    .get("message")
                    .is_some_and(|candidate| same_user_message(candidate, &message))
        }) else {
            continue;
        };
        let Some(entry_id) = pi_star_core::session::entry_id(entry) else {
            continue;
        };
        hub.publish(
            &key,
            json!({
                "type": "user_message_entry",
                "entryId": entry_id,
                "timestamp": message.get("timestamp").cloned().unwrap_or(Value::Null),
                "message": message,
            })
            .to_string()
            .into(),
            Cursor::Num(0),
        );
        return;
    }
}

fn same_user_message(candidate: &Value, expected: &Value) -> bool {
    if candidate.get("role").and_then(Value::as_str) != Some("user")
        || expected.get("role").and_then(Value::as_str) != Some("user")
    {
        return false;
    }
    if candidate.get("timestamp").is_some()
        && expected.get("timestamp").is_some()
        && candidate.get("timestamp") == expected.get("timestamp")
    {
        return true;
    }
    candidate.get("content") == expected.get("content")
}

// ── Terminal ──

struct TerminalSource {
    manager: Arc<TerminalManager>,
    id: String,
}

fn terminal_frames(subscription: &TerminalSubscription) -> Snapshot {
    let mut frames = Vec::with_capacity(3);
    let TerminalEvent::Output { data, offset, .. } = &subscription.output else {
        return frames;
    };
    let offset = *offset as u64;
    if let Some(cwd) = &subscription.cwd {
        frames.push((
            json!({"type": "cwd", "cwd": cwd}).to_string().into(),
            Cursor::Num(offset),
        ));
    }
    frames.push((
        json!({"type": "output", "data": data, "offset": offset, "reset": true})
            .to_string()
            .into(),
        Cursor::Num(offset),
    ));
    if subscription.exited {
        frames.push((
            json!({"type": "exit", "exitCode": subscription.exit_code.unwrap_or(0)})
                .to_string()
                .into(),
            Cursor::Num(offset),
        ));
    }
    frames
}

fn terminal_source(state: &Arc<AppState>, id: &str) -> Result<Arc<dyn Source>, SubError> {
    if state.terminals.cwd(id).is_none() {
        return Err(SubError::UnknownChannel);
    }
    Ok(Arc::new(TerminalSource {
        manager: state.terminals.clone(),
        id: id.to_string(),
    }))
}

impl Source for TerminalSource {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Terminal
    }

    fn snapshot(&self) -> Snapshot {
        self.manager
            .subscribe(&self.id)
            .map(|subscription| terminal_frames(&subscription))
            .unwrap_or_default()
    }

    fn start(self: Arc<Self>, hub: Arc<Hub>, key: String) -> SourceFuture {
        Box::pin(async move {
            let Some(mut subscription) = self.manager.subscribe(&self.id) else {
                return;
            };
            let mut offset = match &subscription.output {
                TerminalEvent::Output { offset, .. } => *offset as u64,
                _ => 0,
            };
            loop {
                match subscription.receiver.recv().await {
                    Ok(event) => match event {
                        TerminalEvent::Output {
                            data, offset: at, ..
                        } => {
                            offset = at as u64;
                            hub.publish(
                                &key,
                                json!({"type": "output", "data": data, "offset": at})
                                    .to_string()
                                    .into(),
                                Cursor::Num(at as u64),
                            );
                        }
                        TerminalEvent::Cwd { cwd } => {
                            hub.publish(
                                &key,
                                json!({"type": "cwd", "cwd": cwd}).to_string().into(),
                                Cursor::Num(offset),
                            );
                        }
                        TerminalEvent::Exit { exit_code } => {
                            hub.publish(
                                &key,
                                json!({"type": "exit", "exitCode": exit_code})
                                    .to_string()
                                    .into(),
                                Cursor::Num(offset),
                            );
                            break;
                        }
                        TerminalEvent::Closed => break,
                    },
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // Bytes were missed: re-subscribe and publish a fresh
                        // reset baseline; clients clear and redraw from it.
                        let Some(fresh) = self.manager.subscribe(&self.id) else {
                            break;
                        };
                        subscription = fresh;
                        for (payload, cursor) in terminal_frames(&subscription) {
                            if let Cursor::Num(at) = cursor {
                                offset = at;
                            }
                            hub.publish(&key, payload, cursor);
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }
}

// ── File watch ──

struct FileSource {
    path: PathBuf,
}

fn version(path: &Path) -> Option<(u64, String)> {
    pi_star_core::files::modified_version(path)
}

fn version_cursor(version: &Option<(u64, String)>) -> Cursor {
    match version {
        Some((size, mtime)) => Cursor::Text(format!("{size}:{mtime}")),
        None => Cursor::Text("missing".into()),
    }
}

fn file_source(state: &Arc<AppState>, path: &str) -> Result<Arc<dyn Source>, SubError> {
    let path = PathBuf::from(path);
    if !path.is_absolute() || !state.roots.is_allowed(&path) {
        return Err(SubError::Forbidden);
    }
    let authorization = if path.exists() {
        path.clone()
    } else {
        path.parent().unwrap_or(&path).to_path_buf()
    };
    if !state.roots.is_allowed_existing(&authorization) {
        return Err(SubError::Forbidden);
    }
    Ok(Arc::new(FileSource { path }))
}

impl Source for FileSource {
    fn kind(&self) -> ChannelKind {
        ChannelKind::File
    }

    fn snapshot(&self) -> Snapshot {
        let version = version(&self.path);
        vec![(
            json!({
                "type": "connected",
                "filePath": self.path.to_string_lossy(),
            })
            .to_string()
            .into(),
            version_cursor(&version),
        )]
    }

    fn start(self: Arc<Self>, hub: Arc<Hub>, key: String) -> SourceFuture {
        Box::pin(async move {
            let mut last = version(&self.path);
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            tick.tick().await;
            loop {
                tick.tick().await;
                let path = self.path.clone();
                let next = match tokio::task::spawn_blocking(move || version(&path)).await {
                    Ok(version) => version,
                    Err(_) => break,
                };
                if next != last {
                    last = next.clone();
                    let payload = match &next {
                        Some((size, mtime)) => {
                            json!({"type": "change", "size": size, "mtime": mtime})
                        }
                        None => json!({"type": "change", "size": 0, "mtime": Value::Null}),
                    };
                    hub.publish(&key, payload.to_string().into(), version_cursor(&next));
                }
            }
        })
    }
}

// ── Auth ──

struct AuthSource {
    flow: Mutex<Option<auth::Flow>>,
}

async fn auth_source(state: &Arc<AppState>, provider: &str) -> Result<Arc<dyn Source>, SubError> {
    let flow = auth::start_flow(state.clone(), provider.to_string())
        .await
        .map_err(|(_, error)| SubError::Unavailable(error))?;
    Ok(Arc::new(AuthSource {
        flow: Mutex::new(Some(flow)),
    }))
}

impl Source for AuthSource {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Auth
    }

    fn snapshot(&self) -> Snapshot {
        Vec::new()
    }

    fn start(self: Arc<Self>, hub: Arc<Hub>, key: String) -> SourceFuture {
        Box::pin(async move {
            let Some(mut flow) = self.flow.lock().take() else {
                return;
            };
            while let Some(event) = flow.rx.recv().await {
                hub.publish(&key, event.to_string().into(), Cursor::Num(0));
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_kinds_have_stable_names() {
        assert_eq!(ChannelKind::Agent.as_str(), "agent");
        assert_eq!(ChannelKind::Terminal.as_str(), "terminal");
        assert_eq!(ChannelKind::File.as_str(), "file");
        assert_eq!(ChannelKind::Auth.as_str(), "auth");
    }

    #[test]
    fn matches_the_user_message_the_file_holds() {
        let expected = json!({"role": "user", "content": "hi", "timestamp": 12});
        assert!(same_user_message(
            &json!({"role": "user", "content": "hi", "timestamp": 12}),
            &expected
        ));
        assert!(same_user_message(
            &json!({"role": "user", "content": "hi"}),
            &json!({"role": "user", "content": "hi", "timestamp": 13})
        ));
        assert!(!same_user_message(
            &json!({"role": "assistant", "content": "hi", "timestamp": 12}),
            &expected
        ));
        // The persisted message carries the same timestamp; the file may add
        // fields to content, so the timestamp is the primary identity.
        assert!(same_user_message(
            &json!({"role": "user", "content": "other", "timestamp": 12}),
            &expected
        ));
    }

    #[test]
    fn missing_file_has_a_stable_cursor() {
        assert_eq!(version_cursor(&None), Cursor::Text("missing".into()));
        assert_eq!(
            version_cursor(&Some((12, "t".into()))),
            Cursor::Text("12:t".into())
        );
    }
}
