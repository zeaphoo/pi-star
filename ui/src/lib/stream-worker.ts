/**
 * The SharedWorker behind the stream bus: one `/api/stream` WebSocket for the
 * whole browser profile, shared by every tab through MessagePorts.
 *
 * Browsers cap WebSocket connections far above what this needs (roughly 255
 * per host in Chromium, 200 globally in Firefox), and one socket per profile
 * never approaches either.
 */

import { createRelay, webSocketTransport, type RelayStatus } from "./stream-relay";

type WorkerPort = MessagePort;

const ports = new Map<string, WorkerPort>();
let nextTab = 1;
let relay: ReturnType<typeof createRelay> | null = null;

function holder(): string {
  const key = Symbol.for("pi-star:bus-holder");
  const store = globalThis as Record<PropertyKey, unknown>;
  const existing = store[key];
  if (typeof existing === "string") return existing;
  const id = `worker-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
  store[key] = id;
  return id;
}

function ensureRelay(): ReturnType<typeof createRelay> {
  if (relay) return relay;
  const created = createRelay(webSocketTransport(holder()), {
    toTab(tab, message) {
      ports.get(tab)?.postMessage(message);
    },
  });
  relay = created;
  created.connect();
  return created;
}

function statusOf(current: ReturnType<typeof createRelay>): RelayStatus {
  return current.snapshot().status;
}

const workerScope = self as unknown as {
  onconnect: ((event: MessageEvent) => void) | null;
};

workerScope.onconnect = (event: MessageEvent) => {
  const port = (event as unknown as { ports: WorkerPort[] }).ports?.[0];
  if (!port) return;
  const tab = `w${nextTab++}`;
  ports.set(tab, port);
  port.onmessage = (message: MessageEvent) => {
    const data = message.data as Record<string, unknown> | null;
    if (!data || typeof data !== "object") return;
    if (data.type === "bye") {
      ports.delete(tab);
      relay?.tabGone(tab);
      port.close();
      return;
    }

    ensureRelay().fromTab(tab, data);
  };
  port.start();
  const current = ensureRelay();
  port.postMessage({ type: "status", state: statusOf(current) });
};
