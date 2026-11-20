import assert from "node:assert/strict";
import test from "node:test";
import { createRelay } from "./stream-relay.ts";

function harness() {
  const sent = [];
  const toTab = [];
  const timers = [];
  let handlers = null;
  const relay = createRelay(
    {
      connect(next) {
        handlers = next;
        return {
          send(text) { sent.push(JSON.parse(text)); },
          close() { next.onClose(); },
        };
      },
    },
    {
      toTab(tab, message) { toTab.push([tab, message]); },
      setTimeout(handler) { timers.push(handler); return timers.length; },
      clearTimeout() {},
      retryDelayMs() { return 1; },
    },
  );
  relay.connect();
  return {
    relay,
    sent,
    toTab,
    timers,
    ready() { handlers.onText(JSON.stringify({ op: "ready", protocol: 1 })); },
    frame(frame) { handlers.onText(JSON.stringify(frame)); },
    drop() { handlers.onClose(); },
  };
}

test("two tabs share one server subscription", () => {
  const h = harness();
  h.ready();
  h.relay.fromTab("a", { type: "sub", channel: "terminal:t1", cursor: 12 });
  h.relay.fromTab("b", { type: "sub", channel: "terminal:t1" });
  assert.deepEqual(h.sent, [{ op: "sub", id: 1, channel: "terminal:t1", cursor: 12 }]);

  h.frame({ op: "ev", channel: "terminal:t1", seq: 5, data: { type: "output", offset: 30, data: "x" } });
  const delivered = h.toTab.filter(([, message]) => message.type === "frame" && message.frame.op === "ev");
  assert.deepEqual(delivered.map(([tab]) => tab), ["a", "b"]);

  h.relay.fromTab("a", { type: "unsub", channel: "terminal:t1" });
  assert.equal(h.sent.length, 1, "b still holds it");
  h.relay.fromTab("b", { type: "unsub", channel: "terminal:t1" });
  assert.deepEqual(h.sent.at(-1), { op: "unsub", id: 1 });
});

test("reconnect resubscribes with the last cursor", () => {
  const h = harness();
  h.ready();
  h.relay.fromTab("a", { type: "sub", channel: "agent:s1" });
  h.frame({ op: "ev", channel: "agent:s1", seq: 42, data: { type: "message_update" } });
  h.drop();
  assert.equal(h.timers.length, 1);
  h.timers[0]();
  h.ready();
  assert.deepEqual(h.sent.at(-1), { op: "sub", id: 2, channel: "agent:s1", cursor: 42 });
});

test("a gap clears the cursor and asks the page to resync", () => {
  const h = harness();
  h.ready();
  h.relay.fromTab("a", { type: "sub", channel: "terminal:t1" });
  h.frame({ op: "ev", channel: "terminal:t1", seq: 3, data: { type: "output", offset: 9, data: "x" } });
  h.frame({ op: "gap", channel: "terminal:t1", from: 4, to: 9 });
  assert.ok(h.toTab.some(([, message]) => message.type === "frame" && message.frame.op === "gap"));
  assert.equal(h.relay.snapshot().gaps, 1);
  h.relay.fromTab("a", { type: "resync", channel: "terminal:t1" });
  const resub = h.sent.filter((frame) => frame.op === "sub").at(-1);
  assert.equal(resub.cursor, null);
});

test("closing a tab releases its channels", () => {
  const h = harness();
  h.ready();
  h.relay.fromTab("a", { type: "sub", channel: "agent:s1" });
  h.relay.fromTab("b", { type: "sub", channel: "agent:s1" });
  h.relay.tabGone("a");
  assert.equal(h.relay.snapshot().channels[0].tabs, 1);
  h.relay.tabGone("b");
  assert.deepEqual(h.relay.snapshot().channels, []);
});
