import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

test("a subagent viewer offers its execution trace beside the transcript", async () => {
  const viewer = await readFile(new URL("./SubagentViewer.tsx", import.meta.url), "utf8");
  const trace = await readFile(new URL("./SubagentTrace.tsx", import.meta.url), "utf8");
  assert.match(viewer, /SubagentTrace/);
  assert.match(viewer, /subagent\.tab\.trace/);
  // The trace is read-only: it must not subscribe to the agent event stream.
  assert.doesNotMatch(trace, /agent:/);
  assert.match(trace, /\/entries\?/);
  assert.match(trace, /buildTraceEvents/);
});
