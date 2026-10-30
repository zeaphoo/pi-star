//! Open SSE responses and the pages that hold them, for the Debug panel.
//!
//! Only labels and counters live here: a page id (`?holder=`), the request's
//! reported address and a short browser/system label. Nothing decides access.
//! The equivalent upstream modules are `lib/stream-client.ts` and
//! `lib/page-stream-registry.ts`; agent streams also come from
//! `lib/agent-event-stream.ts`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::HeaderMap;
use parking_lot::Mutex;
use serde_json::{Value, json};

/// Longer than any IPv6 text with a zone; a longer entry is not an address.
const MAX_ADDRESS_LENGTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    Agent,
    Terminal,
    FileWatch,
}

impl StreamKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Terminal => "terminal",
            Self::FileWatch => "fileWatch",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamRecord {
    pub id: u64,
    pub kind: StreamKind,
    pub session_id: Option<String>,
    pub holder: Option<String>,
    pub address: Option<String>,
    pub client: Option<String>,
    pub opened_at: i64,
    pub connected_at: Option<i64>,
    pub last_write_at: Option<i64>,
    pub dropped_events: u64,
    pub queued_bytes: u64,
}

impl StreamRecord {
    pub fn to_client_json(&self) -> Value {
        json!({
            "holder": self.holder.clone(),
            "address": self.address.clone(),
            "client": self.client.clone(),
            "openedAt": self.opened_at,
            "connectedAt": self.connected_at,
            "lastWriteAt": self.last_write_at,
            "droppedEvents": self.dropped_events,
            "queuedBytes": self.queued_bytes,
        })
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// Who opened an event stream: the request's reported address and a short
/// browser/system label. A label, never an identity.
pub fn describe_stream_client(headers: &HeaderMap) -> (Option<String>, Option<String>) {
    (
        stream_address(
            headers
                .get("x-forwarded-for")
                .and_then(|value| value.to_str().ok()),
        ),
        stream_client_label(
            headers
                .get("user-agent")
                .and_then(|value| value.to_str().ok()),
        ),
    )
}

/// The first `x-forwarded-for` entry, IPv4-mapped IPv6 shortened
/// (`::ffff:10.0.0.2` becomes `10.0.0.2`).
pub fn stream_address(forwarded_for: Option<&str>) -> Option<String> {
    let first = forwarded_for?.split(',').next()?.trim();
    if first.is_empty()
        || first.len() > MAX_ADDRESS_LENGTH
        || !first
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ".:%[]_-".contains(ch))
    {
        return None;
    }
    if let Some(mapped) = first
        .strip_prefix("::ffff:")
        .or_else(|| first.strip_prefix("::FFFF:"))
    {
        if is_ipv4(mapped) {
            return Some(mapped.to_string());
        }
    }
    Some(first.to_string())
}

fn is_ipv4(value: &str) -> bool {
    let mut parts = 0;
    for part in value.split('.') {
        if part.is_empty() || part.len() > 3 || !part.chars().all(|ch| ch.is_ascii_digit()) {
            return false;
        }
        parts += 1;
    }
    parts == 4
}

/// "Chrome · macOS", "Safari · iOS"; null when the user agent names neither.
pub fn stream_client_label(user_agent: Option<&str>) -> Option<String> {
    let user_agent = user_agent?;
    if user_agent.is_empty() {
        return None;
    }
    // Order matters: Edge and Opera also say Chrome, Chrome also says Safari,
    // iOS browsers say Safari.
    let browser = if contains_any(user_agent, &["Edg/", "EdgA/", "EdgiOS/", "Edge/"]) {
        Some("Edge")
    } else if contains_any(user_agent, &["OPR/", "Opera"]) {
        Some("Opera")
    } else if contains_any(user_agent, &["Firefox/", "FxiOS/"]) {
        Some("Firefox")
    } else if contains_any(user_agent, &["Chrome/", "CriOS/"]) {
        Some("Chrome")
    } else if user_agent.contains("Version/") && user_agent.contains("Safari/") {
        Some("Safari")
    } else {
        None
    };
    // iPhones and iPads before the system says Mac (an iPad asking for desktop
    // sites says Macintosh).
    let system = if contains_any(user_agent, &["iPhone", "iPad", "iPod"]) {
        Some("iOS")
    } else if user_agent.contains("Android") {
        Some("Android")
    } else if user_agent.contains("CrOS") {
        Some("ChromeOS")
    } else if user_agent.contains("Windows") {
        Some("Windows")
    } else if contains_any(user_agent, &["Macintosh", "Mac OS X"]) {
        Some("macOS")
    } else if user_agent.contains("Linux") {
        Some("Linux")
    } else {
        None
    };
    let parts: Vec<&str> = [browser, system].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// A page id: `[A-Za-z0-9_-]{1,64}` (pi-web's `isValidLivenessHolder`).
fn is_valid_holder(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[derive(Default)]
struct RegistryState {
    next_id: u64,
    records: Vec<StreamRecord>,
}

pub struct DebugStreams {
    state: Mutex<RegistryState>,
}

impl Default for DebugStreams {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugStreams {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(RegistryState::default()),
        }
    }

    /// Lists one open stream until the returned handle drops; dropping twice
    /// is impossible (Rust), and a stream that outlives its route stays listed
    /// until the response body is dropped.
    pub fn register(
        self: &Arc<Self>,
        kind: StreamKind,
        session_id: Option<String>,
        holder: Option<String>,
        headers: &HeaderMap,
    ) -> StreamHandle {
        let holder = holder.filter(|value| is_valid_holder(value));
        let (address, client) = describe_stream_client(headers);
        let mut state = self.state.lock();
        state.next_id = state.next_id.saturating_add(1);
        let id = state.next_id;
        state.records.push(StreamRecord {
            id,
            kind,
            session_id,
            holder,
            address,
            client,
            opened_at: now_ms(),
            connected_at: None,
            last_write_at: None,
            dropped_events: 0,
            queued_bytes: 0,
        });
        StreamHandle {
            registry: self.clone(),
            id,
        }
    }

    #[allow(dead_code)]
    pub fn list(&self) -> Vec<StreamRecord> {
        self.state.lock().records.clone()
    }

    pub fn count(&self, kind: StreamKind) -> usize {
        self.state
            .lock()
            .records
            .iter()
            .filter(|record| record.kind == kind)
            .count()
    }

    pub fn session_agent_streams(&self, session_id: &str) -> Vec<StreamRecord> {
        self.state
            .lock()
            .records
            .iter()
            .filter(|record| {
                record.kind == StreamKind::Agent && record.session_id.as_deref() == Some(session_id)
            })
            .cloned()
            .collect()
    }

    pub fn session_ids_with_streams(&self) -> Vec<String> {
        let state = self.state.lock();
        let mut ids: Vec<String> = state
            .records
            .iter()
            .filter(|record| record.kind == StreamKind::Agent)
            .filter_map(|record| record.session_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// Each page with its streams of every kind, labelled by the first one's
    /// request. Pages with only terminal/file-watch streams still appear.
    pub fn pages(&self) -> Vec<Value> {
        let state = self.state.lock();
        let mut pages: BTreeMap<Option<String>, Value> = BTreeMap::new();
        for record in &state.records {
            let entry = pages.entry(record.holder.clone()).or_insert_with(|| {
                json!({
                    "holder": record.holder.clone(),
                    "address": record.address.clone(),
                    "client": record.client.clone(),
                    "agent": 0,
                    "terminal": 0,
                    "fileWatch": 0,
                })
            });
            let key = record.kind.as_str();
            let count = entry.get(key).and_then(Value::as_u64).unwrap_or(0);
            entry[key] = Value::from(count + 1);
        }
        pages.into_values().collect()
    }

    fn mark_connected(&self, id: u64) {
        if let Some(record) = self
            .state
            .lock()
            .records
            .iter_mut()
            .find(|row| row.id == id)
        {
            record.connected_at.get_or_insert_with(now_ms);
        }
    }

    fn mark_write(&self, id: u64) {
        if let Some(record) = self
            .state
            .lock()
            .records
            .iter_mut()
            .find(|row| row.id == id)
        {
            record.last_write_at = Some(now_ms());
            record.connected_at.get_or_insert_with(now_ms);
        }
    }

    fn remove(&self, id: u64) {
        self.state.lock().records.retain(|record| record.id != id);
    }
}

pub struct StreamHandle {
    registry: Arc<DebugStreams>,
    id: u64,
}

impl StreamHandle {
    /// Marks the stream as having sent its `connected` event.
    pub fn connected(&self) {
        self.registry.mark_connected(self.id);
    }

    /// Marks any data written to the stream, heartbeats included.
    pub fn write(&self) {
        self.registry.mark_write(self.id);
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        self.registry.remove(self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    #[test]
    fn addresses_are_shortened_and_validated() {
        assert_eq!(
            stream_address(Some("::ffff:10.0.0.2")).as_deref(),
            Some("10.0.0.2")
        );
        assert_eq!(
            stream_address(Some(" 203.0.113.9, 10.0.0.1")).as_deref(),
            Some("203.0.113.9")
        );
        assert_eq!(
            stream_address(Some("fe80::1%eth0")).as_deref(),
            Some("fe80::1%eth0")
        );
        assert_eq!(stream_address(Some("<script>")), None);
        assert_eq!(
            stream_address(Some(&"1".repeat(MAX_ADDRESS_LENGTH + 1))),
            None
        );
        assert_eq!(stream_address(None), None);
    }

    #[test]
    fn client_labels_read_browser_and_system() {
        assert_eq!(
            stream_client_label(Some("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36")).as_deref(),
            Some("Chrome · macOS")
        );
        assert_eq!(
            stream_client_label(Some("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1")).as_deref(),
            Some("Safari · iOS")
        );
        assert_eq!(
            stream_client_label(Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36 Edg/120.0")).as_deref(),
            Some("Edge · Windows")
        );
        assert_eq!(stream_client_label(Some("curl/8.0")), None);
        assert_eq!(stream_client_label(None), None);
    }

    #[test]
    fn pages_count_every_kind_and_release_with_the_handle() {
        let registry = Arc::new(DebugStreams::new());
        let h = headers(&[
            ("x-forwarded-for", "10.0.0.2"),
            ("user-agent", "Mozilla/5.0 (Windows NT 10.0) Chrome/120.0"),
        ]);
        let agent = registry.register(
            StreamKind::Agent,
            Some("s1".into()),
            Some("page-a".into()),
            &h,
        );
        agent.connected();
        let terminal = registry.register(StreamKind::Terminal, None, Some("page-a".into()), &h);
        let watch = registry.register(StreamKind::FileWatch, None, None, &HeaderMap::new());
        assert_eq!(registry.count(StreamKind::Agent), 1);
        assert_eq!(
            registry.session_agent_streams("s1")[0].address.as_deref(),
            Some("10.0.0.2")
        );
        let pages = registry.pages();
        assert_eq!(pages.len(), 2);
        let page = pages
            .iter()
            .find(|page| page["holder"] == "page-a")
            .expect("the page with a holder is listed");
        assert_eq!(page["agent"], 1);
        assert_eq!(page["terminal"], 1);
        assert_eq!(page["fileWatch"], 0);
        let anonymous = pages
            .iter()
            .find(|page| page["holder"].is_null())
            .expect("a stream without a page id is listed");
        assert_eq!(anonymous["fileWatch"], 1);

        drop(terminal);
        drop(watch);
        drop(agent);
        assert!(registry.list().is_empty());
    }
}
