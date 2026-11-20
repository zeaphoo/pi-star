import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url, { interopDefault: true, moduleCache: false });
const {
  __injectBusMessageForTests,
  __resetBusForTests,
  __setBusLinkForTests,
  busSnapshot,
  request,
  subscribe,
} = await jiti.import("./stream-bus.ts");

function connectedBus() {
  const sent = [];
  __resetBusForTests();
  __setBusLinkForTests({ send(message) { sent.push(message); }, dispose() {} });
  const frame = (payload) => __injectBusMessageForTests({ type: "frame", frame: payload });
  return { sent, frame };
}

test("two handlers on one channel share one subscription", () => {
  const { sent, frame } = connectedBus();
  const seen = [];
  const offA = subscribe("agent:s1", (data) => seen.push(["a", data.type]));
  const offB = subscribe("agent:s1", (data) => seen.push(["b", data.type]));
  assert.deepEqual(sent, [{ type: "sub", channel: "agent:s1", cursor: null }]);

  frame({ op: "ev", channel: "agent:s1", seq: 7, data: { type: "message_update" } });
  assert.deepEqual(seen, [["a", "message_update"], ["b", "message_update"]]);
  assert.deepEqual(busSnapshot().channels, [{ channel: "agent:s1", kind: "agent" }]);

  offA();
  assert.equal(sent.filter((message) => message.type === "unsub").length, 0);
  offB();
  assert.deepEqual(sent.at(-1), { type: "unsub", channel: "agent:s1" });
});

test("a gap drops the cursor and asks the relay to resubscribe", () => {
  const { sent, frame } = connectedBus();
  const resyncs = [];
  subscribe("terminal:t1", (data) => resyncs.push(data), { cursor: 12 });
  assert.equal(sent[0].cursor, 12);
  frame({ op: "ev", channel: "terminal:t1", seq: 2, data: { type: "output", offset: 40, data: "x" } });
  frame({ op: "gap", channel: "terminal:t1", from: 3, to: 9 });
  assert.deepEqual(resyncs.at(-1), { type: "resync", reason: "gap" });
  assert.deepEqual(sent.at(-1), { type: "resync", channel: "terminal:t1" });
  assert.equal(busSnapshot().gaps, 1);
});

test("file channels resume by version and agent channels by sequence", () => {
  const { sent, frame } = connectedBus();
  subscribe("file:/work/app.ts", () => {});
  const offAgent = subscribe("agent:s1", () => {});
  frame({ op: "ev", channel: "file:/work/app.ts", seq: 1, data: { type: "change", size: 12, mtime: "t" } });
  frame({ op: "ev", channel: "agent:s1", seq: 9, data: { type: "message_update" } });
  assert.deepEqual(sent.filter((message) => message.type === "sub").map((message) => message.cursor), [null, null]);
  // A later subscriber uses the cursor the page kept for that channel.
  offAgent();
  subscribe("agent:s1", () => {});
  const last = sent.filter((message) => message.type === "sub").at(-1);
  assert.equal(last.cursor, 9);
});

test("request resolves with the server's command result", async () => {
  const { sent, frame } = connectedBus();
  const pending = request("agent:s1", { type: "prompt", message: "hi" });
  const sentFrame = sent.at(-1).frame;
  assert.equal(sentFrame.op, "cmd");
  assert.equal(sentFrame.payload.type, "prompt");
  frame({ op: "cmd_result", channel: "agent:s1", id: sentFrame.payload.requestId, status: 200, body: { success: true } });
  const reply = await pending;
  assert.deepEqual(reply, { status: 200, body: { success: true } });
});
