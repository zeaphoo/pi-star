/**
 * The multiplexing relay: it owns the single `/api/stream` WebSocket and fans
 * channels out to the pages that subscribe to them. The same code runs inside
 * the SharedWorker, inside a leader tab (when SharedWorker is unavailable) and
 * inside a lone page (the last-resort transport).
 *
 * A "tab" is just an opaque id: the worker maps it to a MessagePort, the
 * leader maps it to a BroadcastChannel peer, and a lone page is one local tab.
 */

export type RelayStatus = "connecting" | "open" | "closed";

export interface RelayTransport {
  connect(handlers: {
    onOpen(): void;
    onText(text: string): void;
    onClose(): void;
  }): { send(text: string): void; close(): void };
}

export interface RelayOptions {
  /** Deliver a message to one tab; the transport decides how. */
  toTab(tab: string, message: unknown): void;
  retryDelayMs?(attempt: number): number;
  setTimeout?(handler: () => void, ms: number): unknown;
  clearTimeout?(timer: unknown): void;
  now?(): number;
}

interface ChannelEntry {
  tabs: Set<string>;
  serverId: number | null;
  cursor: number | string | null;
}

export interface RelaySnapshot {
  status: RelayStatus;
  sockets: number;
  channels: { channel: string; tabs: number }[];
  dropped: number;
  gaps: number;
  reconnects: number;
}

const MAX_QUEUE = 64;

export function createRelay(transport: RelayTransport, options: RelayOptions) {
  const channels = new Map<string, ChannelEntry>();
  /** Which tab asked for a command, so its result goes to that tab even when
   *  it does not subscribe to the channel (a prompt on a new session). */
  const requestOwners = new Map<number, string>();
  const timers = options.setTimeout ?? ((handler, ms) => setTimeout(handler, ms));
  const clear = options.clearTimeout ?? ((timer) => clearTimeout(timer as ReturnType<typeof setTimeout>));
  const retryDelay = options.retryDelayMs ?? ((attempt: number) => Math.min(30_000, 1000 * 2 ** attempt) + Math.random() * 500);
  let socket: { send(text: string): void; close(): void } | null = null;
  let status: RelayStatus = "closed";
  let serverReady = false;
  let attempts = 0;
  let reconnects = 0;
  let retryTimer: unknown = null;
  let disposed = false;
  let nextId = 1;
  let dropped = 0;
  let gaps = 0;
  const queued: string[] = [];

  function setStatus(next: RelayStatus): void {
    if (status === next) return;
    status = next;
    for (const tab of allTabs()) options.toTab(tab, { type: "status", state: next });
  }

  function allTabs(): Set<string> {
    const tabs = new Set<string>();
    for (const entry of channels.values()) for (const tab of entry.tabs) tabs.add(tab);
    return tabs;
  }

  function send(text: string): boolean {
    if (disposed) return false;
    if (!socket || !serverReady) {
      queued.push(text);
      if (queued.length > MAX_QUEUE) {
        queued.shift();
        dropped += 1;
      }
      return false;
    }
    try {
      socket.send(text);
      return true;
    } catch {
      queued.push(text);
      if (queued.length > MAX_QUEUE) {
        queued.shift();
        dropped += 1;
      }
      return false;
    }
  }

  function serverFrame(text: string): void {
    let frame: Record<string, unknown>;
    try {
      frame = JSON.parse(text) as Record<string, unknown>;
    } catch {
      return;
    }
    switch (frame.op) {
      case "ready":
        serverReady = true;
        attempts = 0;
        setStatus("open");
        for (const [channel, entry] of channels) {
          if (entry.tabs.size === 0) continue;
          entry.serverId = nextId++;
          send(JSON.stringify({ op: "sub", id: entry.serverId, channel, cursor: entry.cursor }));
        }
        // Drain in a bounded pass: a send that throws re-queues its frame, so
        // an unbounded `while queued.length > 0` would spin the page (or the
        // SharedWorker) at 100% CPU. Leave the rest for the next `ready`.
        let pending = queued.length;
        while (serverReady && pending > 0) {
          const next = queued.shift();
          pending -= 1;
          if (next && !send(next)) break;
        }
        return;
      case "ev": {
        const channel = String(frame.channel ?? "");
        const entry = channels.get(channel);
        if (!entry) return;
        updateCursor(entry, frame);
        for (const tab of entry.tabs) options.toTab(tab, { type: "frame", frame });
        return;
      }
      case "gap": {
        const channel = String(frame.channel ?? "");
        const entry = channels.get(channel);
        gaps += 1;
        if (entry) entry.cursor = null;
        for (const tab of entry ? entry.tabs : []) options.toTab(tab, { type: "frame", frame });
        return;
      }
      case "sub_ok": {
        const id = Number(frame.id);
        const found = findByServerId(id);
        if (!found) return;
        found.entry.serverId = id;
        for (const tab of found.entry.tabs) {
          options.toTab(tab, { type: "frame", frame: { op: "ack", channel: found.channel } });
        }
        return;
      }
      case "sub_err": {
        const id = Number(frame.id);
        const found = findByServerId(id);
        const channel = found?.channel ?? String(frame.channel ?? "");
        const entry = found?.entry;
        if (entry) entry.serverId = null;
        for (const tab of entry ? entry.tabs : []) {
          options.toTab(tab, {
            type: "frame",
            frame: { op: "sub_err", channel, code: frame.code, message: frame.message },
          });
        }
        return;
      }
      case "cmd_result": {
        const id = Number(frame.id);
        const owner = requestOwners.get(id);
        requestOwners.delete(id);
        if (owner) {
          options.toTab(owner, { type: "frame", frame });
          return;
        }
        const channel = String(frame.channel ?? "");
        const entry = channels.get(channel);
        for (const tab of entry ? entry.tabs : []) options.toTab(tab, { type: "frame", frame });
        return;
      }
      case "bye":
      case "pong":
      default:
        for (const tab of allTabs()) options.toTab(tab, { type: "frame", frame });
        return;
    }
  }

  function findByServerId(id: number): { channel: string; entry: ChannelEntry } | undefined {
    for (const [channel, entry] of channels) {
      if (entry.serverId === id) return { channel, entry };
    }
    return undefined;
  }

  function updateCursor(entry: ChannelEntry, frame: Record<string, unknown>): void {
    const channel = String(frame.channel ?? "");
    const data = (frame.data ?? {}) as Record<string, unknown>;
    const seq = Number(frame.seq ?? 0);
    if (channel.startsWith("terminal:")) {
      if (typeof data.offset === "number") entry.cursor = data.offset;
    } else if (channel.startsWith("file:")) {
      if (typeof data.size === "number") entry.cursor = `${data.size}:${String(data.mtime ?? "")}`;
    } else if (channel.startsWith("agent:")) {
      entry.cursor = seq;
    }
  }

  function subscribe(tab: string, channel: string, cursor: number | string | null | undefined): void {
    let entry = channels.get(channel);
    if (!entry) {
      entry = { tabs: new Set(), serverId: null, cursor: cursor ?? null };
      channels.set(channel, entry);
    }
    const first = entry.tabs.size === 0;
    entry.tabs.add(tab);
    if (first) {
      if (cursor !== undefined && cursor !== null) entry.cursor = cursor;
      if (!serverReady) return;
      entry.serverId = nextId++;
      send(JSON.stringify({ op: "sub", id: entry.serverId, channel, cursor: entry.cursor }));
    }
  }

  function unsubscribe(tab: string, channel: string): void {
    const entry = channels.get(channel);
    if (!entry) return;
    entry.tabs.delete(tab);
    if (entry.tabs.size === 0) {
      if (entry.serverId !== null) send(JSON.stringify({ op: "unsub", id: entry.serverId }));
      channels.delete(channel);
    }
  }

  function resync(tab: string, channel: string): void {
    const entry = channels.get(channel);
    if (!entry) return;
    entry.tabs.add(tab);
    entry.cursor = null;
    if (!serverReady) return;
    if (entry.serverId !== null) send(JSON.stringify({ op: "unsub", id: entry.serverId }));
    entry.serverId = nextId++;
    send(JSON.stringify({ op: "sub", id: entry.serverId, channel, cursor: null }));
  }

  function fromTab(tab: string, message: Record<string, unknown>): void {
    switch (message.type) {
      case "sub":
        subscribe(tab, String(message.channel ?? ""), message.cursor as number | string | null | undefined);
        return;
      case "unsub":
        unsubscribe(tab, String(message.channel ?? ""));
        return;
      case "resync":
        resync(tab, String(message.channel ?? ""));
        return;
      case "cursor": {
        const entry = channels.get(String(message.channel ?? ""));
        if (entry && entry.cursor === null) {
          entry.cursor = (message.cursor as number | string | null) ?? null;
        }
        return;
      }
      case "send": {
        const frame = message.frame as Record<string, unknown> | undefined;
        const payload = frame?.payload as Record<string, unknown> | undefined;
        if (frame?.op === "cmd" && typeof payload?.requestId === "number") {
          requestOwners.set(payload.requestId, tab);
        }
        send(JSON.stringify(frame));
        return;
      }
      case "bye":
        tabGone(tab);
        return;
      default:
        return;
    }
  }

  function tabGone(tab: string): void {
    for (const [id, owner] of [...requestOwners]) {
      if (owner === tab) requestOwners.delete(id);
    }
    for (const [channel, entry] of [...channels]) {
      if (!entry.tabs.has(tab)) continue;
      unsubscribe(tab, channel);
    }
  }

  function connect(): void {
    if (disposed) return;
    setStatus("connecting");
    serverReady = false;
    socket = transport.connect({
      onOpen() {
        // The server's `ready` frame follows; wait for it before subscribing
        // so ids align with a fresh connection.
      },
      onText: serverFrame,
      onClose() {
        socket = null;
        serverReady = false;
        if (disposed) return;
        attempts += 1;
        reconnects += 1;
        setStatus("closed");
        retryTimer = timers(() => {
          retryTimer = null;
          connect();
        }, retryDelay(attempts));
      },
    });
  }

  function dispose(): void {
    disposed = true;
    if (retryTimer) clear(retryTimer);
    retryTimer = null;
    try {
      socket?.close();
    } catch {
      // Already closed.
    }
    socket = null;
    channels.clear();
  }

  return {
    fromTab,
    tabGone,
    dispose,
    connect,
    snapshot(): RelaySnapshot {
      return {
        status,
        sockets: socket && serverReady ? 1 : 0,
        channels: [...channels].map(([channel, entry]) => ({ channel, tabs: entry.tabs.size })),
        dropped,
        gaps,
        reconnects,
      };
    },
  };
}

/** The browser end of `/api/stream`: one WebSocket, reconnected by the relay. */
export function webSocketTransport(holder: string): RelayTransport {
  return {
    connect(handlers) {
      const protocol = location.protocol === "https:" ? "wss:" : "ws:";
      const url = `${protocol}//${location.host}/api/stream?holder=${encodeURIComponent(holder)}`;
      const socket = new WebSocket(url);
      socket.onopen = () => handlers.onOpen();
      socket.onmessage = (event) => {
        if (typeof event.data === "string") handlers.onText(event.data);
      };
      socket.onclose = () => handlers.onClose();
      socket.onerror = () => {
        try {
          socket.close();
        } catch {
          // Already closing.
        }
      };
      return {
        send: (text) => socket.send(text),
        close: () => socket.close(),
      };
    },
  };
}
