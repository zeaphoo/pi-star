import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url, { tsconfigPaths: true });
const {
  collectSessionArtifacts,
  loadSessionArtifacts,
  mergeSessionArtifacts,
  summarizeArtifactEvents,
} = await jiti.import("./session-artifacts.ts");

const CWD = "/work/app";

function call(toolCallId, toolName, input) {
  return { type: "toolCall", toolCallId, toolName, input };
}

function assistant(toolCalls, timestamp = 1_000) {
  return { role: "assistant", content: toolCalls, model: "m", provider: "p", timestamp };
}

function result(toolCallId, toolName, { timestamp = 2_000, isError = false, details } = {}) {
  return { role: "toolResult", toolCallId, toolName, content: [], isError, details, timestamp };
}

test("artifacts are the files a session wrote, newest first, deletes and failures left out", () => {
  const messages = [
    assistant([
      call("w1", "write", { file_path: "src/a.ts" }),
      call("e1", "edit", { file_path: "src/b.ts" }),
      call("p1", "apply_patch", { input: "*** Begin Patch\n*** Update File: src/c.ts\n*** End Patch" }),
      call("w2", "write", { file_path: "src/broken.ts" }),
      call("r1", "read", { file_path: "src/read-only.ts" }),
      call("w3", "write", { file_path: "src/a.ts" }),
    ], 900),
  ];
  messages.push(
    result("w1", "write", { timestamp: 1_000 }),
    result("e1", "edit", { timestamp: 3_000 }),
    result("p1", "apply_patch", { timestamp: 2_000, details: { result: { appliedFiles: ["/work/app/src/c.ts"] } } }),
    result("w2", "write", { isError: true, timestamp: 4_000 }),
    result("r1", "read", { timestamp: 5_000 }),
    result("w3", "write", { timestamp: 6_000 }),
  );
  const toolResults = new Map(messages.filter((message) => message.role === "toolResult").map((message) => [message.toolCallId, message]));
  assert.deepEqual(collectSessionArtifacts(messages, toolResults, CWD), [
    { filePath: "/work/app/src/a.ts", relativePath: "src/a.ts", lastWriteAt: 6_000, writes: 2 },
    { filePath: "/work/app/src/b.ts", relativePath: "src/b.ts", lastWriteAt: 3_000, writes: 1 },
    { filePath: "/work/app/src/c.ts", relativePath: "src/c.ts", lastWriteAt: 2_000, writes: 1 },
  ]);
});

test("a plain delete is not an artifact, and a apply_patch that reports no applied file falls back to its document", () => {
  const messages = [
    assistant([
      call("d1", "apply_patch", { input: "*** Begin Patch\n*** Delete File: src/old.ts\n*** End Patch" }),
      call("p2", "apply_patch", { input: "*** Begin Patch\n*** Add File: src/new.ts\n+hi\n*** End Patch" }),
    ]),
  ];
  messages.push(
    result("d1", "apply_patch", { timestamp: 1_000, details: { result: { appliedFiles: ["/work/app/src/old.ts"] } } }),
    result("p2", "apply_patch", { timestamp: 2_000 }),
  );
  const toolResults = new Map(messages.filter((message) => message.role === "toolResult").map((message) => [message.toolCallId, message]));
  assert.deepEqual(collectSessionArtifacts(messages, toolResults, CWD).map((artifact) => artifact.relativePath), ["src/new.ts"]);
});

test("merging a live list with a history scan never double-counts an overlap", () => {
  const merged = mergeSessionArtifacts(
    [{ filePath: "/work/app/a.ts", relativePath: "a.ts", lastWriteAt: 1_000, writes: 1 }],
    [
      { filePath: "/work/app/a.ts", relativePath: "a.ts", lastWriteAt: 2_000, writes: 3 },
      { filePath: "/work/app/b.ts", relativePath: "b.ts", lastWriteAt: 500, writes: 1 },
    ],
  );
  assert.deepEqual(merged.map((artifact) => [artifact.relativePath, artifact.lastWriteAt, artifact.writes]), [
    ["a.ts", 2_000, 3],
    ["b.ts", 500, 1],
  ]);
});

test("summary keeps first-seen order for equal times and ignores unknown timestamps", () => {
  const artifacts = summarizeArtifactEvents([
    { filePath: "/work/app/z.ts" },
    { filePath: "/work/app/a.ts" },
    { filePath: "/work/app/z.ts", timestamp: 10 },
  ], CWD);
  assert.deepEqual(artifacts.map((artifact) => [artifact.relativePath, artifact.lastWriteAt, artifact.writes]), [
    ["z.ts", 10, 2],
    ["a.ts", undefined, 1],
  ]);
});

test("the history scan pages the whole transcript and runs the same write rules", async () => {
  const realFetch = globalThis.fetch;
  const calls = [];
  globalThis.fetch = async (url) => {
    calls.push(String(url));
    const older = !String(url).includes("before=");
    const page = older
      ? {
          context: {
            messages: [
              assistant([call("old-write", "write", { file_path: "docs/old.md" })], 100),
              result("old-write", "write", { timestamp: 200 }),
            ],
            oldestEntryId: "entry-1",
            hasMore: true,
          },
        }
      : {
          context: {
            messages: [
              assistant([call("new-write", "write", { file_path: "src/new.ts" })], 300),
              result("new-write", "write", { timestamp: 400 }),
            ],
            oldestEntryId: null,
            hasMore: false,
          },
        };
    return { ok: true, json: async () => page };
  };
  try {
    const artifacts = await loadSessionArtifacts({ sessionId: "session-1", cwd: CWD });
    assert.deepEqual(artifacts.map((artifact) => artifact.relativePath), ["src/new.ts", "docs/old.md"]);
    assert.equal(calls.length, 2);
    assert.match(calls[0], /tail=1000/);
    assert.match(calls[1], /before=entry-1/);
  } finally {
    globalThis.fetch = realFetch;
  }
});

test("the scan stops at the page cap without throwing away what it read", async () => {
  const realFetch = globalThis.fetch;
  globalThis.fetch = async () => ({
    ok: true,
    json: async () => ({
      context: {
        messages: [assistant([call("w", "write", { file_path: "a.ts" })]), result("w", "write")],
        oldestEntryId: "same",
        hasMore: true,
      },
    }),
  });
  try {
    const artifacts = await loadSessionArtifacts({ sessionId: "s", cwd: CWD, maxPages: 2 });
    assert.deepEqual(artifacts.map((artifact) => artifact.relativePath), ["a.ts"]);
  } finally {
    globalThis.fetch = realFetch;
  }
});
