import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const { buildDiffHighlightPlan, diffLineHtml } = await createJiti(import.meta.url).import("./diff-highlight.ts");

const context = (n, text) => ({ type: "context", text, oldLineNo: n, newLineNo: n });

test("each hunk side becomes one contiguous segment, gaps included", () => {
  const lines = [
    context(1, "a"),
    { type: "removed", text: "b-old", oldLineNo: 2, newLineNo: null },
    { type: "added", text: "b-new", oldLineNo: null, newLineNo: 2 },
    context(3, "c"),
    { type: "gap", count: 40 },
    context(44, "d"),
  ];
  const plan = buildDiffHighlightPlan(lines);
  // Old side of the first hunk then its new side, and the second hunk's sides
  // after the gap; nothing crosses the gap.
  assert.deepEqual(plan.segments, ["a\nb-old\nc", "a\nb-new\nc", "d", "d"]);
  assert.deepEqual(plan.oldAt, [
    { segment: 0, line: 0 },
    { segment: 0, line: 1 },
    null,
    { segment: 0, line: 2 },
    null,
    { segment: 2, line: 0 },
  ]);
  assert.deepEqual(plan.newAt, [
    { segment: 1, line: 0 },
    null,
    { segment: 1, line: 1 },
    { segment: 1, line: 2 },
    null,
    { segment: 3, line: 0 },
  ]);
});

test("a highlighted line reads its own side of the segment", () => {
  const lines = [
    { type: "removed", text: "old", oldLineNo: 1, newLineNo: null },
    { type: "added", text: "new", oldLineNo: null, newLineNo: 1 },
    context(2, "same"),
  ];
  const plan = buildDiffHighlightPlan(lines);
  const highlighted = [["<i>old</i>"], ["<i>new</i>", "<i>same</i>"], ["<i>same</i>"]];
  assert.equal(diffLineHtml(lines[0], 0, plan, highlighted), "<i>old</i>");
  assert.equal(diffLineHtml(lines[1], 1, plan, highlighted), "<i>new</i>");
  // A context line exists on both sides; either copy renders the same text.
  assert.equal(diffLineHtml(lines[2], 2, plan, highlighted), "<i>same</i>");
  // Before the worker answers, the caller keeps its plain row.
  assert.equal(diffLineHtml(lines[0], 0, plan, null), null);
  // A gap has no code of its own.
  assert.equal(diffLineHtml({ type: "gap", count: 3 }, 0, plan, highlighted), null);
});
