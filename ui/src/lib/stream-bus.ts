/**
 * The page-side stream bus: the only place in the UI allowed to hold a live
 * connection. Components subscribe to a channel name (`agent:<sid>`,
 * `terminal:<id>`, `file:<path>`, `auth:<provider>`) and never see a socket.
 *
 * One WebSocket per browser profile is shared by every tab through a
 * SharedWorker. When that is unavailable a leader tab (Web Locks) holds the
 * socket and relays through BroadcastChannel; a lone page is the last resort.
 * Browsers without either fall back to the old per-feature SSE endpoints so a
 * degraded environment still works, at the old connection cost.
 */

import { createRelay, webSocketTransport, type RelayStatus } from "./stream-relay";
import { encodeFilePathForApi } from "./file-paths";

export type BusCursor = number | string | null;

export interface BusEventMeta {
  channel: string;
  seq: number;
}

export type BusHandler = (data: unknown, meta: BusEventMeta) => void;

export interface SubscriptionOptions {
  cursor?: BusCursor;
  /** The server accepted the subscription (it is now live). */
  onOpen?: () => void;
  onError?: (error: { code: string; message: string }) => void;
}

interface Entry {
  handler: BusHandler;
  onOpen?: () => void;
  onError?: (error: { code: string; message: string }) => void;
}

interface BrowserLink {
  send(message: Record<string, unknown>): void;
  dispose(): void;
}

const CHANNEL_NAME = "pi-star-stream-bus";
const LEADER_LOCK = "pi-star-stream-bus-leader";
const PAGE_ID_KEY = Symbol.for("pi-star:page-id");

const channels = new Map<string, Set<Entry>>();
const cursors = new Map<string, BusCursor>();
const pending = new Map<number, { resolve: (value: BusReply) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>();

let link: BrowserLink | null | undefined;
let status: RelayStatus | "legacy" | "idle" = "idle";
let requestId = 0;
let gaps = 0;
let dropped = 0;

export class BusUnavailable extends Error {
  constructor() {
    super("The stream bus is not available in this environment");
    this.name = "BusUnavailable";
  }
}

export interface BusReply {
  status: number;
  body: unknown;
}

function pageId(): string {
  const store = globalThis as Record<PropertyKey, unknown>;
  const existing = store[PAGE_ID_KEY];
  if (typeof existing === "string") return existing;
  const id = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
  store[PAGE_ID_KEY] = id;
  return id;
}

/**
 * A rollback switch for one release: `?stream=legacy` or the localStorage flag
 * `pi-star:stream=legacy` restores the per-feature SSE endpoints without a
 * rebuild. Absent, the bus is used whenever the browser can host it.
 */
function busDisabled(): boolean {
  if (typeof location === "undefined") return false;
  try {
    if (new URLSearchParams(location.search).get("stream") === "legacy") return true;
    if (typeof localStorage !== "undefined" && localStorage.getItem("pi-star:stream") === "legacy") return true;
  } catch {
    // Storage can be blocked; the query parameter still works.
  }
  return false;
}

function busSupported(): boolean {
  if (typeof window === "undefined" || typeof location === "undefined") return false;
  if (busDisabled()) return false;
  return typeof WebSocket !== "undefined" || typeof SharedWorker !== "undefined";
}

function ensureLink(): BrowserLink | null {
  if (link !== undefined) return link;
  if (!busSupported()) {
    link = null;
    status = "legacy";
    return null;
  }
  link = createWorkerLink() ?? createLeaderLink() ?? createDirectLink();
  if (!link) status = "legacy";
  return link;
}

function createWorkerLink(): BrowserLink | null {
  if (typeof SharedWorker === "undefined") return null;
  try {
    const worker = new SharedWorker(new URL("./stream-worker.ts", import.meta.url), { type: "module" });
    worker.port.onmessage = (event: MessageEvent) => onLinkMessage(event.data);
    worker.port.start();
    return {
      send: (message) => worker.port.postMessage(message),
      dispose: () => worker.port.close(),
    };
  } catch {
    return null;
  }
}

/**
 * Web Locks elect one tab to hold the socket; every tab talks to it over
 * BroadcastChannel. When the leader closes its lock is released and the next
 * tab takes over.
 */
function createLeaderLink(): BrowserLink | null {
  if (typeof BroadcastChannel === "undefined" || !navigator.locks?.request) return null;
  const tab = `t${Math.random().toString(36).slice(2, 10)}`;
  const channel = new BroadcastChannel(CHANNEL_NAME);
  let relay: ReturnType<typeof createRelay> | null = null;
  let leader = false;

  const post = (message: Record<string, unknown>) => {
    channel.postMessage({ __piStarBus: 1, tab, ...message });
  };

  const becomeLeader = () => {
    leader = true;
    relay = createRelay(webSocketTransport(pageId()), {
      toTab(target, message) {
        if (target === tab) onLinkMessage(message);
        else channel.postMessage({ __piStarBus: 1, kind: "toTab", tab: target, message });
      },
    });
    relay.connect();
    post({ kind: "leader" });
    status = "open";
    resubscribeAll();
  };

  channel.onmessage = (event: MessageEvent) => {
    const data = event.data as Record<string, unknown> | null;
    if (!data || data.__piStarBus !== 1) return;
    if (data.kind === "leader") {
      if (data.tab === tab) return;
      leader = false;
      relay = null;
      status = "open";
      // The new leader's relay starts empty: announce every subscription.
      resubscribeAll();
      return;
    }
    if (data.kind === "fromTab" && leader && relay) {
      relay.fromTab(String(data.tab), data.message as Record<string, unknown>);
      return;
    }
    if (data.kind === "toTab" && data.tab === tab) {
      onLinkMessage(data.message);
    }
  };

  void navigator.locks
    .request(LEADER_LOCK, { mode: "exclusive" }, async () => {
      becomeLeader();
      await new Promise<void>(() => {
        // Held until the tab goes away.
      });
    })
    .catch(() => {
      // Lock requests can fail (an insecure context, a closed worker); the
      // direct fallback still gives this page a socket of its own.
      leader = false;
    });

  return {
    send(message) {
      if (leader && relay) {
        relay.fromTab(tab, message);
        return;
      }
      if (leader) return;
      post({ kind: "fromTab", message });
    },
    dispose() {
      channel.close();
      relay?.dispose();
    },
  };
}

function createDirectLink(): BrowserLink {
  const tab = "local";
  const relay = createRelay(webSocketTransport(pageId()), {
    toTab(_target, message) {
      onLinkMessage(message);
    },
  });
  relay.connect();
  return {
    send: (message) => relay.fromTab(tab, message),
    dispose: () => relay.dispose(),
  };
}

function onLinkMessage(message: unknown): void {
  const data = message as Record<string, unknown> | null;
  if (!data || typeof data !== "object") return;
  if (data.type === "status") {
    const next = String(data.state ?? "closed") as RelayStatus;
    status = next;
    if (next === "open") resubscribeAll();
    return;
  }
  if (data.type !== "frame") return;
  const frame = data.frame as Record<string, unknown> | null;
  if (!frame) return;
  handleFrame(frame);
}

function handleFrame(frame: Record<string, unknown>): void {
  const op = String(frame.op ?? "");
  const channel = String(frame.channel ?? "");
  if (op === "ev") {
    const meta: BusEventMeta = { channel, seq: Number(frame.seq ?? 0) };
    const data = frame.data;
    cursors.set(channel, nextCursor(channel, data, meta.seq));
    for (const entry of channels.get(channel) ?? []) {
      try {
        entry.handler(data, meta);
      } catch (error) {
        console.error("stream bus handler failed", error);
      }
    }
    return;
  }
  if (op === "gap") {
    gaps += 1;
    cursors.delete(channel);
    link?.send({ type: "resync", channel });
    for (const entry of channels.get(channel) ?? []) {
      try {
        entry.handler({ type: "resync", reason: "gap" }, { channel, seq: 0 });
      } catch {
        // A handler that throws must not break the bus.
      }
    }
    return;
  }
  if (op === "ack") {
    for (const entry of channels.get(channel) ?? []) entry.onOpen?.();
    return;
  }
  if (op === "sub_err") {
    const error = { code: String(frame.code ?? "unavailable"), message: String(frame.message ?? "Subscription failed") };
    for (const entry of channels.get(channel) ?? []) entry.onError?.(error);
    return;
  }
  if (op === "cmd_result") {
    const id = Number(frame.id);
    const waiter = pending.get(id);
    if (!waiter) return;
    pending.delete(id);
    clearTimeout(waiter.timer);
    waiter.resolve({ status: Number(frame.status ?? 500), body: frame.body });
    return;
  }
  if (op === "bye") {
    dropped += 1;
  }
}

function nextCursor(channel: string, data: unknown, seq: number): BusCursor {
  if (channel.startsWith("agent:")) return seq;
  const payload = data as Record<string, unknown> | null;
  if (channel.startsWith("terminal:")) {
    return typeof payload?.offset === "number" ? payload.offset : cursors.get(channel) ?? null;
  }
  if (channel.startsWith("file:")) {
    return typeof payload?.size === "number" ? `${payload.size}:${String(payload.mtime ?? "")}` : cursors.get(channel) ?? null;
  }
  return cursors.get(channel) ?? null;
}

function resubscribeAll(): void {
  if (!link) return;
  for (const channel of channels.keys()) {
    link.send({ type: "sub", channel, cursor: cursors.get(channel) ?? null });
  }
}

/** Subscribe to one channel; the returned function releases this listener. */
export function subscribe(channel: string, handler: BusHandler, options: SubscriptionOptions = {}): () => void {
  const active = ensureLink();
  if (!active) return subscribeLegacy(channel, handler, options);
  let entries = channels.get(channel);
  if (!entries) {
    entries = new Set();
    channels.set(channel, entries);
  }
  const entry: Entry = { handler, onOpen: options.onOpen, onError: options.onError };
  entries.add(entry);
  if (entries.size === 1) {
    if (options.cursor !== undefined && options.cursor !== null) cursors.set(channel, options.cursor);
    active.send({ type: "sub", channel, cursor: cursors.get(channel) ?? null });
  }
  return () => {
    entries.delete(entry);
    if (entries.size > 0) return;
    channels.delete(channel);
    link?.send({ type: "unsub", channel });
  };
}

/** Fire-and-forget command on a channel (terminal input, resize). */
export function sendCommand(channel: string, payload: Record<string, unknown>): boolean {
  const active = ensureLink();
  if (!active || status !== "open") return false;
  active.send({ type: "send", frame: { op: "cmd", channel, payload } });
  return true;
}

/**
 * Request/response over the bus (agent commands). Throws `BusUnavailable` when
 * no socket exists, which callers use to fall back to an HTTP POST.
 */
export function request(
  channel: string,
  payload: Record<string, unknown>,
  timeoutMs = 15_000,
): Promise<BusReply> {
  const active = ensureLink();
  if (!active) return Promise.reject(new BusUnavailable());
  const id = ++requestId;
  return new Promise<BusReply>((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error("The stream bus did not answer in time"));
    }, timeoutMs);
    pending.set(id, { resolve, reject, timer });
    active.send({
      type: "send",
      frame: { op: "cmd", channel, payload: { ...payload, requestId: id } },
    });
  });
}

/**
 * A hidden tab releases terminal and file channels after the grace and takes
 * them back when it returns; the agent channel stays so a run keeps updating
 * the conversation. The socket itself is shared, so this only trims work.
 */
const detached = new Set<string>();
let hiddenTimer: ReturnType<typeof setTimeout> | null = null;
const HIDDEN_GRACE_MS = 30_000;

function armHiddenRelease(): void {
  if (typeof document === "undefined") return;
  const schedule = () => {
    if (hiddenTimer !== null) return;
    hiddenTimer = setTimeout(() => {
      hiddenTimer = null;
      if (document.visibilityState !== "hidden") return;
      for (const channel of channels.keys()) {
        if (channel.startsWith("agent:")) continue;
        if (detached.has(channel)) continue;
        detached.add(channel);
        link?.send({ type: "unsub", channel });
      }
    }, HIDDEN_GRACE_MS);
  };
  const onVisibility = () => {
    if (document.visibilityState === "hidden") {
      schedule();
      return;
    }
    if (hiddenTimer !== null) {
      clearTimeout(hiddenTimer);
      hiddenTimer = null;
    }
    for (const channel of [...detached]) {
      detached.delete(channel);
      link?.send({ type: "sub", channel, cursor: cursors.get(channel) ?? null });
    }
  };
  document.addEventListener("visibilitychange", onVisibility);
  if (typeof window !== "undefined") {
    window.addEventListener("pagehide", () => link?.send({ type: "bye" }));
  }
}

armHiddenRelease();

/** Test seam: drive the bus without a browser transport. */
export function __setBusLinkForTests(custom: { send(message: Record<string, unknown>): void; dispose(): void } | null): void {
  link = custom;
  status = custom ? "open" : "legacy";
}

/** Test seam: deliver a server frame (or link message) to the page. */
export function __injectBusMessageForTests(message: unknown): void {
  onLinkMessage(message);
}

export function __resetBusForTests(): void {
  channels.clear();
  cursors.clear();
  pending.clear();
  link = undefined;
  status = "idle";
  gaps = 0;
  dropped = 0;
}

export function busAvailable(): boolean {
  return ensureLink() !== null;
}

/** Whether the socket is open right now; HTTP is the better path while connecting. */
export function busReady(): boolean {
  return ensureLink() !== null && status === "open";
}

export interface BusSnapshot {
  mode: "bus" | "legacy";
  state: string;
  sockets: number;
  channels: { channel: string; kind: string }[];
  gaps: number;
  dropped: number;
}

/** This page's subscriptions, for the Debug panel. */
export function busSnapshot(): BusSnapshot {
  return {
    mode: link ? "bus" : "legacy",
    state: status,
    sockets: link && (status === "open" || status === "connecting") ? 1 : 0,
    channels: [...channels.keys()].map((channel) => ({
      channel,
      kind: channel.split(":")[0] ?? channel,
    })),
    gaps,
    dropped,
  };
}

// A read-only diagnostic hook (the Debug panel and browser tests use it).
if (typeof window !== "undefined") {
  (window as unknown as { __piStarBus: () => BusSnapshot }).__piStarBus = busSnapshot;
}

/** Channels this page holds, for the Debug panel's source list. */
export function listBusChannels(): { channel: string; kind: string }[] {
  return [...channels.keys()].map((channel) => ({ channel, kind: channel.split(":")[0] ?? channel }));
}

// ── Legacy fallback (no WebSocket/SharedWorker: tests, exotic webviews) ──

function legacyUrl(channel: string): string | null {
  const [kind, ...rest] = channel.split(":");
  const value = rest.join(":");
  if (!value) return null;
  const holder = `?holder=${encodeURIComponent(pageId())}`;
  if (kind === "agent") return `/api/agent/${encodeURIComponent(value)}/events${holder}`;
  if (kind === "terminal") return `/api/terminal/${encodeURIComponent(value)}/events${holder}`;
  if (kind === "file") return `/api/files/${encodeFilePathForApi(value)}?type=watch&holder=${encodeURIComponent(pageId())}`;
  if (kind === "auth") return `/api/auth/login/${encodeURIComponent(value)}`;
  return null;
}

function subscribeLegacy(channel: string, handler: BusHandler, options: SubscriptionOptions): () => void {
  const url = legacyUrl(channel);
  if (!url) {
    options.onError?.({ code: "unknown_channel", message: `No legacy stream for ${channel}` });
    return () => {};
  }
  const source = new EventSource(url);
  source.onopen = () => options.onOpen?.();
  source.onmessage = (event) => {
    try {
      handler(JSON.parse(event.data), { channel, seq: 0 });
    } catch {
      // Ignore malformed frames, as the old clients did.
    }
  };
  if (channel.startsWith("file:")) {
    source.addEventListener("connected", (event) => {
      const payload = (event as MessageEvent).data;
      try {
        handler({ type: "connected", ...JSON.parse(payload) }, { channel, seq: 0 });
      } catch {
        handler({ type: "connected" }, { channel, seq: 0 });
      }
    });
    source.addEventListener("change", (event) => {
      const payload = (event as MessageEvent).data;
      try {
        handler({ type: "change", ...JSON.parse(payload) }, { channel, seq: 0 });
      } catch {
        handler({ type: "change" }, { channel, seq: 0 });
      }
    });
  }
  source.onerror = () => {
    options.onError?.({ code: "stream_error", message: "The event stream failed" });
  };
  return () => source.close();
}
