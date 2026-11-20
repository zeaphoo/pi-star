//! One multiplexed WebSocket carries every live stream (agent events, terminal
//! output, file watches, OAuth logins) between a browser and pi-star.
//!
//! The browser allows six HTTP/1.1 connections per origin, shared by every tab,
//! so per-feature SSE turns the number of open streams into the number of
//! widget instances. A WebSocket leaves that pool after the upgrade, and the
//! SharedWorker on the client side keeps one socket per browser profile. This
//! module is the server half: a registry of channels, each with a bounded ring
//! of recent events, replay by cursor, per-channel drop policy and a whole-hub
//! memory budget.
//!
//! Frames are JSON text; see [`ws`] for the operations a client may send.

pub mod sources;
pub mod ws;

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{Value, json};

use sources::{ChannelKind, Cursor, Source};

/// Client frames larger than this are rejected; terminal input is capped far
/// lower (64 KiB) before it reaches the PTY.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
/// Terminal input frames: a paste larger than this is split by the client.
pub const MAX_TERMINAL_INPUT_BYTES: usize = 64 * 1024;
/// Channels one connection may subscribe to.
pub const MAX_CHANNELS_PER_CONN: usize = 64;
/// Concurrent connections one page (`?holder=`) may open.
pub const MAX_CONNECTIONS_PER_HOLDER: usize = 8;
/// How many frames a channel keeps for one subscriber. The bound is per
/// channel, so a slow terminal does not count against agent events.
const SUB_QUEUE_FRAMES: usize = 256;

/// Server → client frame, already serialized (or a close instruction).
pub enum Outbound {
    Text(String),
    Close(u16, &'static str),
}

/// Per-connection stop signal: a slow consumer or shutdown wakes the writer.
#[derive(Default)]
pub struct Shutdown {
    flag: AtomicBool,
    notify: tokio::sync::Notify,
}

impl Shutdown {
    pub fn close(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_closed(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
pub struct ConnHandle {
    pub id: u64,
    pub tx: tokio::sync::mpsc::Sender<Outbound>,
    pub shutdown: Arc<Shutdown>,
}

impl ConnHandle {
    /// Queue a frame; false means the queue is full or the connection is gone.
    pub fn try_send(&self, text: String) -> bool {
        if self.shutdown.is_closed() {
            return false;
        }
        self.tx.try_send(Outbound::Text(text)).is_ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resume {
    /// Terminal channels resume by byte offset.
    Offset,
    /// Agent channels resume by sequence number.
    Seq,
    /// File channels resume by `(size, mtime)` version.
    Version,
    /// Auth channels replay everything.
    All,
}

#[derive(Debug)]
pub enum SubError {
    UnknownChannel,
    Forbidden,
    Unavailable(String),
    AlreadySubscribed,
}

impl SubError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownChannel => "unknown_channel",
            Self::Forbidden => "forbidden",
            Self::Unavailable(_) => "unavailable",
            Self::AlreadySubscribed => "already_subscribed",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::UnknownChannel => "No such channel".into(),
            Self::Forbidden => "Access denied".into(),
            Self::Unavailable(message) => message.clone(),
            Self::AlreadySubscribed => "This connection already subscribes to it".into(),
        }
    }
}

/// Whether `subscribe` attached or only reported a gap. A gap means the client
/// must resubscribe without a cursor; nothing was registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attach {
    Subscribed,
    Gap,
}

/// Bounds from the environment. Defaults follow the stream-bus design: a
/// terminal may grow to 20 MiB of replay history before it drops frames.
#[derive(Debug, Clone)]
pub struct HubConfig {
    pub budget_bytes: usize,
    pub terminal_max: usize,
    pub agent_max: usize,
    pub file_max: usize,
    pub auth_max: usize,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            budget_bytes: env_bytes("PI_STAR_STREAM_RING_BUDGET_BYTES", 256 * 1024 * 1024),
            terminal_max: env_bytes("PI_STAR_STREAM_RING_TERMINAL_MAX_BYTES", 20 * 1024 * 1024),
            agent_max: env_bytes("PI_STAR_STREAM_RING_AGENT_MAX_BYTES", 8 * 1024 * 1024),
            file_max: env_bytes("PI_STAR_STREAM_RING_FILE_MAX_BYTES", 1024 * 1024),
            auth_max: env_bytes("PI_STAR_STREAM_RING_AUTH_MAX_BYTES", 256 * 1024),
        }
    }
}

fn env_bytes(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value >= 16 * 1024)
        .unwrap_or(default)
}

impl HubConfig {
    fn limits(&self, kind: ChannelKind) -> (usize, usize) {
        match kind {
            ChannelKind::Terminal => (1024 * 1024, self.terminal_max),
            ChannelKind::Agent => (512 * 1024, self.agent_max),
            ChannelKind::File => (64 * 1024, self.file_max),
            ChannelKind::Auth => (64 * 1024, self.auth_max),
        }
    }
}

// ── Ring ──

#[derive(Debug, Clone)]
struct Stored {
    seq: u64,
    cursor: Cursor,
    payload: Arc<str>,
    /// The snapshot frame a cursor replays from; trimming never removes it.
    pinned: bool,
}

struct Ring {
    events: VecDeque<Stored>,
    bytes: usize,
    capacity: usize,
    initial: usize,
    max: usize,
    evicted: u64,
    low_since: Option<Instant>,
}

impl Ring {
    fn new(initial: usize, max: usize) -> Self {
        Self {
            events: VecDeque::new(),
            bytes: 0,
            capacity: initial,
            initial,
            max,
            evicted: 0,
            low_since: None,
        }
    }

    fn clear(&mut self) -> usize {
        let bytes = self.bytes;
        self.events.clear();
        self.bytes = 0;
        self.low_since = None;
        bytes
    }

    /// Appends one frame, growing on the high-water mark and evicting the
    /// oldest unpinned frames back down to the capacity. Returns the net byte
    /// delta (negative when frames were evicted).
    fn push(&mut self, seq: u64, cursor: Cursor, payload: Arc<str>, pinned: bool) -> isize {
        let added = payload.len() + 16;
        self.bytes += added;
        self.events.push_back(Stored {
            seq,
            cursor,
            payload,
            pinned,
        });
        if self.bytes > self.capacity * 8 / 10 && self.capacity < self.max {
            self.capacity = (self.capacity * 2).min(self.max);
        }
        let mut removed = 0usize;
        while self.bytes > self.capacity {
            let Some(index) = self.events.iter().position(|event| !event.pinned) else {
                break;
            };
            if self.events.len() <= 1 {
                break;
            }
            if let Some(event) = self.events.remove(index) {
                removed += event.payload.len() + 16;
                self.evicted += 1;
            }
        }
        self.bytes = self.bytes.saturating_sub(removed);
        added as isize - removed as isize
    }

    /// Drops the oldest unpinned frames until `target` bytes remain.
    fn trim_to(&mut self, target: usize) -> usize {
        let mut removed = 0usize;
        while self.bytes > target {
            let Some(index) = self.events.iter().position(|event| !event.pinned) else {
                break;
            };
            if self.events.len() <= 1 {
                break;
            }
            if let Some(event) = self.events.remove(index) {
                removed += event.payload.len() + 16;
                self.bytes = self.bytes.saturating_sub(event.payload.len() + 16);
                self.evicted += 1;
            }
        }
        removed
    }

    /// Halves an idle buffer back towards its initial capacity.
    fn shrink(&mut self, now: Instant) -> bool {
        if self.capacity <= self.initial {
            self.low_since = None;
            return false;
        }
        if self.bytes * 4 > self.capacity {
            self.low_since = None;
            return false;
        }
        let since = *self.low_since.get_or_insert(now);
        if now.duration_since(since) < Duration::from_secs(60) {
            return false;
        }
        self.capacity = (self.capacity / 2).max(self.initial);
        self.low_since = None;
        true
    }
}

// ── Channel ──

struct SubEntry {
    tx: tokio::sync::mpsc::Sender<(u64, String)>,
    /// Frames this subscriber could not accept, awaiting a gap notice.
    pending_gap: Option<(u64, u64)>,
}

struct ChannelState {
    seq: u64,
    ring: Ring,
    subs: HashMap<u64, SubEntry>,
    connected_at: i64,
}

pub struct Channel {
    key: String,
    source: Arc<dyn Source>,
    kind: ChannelKind,
    resume: Resume,
    total: Arc<AtomicUsize>,
    state: Mutex<ChannelState>,
    producer: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Channel {
    fn new(
        key: String,
        source: Arc<dyn Source>,
        cfg: HubConfig,
        total: Arc<AtomicUsize>,
    ) -> Arc<Self> {
        let kind = source.kind();
        let (initial, max) = cfg.limits(kind);
        let resume = match kind {
            ChannelKind::Terminal => Resume::Offset,
            ChannelKind::Agent => Resume::Seq,
            ChannelKind::File => Resume::Version,
            ChannelKind::Auth => Resume::All,
        };
        Arc::new(Self {
            key,
            source,
            kind,
            resume,
            total,
            state: Mutex::new(ChannelState {
                seq: 0,
                ring: Ring::new(initial, max),
                subs: HashMap::new(),
                connected_at: crate::debug_streams::now_ms(),
            }),
            producer: Mutex::new(None),
        })
    }

    fn start(self: &Arc<Self>, hub: Arc<Hub>) {
        let source = self.source.clone();
        let key = self.key.clone();
        let task = tokio::spawn(async move {
            source.start(hub, key).await;
        });
        *self.producer.lock() = Some(task);
    }

    fn frame_ev(&self, seq: u64, payload: &str) -> String {
        let key = serde_json::to_string(&self.key).unwrap_or_else(|_| "\"\"".into());
        format!("{{\"op\":\"ev\",\"channel\":{key},\"seq\":{seq},\"data\":{payload}}}")
    }

    fn frame_gap(&self, from: u64, to: u64, live: bool) -> String {
        let key = serde_json::to_string(&self.key).unwrap_or_else(|_| "\"\"".into());
        format!(
            "{{\"op\":\"gap\",\"channel\":{key},\"from\":{from},\"to\":{to},\"dropped\":{live}}}"
        )
    }

    fn position(event: &Stored, resume: Resume) -> u64 {
        match resume {
            Resume::Offset => match event.cursor {
                Cursor::Num(value) => value,
                Cursor::Text(_) => event.seq,
            },
            _ => event.seq,
        }
    }

    /// Appends one frame; `deliver` false only while a snapshot is being
    /// installed, where the attaching subscriber gets the frames directly.
    fn push_locked(
        &self,
        st: &mut ChannelState,
        payload: Arc<str>,
        cursor: Cursor,
        pinned: bool,
        deliver: bool,
    ) -> u64 {
        st.seq += 1;
        let seq = st.seq;
        let delta = st.ring.push(seq, cursor, payload.clone(), pinned);
        if delta >= 0 {
            self.total.fetch_add(delta as usize, Ordering::Relaxed);
        } else {
            self.total.fetch_sub((-delta) as usize, Ordering::Relaxed);
        }
        if deliver {
            let frame = self.frame_ev(seq, &payload);
            for sub in st.subs.values_mut() {
                self.send_to_sub(sub, &frame, seq);
            }
        }
        seq
    }

    /// Sends one live frame to one subscriber, applying the channel's drop
    /// policy. Terminal and file frames may be skipped and marked with a gap;
    /// agent frames never are: a stuck consumer is disconnected instead.
    fn send_to_sub(&self, sub: &mut SubEntry, frame: &str, seq: u64) {
        if let Some((from, to)) = sub.pending_gap.take() {
            let gap = self.frame_gap(from, to, true);
            if sub.tx.try_send((seq, gap)).is_err() {
                sub.pending_gap = Some((from, to));
                return;
            }
        }
        if sub.tx.try_send((seq, frame.to_string())).is_err() {
            match self.kind {
                ChannelKind::Terminal | ChannelKind::File => {
                    sub.pending_gap = Some((seq, seq));
                }
                ChannelKind::Agent | ChannelKind::Auth => {
                    // The connection task sees this and closes with 1013.
                    sub.pending_gap = None;
                    let _ = sub.tx.try_send((seq, String::new()));
                }
            }
        }
    }

    /// Replays from a cursor. `Ok(frames)` when the ring covers the cursor,
    /// `Err(bounds)` when it does not (the client must resubscribe without one).
    fn replay_locked(&self, st: &ChannelState, cursor: &Cursor) -> Result<Vec<String>, (u64, u64)> {
        let first_pos = st
            .ring
            .events
            .front()
            .map(|event| Self::position(event, self.resume));
        let last_pos = st
            .ring
            .events
            .back()
            .map(|event| Self::position(event, self.resume))
            .unwrap_or(st.seq);
        let client = match (self.resume, cursor) {
            (Resume::Offset | Resume::Seq, Cursor::Num(value)) => *value,
            (Resume::All, _) => 0,
            _ => return Ok(vec![]),
        };
        if let Some(first) = first_pos {
            if client < first {
                return Err((first, last_pos));
            }
        }
        Ok(st
            .ring
            .events
            .iter()
            .filter(|event| Self::position(event, self.resume) > client)
            .map(|event| self.frame_ev(event.seq, &event.payload))
            .collect())
    }

    /// Installs a fresh snapshot as the new baseline. Any event published after
    /// this call has a position beyond it, so replay stays coherent.
    fn resync_locked(&self, st: &mut ChannelState) -> Vec<String> {
        let removed = st.ring.clear();
        self.total.fetch_sub(removed, Ordering::Relaxed);
        let mut frames = Vec::new();
        for (payload, cursor) in self.source.snapshot() {
            let seq = self.push_locked(st, payload.clone(), cursor, true, false);
            frames.push(self.frame_ev(seq, &payload));
        }
        frames
    }

    fn remove_sub(&self, conn_id: u64) -> bool {
        let mut st = self.state.lock();
        st.subs.remove(&conn_id);
        st.subs.is_empty()
    }

    fn stats(&self) -> Value {
        let st = self.state.lock();
        json!({
            "channel": self.key,
            "kind": self.kind.as_str(),
            "resume": match self.resume {
                Resume::Offset => "offset",
                Resume::Seq => "seq",
                Resume::Version => "version",
                Resume::All => "all",
            },
            "seq": st.seq,
            "subscribers": st.subs.len(),
            "ringBytes": st.ring.bytes,
            "ringCapacity": st.ring.capacity,
            "ringEvents": st.ring.events.len(),
            "evicted": st.ring.evicted,
            "connectedAt": st.connected_at,
        })
    }
}

// ── Hub ──

struct ConnInfo {
    holder: Option<String>,
    channels: HashSet<String>,
}

#[derive(Clone, Copy)]
enum Target {
    Half,
    Empty,
}

pub struct Hub {
    channels: Mutex<HashMap<String, Arc<Channel>>>,
    connections: Mutex<HashMap<u64, ConnInfo>>,
    holders: Mutex<HashMap<String, usize>>,
    next_conn: AtomicU64,
    total: Arc<AtomicUsize>,
    cfg: HubConfig,
    /// Serializes first-time channel creation, which awaits a source.
    creating: tokio::sync::Mutex<()>,
    dropped: AtomicU64,
    gaps: AtomicU64,
    connected: AtomicUsize,
    /// One stop signal per connection; `shutdown()` closes them all.
    conn_shutdowns: Mutex<HashMap<u64, Arc<Shutdown>>>,
    started: Instant,
}

impl Hub {
    pub fn new(cfg: HubConfig) -> Arc<Self> {
        Arc::new(Self {
            channels: Mutex::new(HashMap::new()),
            connections: Mutex::new(HashMap::new()),
            holders: Mutex::new(HashMap::new()),
            next_conn: AtomicU64::new(1),
            total: Arc::new(AtomicUsize::new(0)),
            cfg,
            creating: tokio::sync::Mutex::new(()),
            dropped: AtomicU64::new(0),
            gaps: AtomicU64::new(0),
            connected: AtomicUsize::new(0),
            conn_shutdowns: Mutex::new(HashMap::new()),
            started: Instant::now(),
        })
    }

    pub fn start_maintenance(self: &Arc<Self>) {
        let hub = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(60));
            tick.tick().await;
            loop {
                tick.tick().await;
                hub.shrink_idle();
            }
        });
    }

    pub fn register_conn(
        self: &Arc<Self>,
        holder: Option<String>,
        tx: tokio::sync::mpsc::Sender<Outbound>,
    ) -> Result<ConnHandle, ()> {
        if let Some(holder) = &holder {
            let mut holders = self.holders.lock();
            let count = holders.entry(holder.clone()).or_insert(0);
            if *count >= MAX_CONNECTIONS_PER_HOLDER {
                return Err(());
            }
            *count += 1;
        }
        let id = self.next_conn.fetch_add(1, Ordering::Relaxed);
        self.connections.lock().insert(
            id,
            ConnInfo {
                holder,
                channels: HashSet::new(),
            },
        );
        self.connected.fetch_add(1, Ordering::Relaxed);
        let shutdown = Arc::new(Shutdown::default());
        self.conn_shutdowns.lock().insert(id, shutdown.clone());
        Ok(ConnHandle { id, tx, shutdown })
    }

    /// Records that `key` belongs to `conn`, so connection cleanup can detach.
    pub fn note_channel(&self, conn_id: u64, key: &str) {
        if let Some(info) = self.connections.lock().get_mut(&conn_id) {
            info.channels.insert(key.to_string());
        }
    }

    pub fn forget_channel(&self, conn_id: u64, key: &str) {
        if let Some(info) = self.connections.lock().get_mut(&conn_id) {
            info.channels.remove(key);
        }
    }

    pub fn drop_conn(&self, conn: &ConnHandle) {
        let info = self.connections.lock().remove(&conn.id);
        self.conn_shutdowns.lock().remove(&conn.id);
        self.connected.fetch_sub(1, Ordering::Relaxed);
        if let Some(info) = info {
            if let Some(holder) = &info.holder {
                let mut holders = self.holders.lock();
                if let Some(count) = holders.get_mut(holder) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        holders.remove(holder);
                    }
                }
            }
            for key in info.channels {
                self.detach(&key, conn.id);
            }
        }
    }

    fn detach(&self, key: &str, conn_id: u64) {
        let empty = {
            let channels = self.channels.lock();
            match channels.get(key) {
                Some(channel) => channel.remove_sub(conn_id),
                None => false,
            }
        };
        if empty {
            self.retire(key);
        }
    }

    fn retire(&self, key: &str) {
        // Re-check emptiness under both locks: a subscriber may have attached
        // between `remove_sub` and here.
        let removed = {
            let mut channels = self.channels.lock();
            let empty = channels
                .get(key)
                .map(|channel| channel.state.lock().subs.is_empty())
                .unwrap_or(false);
            if empty { channels.remove(key) } else { None }
        };
        if let Some(channel) = removed {
            if let Some(task) = channel.producer.lock().take() {
                task.abort();
            }
            let bytes = {
                let mut st = channel.state.lock();
                st.subs.clear();
                st.ring.clear()
            };
            self.total.fetch_sub(bytes, Ordering::Relaxed);
        }
    }

    /// Opens (or reuses) the channel and attaches one connection to it.
    pub async fn subscribe(
        self: &Arc<Self>,
        state: &Arc<crate::state::AppState>,
        conn: &ConnHandle,
        key: &str,
        cursor: Option<Value>,
    ) -> Result<Attach, SubError> {
        let existing = self.channels.lock().get(key).cloned();
        if let Some(channel) = existing {
            return self.attach(&channel, conn, cursor);
        }
        let _guard = self.creating.lock().await;
        let existing = self.channels.lock().get(key).cloned();
        if let Some(channel) = existing {
            return self.attach(&channel, conn, cursor);
        }
        let source = sources::open(state, key).await?;
        let channel = Channel::new(
            key.to_string(),
            source,
            self.cfg.clone(),
            self.total.clone(),
        );
        {
            self.channels
                .lock()
                .insert(key.to_string(), channel.clone());
            channel.start(self.clone());
        }
        self.attach(&channel, conn, cursor)
    }

    fn attach(
        &self,
        channel: &Arc<Channel>,
        conn: &ConnHandle,
        cursor: Option<Value>,
    ) -> Result<Attach, SubError> {
        let mut st = channel.state.lock();
        if st.subs.contains_key(&conn.id) {
            return Err(SubError::AlreadySubscribed);
        }
        let parsed = cursor.as_ref().and_then(parse_cursor);
        let mut out: Vec<String> = Vec::new();
        let mut gap = None;
        let mut resync = false;
        // A fresh channel has no history to replay from, so a client cursor is
        // meaningless there: it would silently replay nothing (a remounted
        // terminal would stay blank). Every first subscriber starts from the
        // source snapshot instead; cursors only resume an existing channel.
        let fresh = st.ring.events.is_empty();
        match (channel.resume, &parsed) {
            _ if fresh => resync = true,
            (_, None) => resync = true,
            (Resume::Version, Some(cursor)) => {
                let current = channel.source.snapshot().into_iter().next();
                if current.as_ref().map(|(_, cursor)| cursor) != Some(cursor) {
                    resync = true;
                }
            }
            (_, Some(cursor)) => match channel.replay_locked(&st, cursor) {
                Ok(frames) => out = frames,
                Err(bounds) => gap = Some(bounds),
            },
        }
        if let Some((from, to)) = gap {
            drop(st);
            self.gaps.fetch_add(1, Ordering::Relaxed);
            let _ = conn.try_send(
                json!({
                    "op": "gap",
                    "channel": channel.key,
                    "from": from,
                    "to": to,
                    "reason": "cursor_too_old",
                })
                .to_string(),
            );
            return Ok(Attach::Gap);
        }
        if resync {
            out = channel.resync_locked(&mut st);
        }
        let (tx, mut rx) = tokio::sync::mpsc::channel::<(u64, String)>(SUB_QUEUE_FRAMES);
        st.subs.insert(
            conn.id,
            SubEntry {
                tx,
                pending_gap: None,
            },
        );
        drop(st);

        for frame in out {
            let _ = conn.try_send(frame);
        }
        // Forward this subscription's frames onto the connection. One task per
        // subscription keeps per-channel drop decisions local; it ends when
        // the channel drops the sender or the connection closes.
        let conn_tx = conn.tx.clone();
        let shutdown = conn.shutdown.clone();
        let kind = channel.kind;
        let key_json = serde_json::to_string(&channel.key).unwrap_or_else(|_| "\"\"".into());
        tokio::spawn(async move {
            let mut pending_gap: Option<(u64, u64)> = None;
            while let Some((seq, frame)) = rx.recv().await {
                // An empty frame is the agent/auth "consumer stuck" marker.
                if frame.is_empty() {
                    let _ = conn_tx.try_send(Outbound::Close(1013, "slow_consumer"));
                    shutdown.close();
                    break;
                }
                if shutdown.is_closed() {
                    break;
                }
                if let Some((from, to)) = pending_gap.take() {
                    let gap = format!(
                        "{{\"op\":\"gap\",\"channel\":{key_json},\"from\":{from},\"to\":{to},\"dropped\":true}}"
                    );
                    if conn_tx.try_send(Outbound::Text(gap)).is_err() {
                        pending_gap = Some((from, to));
                    }
                }
                if conn_tx.try_send(Outbound::Text(frame)).is_err() {
                    match kind {
                        ChannelKind::Terminal | ChannelKind::File => {
                            pending_gap = Some(match pending_gap {
                                Some((from, _)) => (from, seq),
                                None => (seq, seq),
                            });
                        }
                        ChannelKind::Agent | ChannelKind::Auth => {
                            let _ = conn_tx.try_send(Outbound::Close(1013, "slow_consumer"));
                            shutdown.close();
                            break;
                        }
                    }
                }
            }
        });
        Ok(Attach::Subscribed)
    }

    pub fn unsubscribe(self: &Arc<Self>, conn: &ConnHandle, key: &str) {
        self.detach(key, conn.id);
    }

    /// Publishes one live event. `cursor` is the resume cursor for terminal and
    /// file channels; agent channels resume by sequence number.
    pub fn publish(&self, key: &str, payload: Arc<str>, cursor: Cursor) {
        let channel = self.channels.lock().get(key).cloned();
        let Some(channel) = channel else { return };
        {
            let mut st = channel.state.lock();
            channel.push_locked(&mut st, payload, cursor, false, true);
        }
        self.budget();
    }

    fn budget(&self) {
        let budget = self.cfg.budget_bytes.max(16 * 1024);
        if self.total.load(Ordering::Relaxed) <= budget {
            return;
        }
        let channels: Vec<Arc<Channel>> = self.channels.lock().values().cloned().collect();
        // First bring every replayable channel down to half its capacity;
        // still over budget, empty them (pinned snapshots stay) in priority
        // order. Agent history goes last because a client cannot rebuild it
        // from a delta, only from a session snapshot.
        for target in [Target::Half, Target::Empty] {
            for kind in [ChannelKind::Terminal, ChannelKind::File, ChannelKind::Agent] {
                for channel in channels.iter().filter(|channel| channel.kind == kind) {
                    let (bytes, capacity) = {
                        let st = channel.state.lock();
                        (st.ring.bytes, st.ring.capacity)
                    };
                    let goal = match target {
                        Target::Half => capacity / 2,
                        Target::Empty => 0,
                    };
                    if bytes <= goal {
                        continue;
                    }
                    let removed = channel.state.lock().ring.trim_to(goal);
                    if removed > 0 {
                        self.total.fetch_sub(removed, Ordering::Relaxed);
                    }
                    if self.total.load(Ordering::Relaxed) <= budget {
                        return;
                    }
                }
            }
        }
    }

    fn shrink_idle(&self) {
        let now = Instant::now();
        let channels: Vec<Arc<Channel>> = self.channels.lock().values().cloned().collect();
        for channel in channels {
            let mut st = channel.state.lock();
            st.ring.shrink(now);
        }
    }

    pub fn shutdown(&self) {
        for shutdown in self.conn_shutdowns.lock().values() {
            shutdown.close();
        }
        let channels: Vec<Arc<Channel>> = self.channels.lock().values().cloned().collect();
        for channel in channels {
            if let Some(task) = channel.producer.lock().take() {
                task.abort();
            }
            let mut st = channel.state.lock();
            for (_, sub) in st.subs.drain() {
                let _ = sub
                    .tx
                    .try_send((0, "{\"op\":\"bye\",\"reason\":\"shutdown\"}".into()));
            }
            st.ring.clear();
        }
        self.channels.lock().clear();
        self.total.store(0, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> Value {
        let channels: Vec<Value> = self
            .channels
            .lock()
            .values()
            .map(|channel| channel.stats())
            .collect();
        let connections = self.connections.lock();
        let pages: Vec<Value> = {
            let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
            for (_, info) in connections.iter() {
                let label = info.holder.clone().unwrap_or_else(|| "unknown".into());
                let entry = counts.entry(label).or_insert((0, 0));
                entry.0 += 1;
                entry.1 += info.channels.len();
            }
            counts
                .into_iter()
                .map(|(holder, (sockets, channels))| {
                    json!({"holder": holder, "connections": sockets, "channels": channels})
                })
                .collect()
        };
        let by_kind = |kind: ChannelKind| {
            channels
                .iter()
                .filter(|channel| channel["kind"] == kind.as_str())
                .count()
        };
        json!({
            "protocol": 1,
            "uptimeMs": self.started.elapsed().as_millis() as u64,
            "connections": self.connected.load(Ordering::Relaxed),
            "maxConnectionsPerHolder": MAX_CONNECTIONS_PER_HOLDER,
            "channels": channels.len(),
            "channelsByKind": {
                "agent": by_kind(ChannelKind::Agent),
                "terminal": by_kind(ChannelKind::Terminal),
                "file": by_kind(ChannelKind::File),
                "auth": by_kind(ChannelKind::Auth),
            },
            "ringBytes": self.total.load(Ordering::Relaxed),
            "ringBudgetBytes": self.cfg.budget_bytes,
            "dropped": self.dropped.load(Ordering::Relaxed),
            "gaps": self.gaps.load(Ordering::Relaxed),
            "pages": pages,
            "channelList": channels,
        })
    }
}

fn parse_cursor(value: &Value) -> Option<Cursor> {
    match value {
        Value::Number(number) => number.as_u64().map(Cursor::Num),
        Value::String(text) => Some(Cursor::Text(text.clone())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(text: &str) -> Arc<str> {
        Arc::from(text.to_string())
    }

    #[test]
    fn ring_grows_on_high_water_and_caps() {
        let mut ring = Ring::new(1024, 4096);
        for index in 0..40 {
            ring.push(
                index + 1,
                Cursor::Num(index),
                payload("0123456789012345678901234567890123456789"),
                false,
            );
        }
        assert!(ring.capacity > 1024);
        assert!(ring.capacity <= 4096);
        assert!(ring.bytes <= ring.capacity);
    }

    #[test]
    fn ring_keeps_pinned_snapshot() {
        let mut ring = Ring::new(1024, 2048);
        ring.push(1, Cursor::Num(0), payload("snapshot"), true);
        for index in 0..200 {
            ring.push(
                index + 2,
                Cursor::Num(index),
                payload("chunk-chunk-chunk-chunk"),
                false,
            );
        }
        let first = ring.events.front().unwrap();
        assert!(first.pinned);
        assert_eq!(first.seq, 1);
    }

    #[test]
    fn ring_shrinks_after_idle() {
        let mut ring = Ring::new(1024, 8192);
        for index in 0..100 {
            ring.push(
                index + 1,
                Cursor::Num(index),
                payload("0123456789012345678901234567890123456789"),
                false,
            );
        }
        let grown = ring.capacity;
        assert!(grown > 1024);
        ring.clear();
        let now = Instant::now();
        assert!(
            !ring.shrink(now),
            "the first low reading only starts the idle clock"
        );
        assert!(ring.shrink(now + Duration::from_secs(61)));
        assert!(ring.capacity < grown);
    }

    struct TestSource {
        kind: ChannelKind,
    }

    impl sources::Source for TestSource {
        fn kind(&self) -> ChannelKind {
            self.kind
        }

        fn snapshot(&self) -> Vec<(Arc<str>, Cursor)> {
            vec![(payload("{\"type\":\"snapshot\"}"), Cursor::Num(1))]
        }

        fn start(
            self: Arc<Self>,
            _hub: Arc<Hub>,
            _key: String,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
            Box::pin(async {})
        }
    }

    fn test_channel(kind: ChannelKind) -> Arc<Channel> {
        Channel::new(
            "test:1".into(),
            Arc::new(TestSource { kind }),
            HubConfig::default(),
            Arc::new(AtomicUsize::new(0)),
        )
    }

    #[test]
    fn offset_replay_asks_for_a_snapshot_when_the_ring_starts_late() {
        let channel = test_channel(ChannelKind::Terminal);
        let mut st = channel.state.lock();
        channel.push_locked(
            &mut st,
            payload("{\"type\":\"output\"}"),
            Cursor::Num(40),
            true,
            false,
        );
        assert!(
            channel.replay_locked(&st, &Cursor::Num(10)).is_err(),
            "10 is before the snapshot"
        );
        assert!(
            channel
                .replay_locked(&st, &Cursor::Num(40))
                .unwrap()
                .is_empty()
        );
        channel.push_locked(
            &mut st,
            payload("{\"type\":\"output\"}"),
            Cursor::Num(50),
            false,
            false,
        );
        assert_eq!(
            channel.replay_locked(&st, &Cursor::Num(40)).unwrap().len(),
            1
        );
    }

    #[test]
    fn seq_replay_uses_sequence_numbers() {
        let channel = test_channel(ChannelKind::Agent);
        let mut st = channel.state.lock();
        channel.push_locked(
            &mut st,
            payload("{\"type\":\"connected\"}"),
            Cursor::Num(0),
            true,
            false,
        );
        channel.push_locked(
            &mut st,
            payload("{\"type\":\"message_update\"}"),
            Cursor::Num(0),
            false,
            false,
        );
        assert!(
            channel.replay_locked(&st, &Cursor::Num(0)).is_err(),
            "0 is before the snapshot"
        );
        assert!(
            channel
                .replay_locked(&st, &Cursor::Num(99))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            channel.replay_locked(&st, &Cursor::Num(1)).unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn a_fresh_channel_snapshots_even_with_a_client_cursor() {
        let hub = Hub::new(HubConfig::default());
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let conn = hub.register_conn(None, tx).unwrap();
        let channel = Channel::new(
            "terminal:t".into(),
            Arc::new(TestSource {
                kind: ChannelKind::Terminal,
            }),
            HubConfig::default(),
            hub.total.clone(),
        );
        hub.channels
            .lock()
            .insert("terminal:t".into(), channel.clone());
        // A remounted terminal carries the cursor of its old mount; the new
        // channel has no history, so it must answer with a snapshot rather
        // than replaying nothing.
        let attached = hub.attach(&channel, &conn, Some(json!(1234))).unwrap();
        assert_eq!(attached, Attach::Subscribed);
        match rx.try_recv().expect("a frame") {
            Outbound::Text(text) => assert!(text.contains("snapshot"), "{text}"),
            Outbound::Close(_, reason) => panic!("closed: {reason}"),
        }
    }

    #[test]
    fn hub_budget_trims_terminal_history_first() {
        let cfg = HubConfig {
            budget_bytes: 64 * 1024,
            terminal_max: 256 * 1024,
            agent_max: 256 * 1024,
            file_max: 64 * 1024,
            auth_max: 64 * 1024,
        };
        let hub = Hub::new(cfg.clone());
        let terminal = Channel::new(
            "terminal:t".into(),
            Arc::new(TestSource {
                kind: ChannelKind::Terminal,
            }),
            cfg.clone(),
            hub.total.clone(),
        );
        let agent = Channel::new(
            "agent:a".into(),
            Arc::new(TestSource {
                kind: ChannelKind::Agent,
            }),
            cfg,
            hub.total.clone(),
        );
        {
            let mut st = terminal.state.lock();
            for index in 0..2000 {
                terminal.push_locked(
                    &mut st,
                    payload("0123456789012345678901234567890123456789012345678901234567890123"),
                    Cursor::Num(index),
                    false,
                    false,
                );
            }
        }
        {
            let mut st = agent.state.lock();
            agent.push_locked(&mut st, payload("agent-event"), Cursor::Num(0), true, false);
        }
        hub.channels
            .lock()
            .insert("terminal:t".into(), terminal.clone());
        hub.channels.lock().insert("agent:a".into(), agent.clone());
        assert!(hub.total.load(Ordering::Relaxed) > hub.cfg.budget_bytes);
        hub.budget();
        assert!(hub.total.load(Ordering::Relaxed) <= hub.cfg.budget_bytes);
        assert!(
            terminal.state.lock().ring.bytes < agent.state.lock().ring.bytes + 128 * 1024,
            "terminal history paid for the budget"
        );
    }

    #[test]
    fn cursor_parse_accepts_numbers_and_strings() {
        assert!(matches!(parse_cursor(&json!(12)), Some(Cursor::Num(12))));
        assert!(matches!(parse_cursor(&json!("1:2")), Some(Cursor::Text(_))));
        assert!(parse_cursor(&json!(null)).is_none());
    }
}
