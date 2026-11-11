import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url);
const {
  closeTerminalPane,
  countTerminalPanes,
  newTerminalWorkspaceTab,
  replaceTerminalPaneId,
  setTerminalActivePane,
  setTerminalPaneCwd,
  setTerminalPaneRestored,
  splitTerminalPane,
  resizeTerminalSplit,
} = await jiti.import("./terminal-workspace.ts");

function splitRow(tab) {
  return splitTerminalPane(tab, tab.activePaneId, "row");
}

test("a new terminal tab starts with one pane", () => {
  const tab = newTerminalWorkspaceTab("/repo");
  assert.equal(countTerminalPanes(tab.root), 1);
  assert.equal(tab.activePaneId, tab.root.id);
});

test("splitting replaces the active pane and focuses the new pane", () => {
  const tab = newTerminalWorkspaceTab("/repo");
  const split = splitRow(tab);
  assert.equal(countTerminalPanes(split.root), 2);
  assert.equal(split.root.kind, "split");
  assert.notEqual(split.activePaneId, tab.activePaneId);
  assert.equal(split.root.sizes.length, 2);
});

test("closing one side collapses a two-pane split", () => {
  const tab = newTerminalWorkspaceTab("/repo");
  const split = splitRow(tab);
  const first = split.root.kind === "split" ? split.root.children[0] : null;
  assert.ok(first);
  const closed = closeTerminalPane(split, first.id);
  assert.ok(closed);
  assert.equal(closed.root.kind, "pane");
  assert.equal(countTerminalPanes(closed.root), 1);
});

test("closing the last pane closes the tab", () => {
  const tab = newTerminalWorkspaceTab("/repo");
  assert.equal(closeTerminalPane(tab, tab.activePaneId), null);
});

test("a pane can be given a fresh terminal id for restart", () => {
  const tab = newTerminalWorkspaceTab("/repo");
  const before = tab.root.terminalId;
  const next = replaceTerminalPaneId(tab, tab.root.id);
  assert.notEqual(next.root.terminalId, before);
  assert.equal(next.root.restored, false);
});

test("split sizes are normalized", () => {
  const tab = splitRow(newTerminalWorkspaceTab("/repo"));
  assert.equal(tab.root.kind, "split");
  const resized = resizeTerminalSplit(tab.root, tab.root.id, [3, 1]);
  assert.equal(resized.kind, "split");
  assert.deepEqual(resized.sizes, [0.75, 0.25]);
});

test("activating a missing pane is a no-op", () => {
  const tab = newTerminalWorkspaceTab("/repo");
  assert.equal(setTerminalActivePane(tab, "missing"), tab);
});

test("a started pane is marked so a remount reattaches", () => {
  const tab = newTerminalWorkspaceTab("/repo");
  assert.equal(tab.root.restored, undefined);
  const started = setTerminalPaneRestored(tab, tab.root.id, true);
  assert.equal(started.root.restored, true);
  assert.equal(setTerminalPaneRestored(started, started.root.id, true), started);
});

test("a pane keeps the directory its shell moved to", () => {
  const tab = splitRow(newTerminalWorkspaceTab("/repo"));
  const first = tab.root.kind === "split" ? tab.root.children[0] : null;
  assert.ok(first);
  const inactive = first.id === tab.activePaneId
    ? (tab.root.kind === "split" ? tab.root.children[1] : null)
    : first;
  assert.ok(inactive);

  const moved = setTerminalPaneCwd(tab, inactive.id, "/repo/src");
  const movedPane = moved.root.kind === "split"
    ? moved.root.children.find((child) => child.id === inactive.id)
    : null;
  assert.equal(movedPane?.cwd, "/repo/src");
  assert.equal(moved.cwd, tab.cwd, "an inactive pane does not rename the tab");

  const active = setTerminalPaneCwd(moved, moved.activePaneId, "/repo/test");
  assert.equal(active.cwd, "/repo/test");
  assert.equal(setTerminalPaneCwd(active, active.activePaneId, "/repo/test"), active);
});
