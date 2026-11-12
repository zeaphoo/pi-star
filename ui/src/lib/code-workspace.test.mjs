import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const {
  closeAllCodeTabs,
  closeOtherCodeTabs,
  closeCodeTabsToRight,
} = await createJiti(import.meta.url, { tsconfigPaths: true }).import("./code-workspace.ts");

const tab = (id) => ({ id, label: id, filePath: `/p/${id}` });
const state = {
  projectKey: "p",
  root: "/p",
  tabs: [tab("a"), tab("b"), tab("c"), tab("d")],
  activeTabId: "b",
};

test("close all keeps the workspace but drops every tab", () => {
  assert.deepEqual(closeAllCodeTabs(state), { ...state, tabs: [], activeTabId: null });
  const empty = { ...state, tabs: [], activeTabId: null };
  assert.equal(closeAllCodeTabs(empty), empty);
});

test("close others keeps the selected tab active", () => {
  const next = closeOtherCodeTabs(state, "c");
  assert.deepEqual(next.tabs.map((item) => item.id), ["c"]);
  assert.equal(next.activeTabId, "c");
  assert.equal(closeOtherCodeTabs({ ...state, tabs: [tab("a")] }, "a").tabs.length, 1);
  assert.equal(closeOtherCodeTabs(state, "missing"), state);
});

test("close to the right cuts after the active tab", () => {
  const next = closeCodeTabsToRight(state, "b");
  assert.deepEqual(next.tabs.map((item) => item.id), ["a", "b"]);
  assert.equal(next.activeTabId, "b");
  assert.equal(closeCodeTabsToRight(state, "d"), state);
  assert.equal(closeCodeTabsToRight(state, "missing"), state);
});
