import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url);
const {
  TRACE_FILTER_ALL,
  buildTraceEvents,
  filterTraceEvents,
  formatTraceClock,
  formatTraceDuration,
  summarizeTraceEvents,
  traceArgs,
  traceFirstLine,
  traceOneLine,
  traceText,
  traceTimestamp,
  traceToolCall,
} = await jiti.import("./trace-events.ts");

// --- fixtures ---------------------------------------------------------------

const entry = (overrides) => ({
  type: "message",
  id: "e1",
  parentId: null,
  timestamp: "2026-10-31T06:55:26.345Z",
  ...overrides,
});

const user = (id, text, timestamp = "2026-10-31T06:55:26.345Z") =>
  entry({ id, timestamp, message: { role: "user", content: text, timestamp: Date.parse(timestamp) } });

const assistant = (id, content, extra = {}) =>
  entry({
    id,
    message: {
      role: "assistant",
      content,
      model: "deepseek-flash",
      provider: "deepseek",
      timestamp: 1793429726346,
      durationMs: 1392,
      ...extra,
    },
  });

const toolResult = (id, toolCallId, text, extra = {}) =>
  entry({
    id,
    message: {
      role: "toolResult",
      toolCallId,
      toolName: "bash",
      content: [{ type: "text", text }],
      isError: false,
      durationMs: 27,
      ...extra,
    },
  });

const call = (id, name, input) => ({ type: "toolCall", toolCallId: id, toolName: name, input });

// --- text helpers -----------------------------------------------------------

test("traceText reads text blocks and ignores thinking and images", () => {
  assert.equal(traceText("plain"), "plain");
  assert.equal(
    traceText([
      { type: "text", text: "a" },
      { type: "thinking", thinking: "hidden" },
      { type: "image", source: { type: "base64", data: "..." } },
      { type: "text", text: "b" },
    ]),
    "a\nb",
  );
  assert.equal(traceText(null), "");
  assert.equal(traceText(undefined), "");
  assert.equal(traceText([{ type: "text", text: "" }]), "");
});

test("traceOneLine collapses whitespace and traceFirstLine skips blank lines", () => {
  assert.equal(traceOneLine("  a\n\t b  "), "a b");
  assert.equal(traceFirstLine("\n\n   first  \nsecond"), "first");
  assert.equal(traceFirstLine("   "), "");
  assert.equal(traceOneLine(""), "");
});

// --- timestamps -------------------------------------------------------------

test("traceTimestamp prefers the message clock and falls back to the entry's ISO stamp", () => {
  assert.equal(
    traceTimestamp({ timestamp: "2026-10-31T06:55:26.345Z" }, { timestamp: 1793429726346 }),
    1793429726346,
  );
  assert.equal(
    traceTimestamp({ timestamp: "2026-10-31T06:55:26.345Z" }, { timestamp: undefined }),
    Date.parse("2026-10-31T06:55:26.345Z"),
  );
  assert.equal(traceTimestamp({}, null), null);
  assert.equal(traceTimestamp({ timestamp: "not a date" }, null), null);
});

// --- tool call normalisation ------------------------------------------------

test("traceToolCall accepts both pi spellings", () => {
  assert.deepEqual(traceToolCall(call("c1", "bash", { command: "ls" })), {
    id: "c1",
    name: "bash",
    input: { command: "ls" },
  });
  assert.deepEqual(
    traceToolCall({ type: "toolCall", id: "c2", name: "read", arguments: { path: "a" } }),
    { id: "c2", name: "read", input: { path: "a" } },
  );
  assert.equal(traceToolCall({ type: "text", text: "no" }), null);
  assert.equal(traceToolCall({ type: "toolCall", toolCallId: "c3" }), null);
  assert.deepEqual(traceToolCall({ type: "toolCall", toolCallId: "c4", toolName: "ls" }).input, {});
});

test("traceArgs serializes arguments on one line and survives cycles", () => {
  assert.equal(traceArgs({ id: "c", name: "bash", input: { command: "ls -la" } }), '{"command":"ls -la"}');
  const cyclic = { id: "c", name: "x", input: {} };
  cyclic.input.self = cyclic.input;
  assert.equal(traceArgs(cyclic), "");
});

// --- durations --------------------------------------------------------------

test("formatTraceDuration picks a readable unit", () => {
  assert.equal(formatTraceDuration(0), "0ms");
  assert.equal(formatTraceDuration(27), "27ms");
  assert.equal(formatTraceDuration(999), "999ms");
  assert.equal(formatTraceDuration(1392), "1.4s");
  assert.equal(formatTraceDuration(59_000), "59s");
  assert.equal(formatTraceDuration(60_000), "1m 00s");
  assert.equal(formatTraceDuration(123_400), "2m 03s");
  assert.equal(formatTraceDuration(null), "");
  assert.equal(formatTraceDuration(undefined), "");
  assert.equal(formatTraceDuration(-1), "");
  assert.equal(formatTraceDuration(Number.NaN), "");
});

test("formatTraceClock renders millisecond precision and blanks bad input", () => {
  const stamp = new Date(2026, 10, 5, 9, 22, 39, 909).getTime();
  assert.equal(formatTraceClock(stamp), "09:22:39.909");
  assert.equal(formatTraceClock(null), "");
  assert.equal(formatTraceClock(Number.NaN), "");
});

// --- row building -----------------------------------------------------------

test("buildTraceEvents numbers turns from user messages", () => {
  const events = buildTraceEvents([
    user("u1", "first"),
    assistant("a1", [{ type: "text", text: "answer one" }]),
    user("u2", "second"),
    assistant("a2", [{ type: "text", text: "answer two" }]),
  ]);
  assert.deepEqual(
    events.map((event) => [event.lane, event.turn, event.badge]),
    [
      ["input", 1, "user"],
      ["model", 1, "assistant"],
      ["input", 2, "user"],
      ["model", 2, "assistant"],
    ],
  );
});

test("buildTraceEvents splits one assistant message into reply, thinking and call rows", () => {
  const events = buildTraceEvents([
    user("u1", "go"),
    assistant("a1", [
      { type: "thinking", thinking: "I should look\nat the build" },
      { type: "text", text: "Looking now.\nMore detail." },
      call("c1", "bash", { command: "ls" }),
      call("c2", "read", { path: "a.ts" }),
    ]),
    toolResult("r1", "c1", "file-a\nfile-b"),
    toolResult("r2", "c2", "contents"),
  ]);
  assert.deepEqual(
    events.map((event) => event.badge),
    ["user", "assistant", "thinking", "tool", "tool"],
  );
  const reply = events[1];
  assert.equal(reply.text, "Looking now.");
  assert.equal(events[2].text, "I should look");
  const bash = events[3];
  assert.equal(bash.title, "bash");
  assert.equal(bash.args, '{"command":"ls"}');
  assert.equal(bash.result, "file-a");
  assert.equal(bash.durationMs, 27);
  assert.equal(bash.status, "ok");
});

test("buildTraceEvents folds results into calls and never emits result rows twice", () => {
  const events = buildTraceEvents([
    user("u1", "go"),
    assistant("a1", [call("c1", "bash", { command: "ls" })]),
    toolResult("r1", "c1", "ok"),
  ]);
  const calls = events.filter((event) => event.lane === "tool");
  assert.equal(calls.length, 1);
  assert.ok(events.every((event) => event.kind !== "toolResult"));
});

test("buildTraceEvents keeps a result row when its call is not on the page", () => {
  const events = buildTraceEvents([
    assistant("a1", [call("c1", "bash", { command: "ls" })]),
    toolResult("r1", "c1", "ok"),
    // A second result whose call was on an earlier page.
    toolResult("r2", "c-elsewhere", "orphan output"),
  ]);
  assert.deepEqual(
    events.map((event) => event.key),
    ["a1#assistant", "a1#call-0", "r2#orphan"],
  );
  const orphan = events.at(-1);
  assert.equal(orphan.result, "orphan output");
  assert.equal(orphan.toolResult.toolCallId, "c-elsewhere");
});

test("a call without a result is running while the session runs and missing afterwards", () => {
  const entries = [user("u1", "go"), assistant("a1", [call("c1", "bash", { command: "sleep 99" })])];
  assert.equal(buildTraceEvents(entries, { running: true }).find((event) => event.lane === "tool").status, "running");
  assert.equal(buildTraceEvents(entries, { running: false }).find((event) => event.lane === "tool").status, "missing");
});

test("assistant rows fall back to a placeholder when there is no text", () => {
  const withCalls = buildTraceEvents([assistant("a1", [call("c1", "bash", { command: "ls" })])]);
  assert.equal(withCalls[0].text, "(仅工具调用)");
  const empty = buildTraceEvents([assistant("a1", [])]);
  assert.equal(empty[0].text, "(空回复)");
});

test("an errored result and an aborted reply carry their status", () => {
  const events = buildTraceEvents([
    assistant("a1", [call("c1", "bash", { command: "exit 1" })]),
    toolResult("r1", "c1", "boom", { isError: true }),
    assistant("a2", [{ type: "text", text: "stopped" }], { stopReason: "aborted" }),
    assistant("a3", [{ type: "text", text: "failed" }], { errorMessage: "rate limited" }),
  ]);
  const tool = events.find((event) => event.lane === "tool");
  assert.equal(tool.status, "error");
  assert.equal(tool.isError, true);
  const aborted = events.find((event) => event.entryId === "a2");
  const failed = events.find((event) => event.entryId === "a3");
  assert.equal(aborted.status, "aborted");
  assert.equal(failed.status, "error");
  assert.equal(failed.isError, true);
});

test("a subagent result marks its call as nested", () => {
  const events = buildTraceEvents([
    assistant("a1", [call("c1", "task", { prompt: "go" })]),
    toolResult("r1", "c1", "done", {
      details: { kind: "pi-star-subagent", sessionId: "s-1", profile: "default" },
    }),
  ]);
  assert.equal(events.find((event) => event.lane === "tool").nested, true);
});

test("buildTraceEvents turns metadata entries into meta rows", () => {
  const events = buildTraceEvents([
    { type: "model_change", id: "m1", timestamp: "2026-10-30T01:59:18.140Z", provider: "deepseek", modelId: "deepseek-flash" },
    { type: "thinking_level_change", id: "t1", timestamp: "2026-10-30T01:59:18.140Z", thinkingLevel: "high" },
    { type: "compaction", id: "c1", timestamp: "2026-10-30T02:37:27.595Z", summary: "## Goal\n- do the thing" },
    { type: "session_info", id: "s1", timestamp: "2026-10-31T04:01:46.573Z", name: "rename me" },
    { type: "custom", id: "x1", timestamp: "2026-10-30T01:59:18.117Z", customType: "pi-star:tool-selection", data: {} },
    { type: "branch_summary", id: "b1", timestamp: "2026-10-31T04:01:46.573Z", summary: "explored" },
  ]);
  assert.deepEqual(
    events.map((event) => [event.lane, event.badge, event.text]),
    [
      ["meta", "model", "deepseek/deepseek-flash"],
      ["meta", "thinking_level", "high"],
      ["meta", "compaction", "## Goal"],
      ["meta", "session_info", "rename me"],
      ["meta", "custom", "pi-star:tool-selection"],
      ["meta", "branch_summary", "explored"],
    ],
  );
});

test("meta rows are skipped when the entry carries nothing to show", () => {
  const events = buildTraceEvents([
    { type: "model_change", id: "m1" },
    { type: "thinking_level_change", id: "t1" },
    { type: "session_info", id: "s1" },
  ]);
  assert.deepEqual(events, []);
});

test("custom and bashExecution messages become single model rows", () => {
  const events = buildTraceEvents([
    entry({ id: "c1", message: { role: "custom", customType: "note", content: "hello", display: true } }),
    entry({ id: "b1", message: { role: "bashExecution", command: "ls -la", output: "x" } }),
  ]);
  assert.deepEqual(
    events.map((event) => [event.lane, event.badge]),
    [
      ["model", "custom"],
      ["model", "bashExecution"],
    ],
  );
});

test("buildTraceEvents ignores the session header and malformed lines", () => {
  const events = buildTraceEvents([
    { type: "session", id: "header" },
    { type: "message", id: "broken" },
    {},
  ]);
  assert.deepEqual(events, []);
});

test("keys stay unique when one message yields several rows", () => {
  const events = buildTraceEvents([
    assistant("a1", [
      call("c1", "bash", { command: "a" }),
      call("c2", "bash", { command: "b" }),
      { type: "thinking", thinking: "hmm" },
    ]),
  ]);
  const keys = events.map((event) => event.key);
  assert.equal(new Set(keys).size, keys.length);
});

// --- filters and counts -----------------------------------------------------

test("filterTraceEvents drops tool rows and nested calls without renumbering turns", () => {
  const events = buildTraceEvents([
    user("u1", "go"),
    assistant("a1", [call("c1", "bash", { command: "ls" }), call("c2", "task", { prompt: "go" })]),
    toolResult("r1", "c1", "ok"),
    toolResult("r2", "c2", "done", { details: { kind: "pi-star-subagent", sessionId: "s" } }),
    user("u2", "again"),
    assistant("a2", [{ type: "text", text: "fine" }]),
  ]);
  const noTools = filterTraceEvents(events, { ...TRACE_FILTER_ALL, tools: false });
  assert.ok(noTools.every((event) => event.lane !== "tool"));
  assert.deepEqual([...new Set(noTools.map((event) => event.turn))], [1, 2]);

  const noSubcalls = filterTraceEvents(events, { ...TRACE_FILTER_ALL, subcalls: false });
  assert.equal(noSubcalls.filter((event) => event.lane === "tool").length, 1);
});

test("summarizeTraceEvents counts turns, calls, errors and tool time", () => {
  const events = buildTraceEvents([
    user("u1", "go"),
    assistant("a1", [call("c1", "bash", { command: "ls" })]),
    toolResult("r1", "c1", "ok", { durationMs: 30 }),
    assistant("a2", [call("c2", "bash", { command: "false" })]),
    toolResult("r2", "c2", "no", { isError: true, durationMs: 12 }),
  ]);
  const counts = summarizeTraceEvents(events);
  assert.equal(counts.turns, 1);
  assert.equal(counts.calls, 2);
  assert.equal(counts.errors, 1);
  assert.equal(counts.totalDurationMs, 42);
  assert.equal(counts.subcalls, 0);
});