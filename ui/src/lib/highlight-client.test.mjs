import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url, { moduleCache: false });
const { highlightSegments, cachedHighlight } = await jiti.import("./highlight-client.ts");

// No Worker in this process, so the client falls back to the main-thread
// implementation; that is the path a browser without workers takes too.
test("a result is cached per language and segment text", async () => {
  const segments = ["const a = 1;"];
  assert.equal(cachedHighlight(segments, "javascript"), null, "nothing cached yet");
  const lines = await highlightSegments(segments, "javascript");
  assert.equal(lines.length, 1);
  assert.match(lines[0][0], /token keyword/);
  // The same input is served from the cache, the same array instance.
  assert.equal(cachedHighlight(segments, "javascript"), lines);
  assert.equal(await highlightSegments(segments, "javascript"), lines);
  // A different language or text is a different entry.
  assert.equal(cachedHighlight(segments, "typescript"), null);
  assert.equal(cachedHighlight(["const a = 2;"], "javascript"), null);
});

test("segments are returned in order, one array per segment", async () => {
  const lines = await highlightSegments(["let a = 1;", "let b = 2;"], "typescript");
  assert.equal(lines.length, 2);
  assert.match(lines[0][0], /token keyword/);
  assert.match(lines[1][0], /token keyword/);
});
