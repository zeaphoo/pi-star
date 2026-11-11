import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url, { tsconfigPaths: true });
const {
  DOCK_MAX_LEAVES,
  DOCK_MAX_TERMINAL_LEAVES,
  addDockView,
  canAddDockView,
  closeDockLeaf,
  countDockView,
  dockLeaves,
  dockLeafCount,
  emptyDockState,
  ensureDockLeafState,
  focusDockLeaf,
  resizeDockSplit,
  sanitizeDockState,
  setDockLeafCode,
  splitDockLeaf,
} = await jiti.import("./dock-state.ts");
const { openCodeFileTab } = await jiti.import("./code-workspace.ts");

const context = { projectKey: "/repo", root: "/repo" };

function withViews(...views) {
  return views.reduce(
    (state, view) => addDockView(state, view, "row", context),
    emptyDockState(),
  );
}

test("an empty dock has no layout and no per-leaf state", () => {
  const state = emptyDockState();
  assert.equal(state.layout.root, null);
  assert.equal(state.layout.focusedLeafId, null);
  assert.deepEqual(dockLeaves(state.layout), []);
  assert.equal(dockLeafCount(state.layout), 0);
});

test("the first view becomes the root, and new views split the focused leaf", () => {
  const withCode = addDockView(emptyDockState(), "code", "row", context);
  assert.equal(dockLeafCount(withCode.layout), 1);
  const [codeLeaf] = dockLeaves(withCode.layout);
  assert.equal(codeLeaf.view, "code");
  assert.equal(withCode.layout.focusedLeafId, codeLeaf.id);
  assert.ok(withCode.code[codeLeaf.id], "the leaf gets its own code workspace");

  const withTerminal = addDockView(withCode, "terminal", "row", context);
  assert.equal(dockLeafCount(withTerminal.layout), 2);
  assert.equal(withTerminal.layout.root.kind, "split");
  assert.equal(withTerminal.layout.root.direction, "row");
  const [terminalLeaf] = dockLeaves(withTerminal.layout).filter((leaf) => leaf.view === "terminal");
  assert.ok(withTerminal.terminal[terminalLeaf.id], "its own terminal workspace, not the main view's");
  assert.equal(withTerminal.layout.focusedLeafId, terminalLeaf.id);
});

test("a split running the same way gains a sibling instead of nesting", () => {
  const state = withViews("code", "terminal");
  const next = addDockView(state, "diff", "row", context);
  assert.equal(dockLeafCount(next.layout), 3);
  assert.equal(next.layout.root.kind, "split");
  assert.equal(next.layout.root.children.length, 3, "one row of three, not a split inside a split");
  assert.deepEqual(next.layout.root.sizes.map((size) => Math.round(size * 100)), [50, 25, 25]);
});

test("splitting a leaf adds a view of the same kind, in the asked direction", () => {
  const state = withViews("terminal");
  const leaf = dockLeaves(state.layout)[0];
  const next = splitDockLeaf(state, leaf.id, "column", context);
  assert.equal(dockLeafCount(next.layout), 2);
  assert.equal(countDockView(next.layout, "terminal"), 2);
  assert.equal(next.layout.root.direction, "column");
  assert.notEqual(next.layout.focusedLeafId, leaf.id, "the new leaf takes focus");
});

test("the leaf and terminal budgets hold", () => {
  let state = emptyDockState();
  state = addDockView(state, "terminal", "row", context);
  state = addDockView(state, "terminal", "row", context);
  assert.equal(countDockView(state.layout, "terminal"), DOCK_MAX_TERMINAL_LEAVES);
  assert.equal(canAddDockView(state, "terminal"), false);
  assert.equal(addDockView(state, "terminal", "row", context), state, "refused, not replaced");

  state = addDockView(state, "code", "row", context);
  state = addDockView(state, "diff", "row", context);
  assert.equal(dockLeafCount(state.layout), DOCK_MAX_LEAVES);
  assert.equal(canAddDockView(state, "code"), false);
  assert.equal(addDockView(state, "code", "row", context), state);
});

test("closing a leaf drops its state and hands focus to a neighbour", () => {
  const state = withViews("code", "terminal", "diff");
  const leaves = dockLeaves(state.layout);
  const middle = leaves[1];
  const { state: next, killedTerminalIds } = closeDockLeaf(state, middle.id);
  assert.deepEqual(killedTerminalIds, [], "an empty terminal workspace owns no shell yet");
  assert.equal(dockLeafCount(next.layout), 2);
  assert.equal(next.terminal[middle.id], undefined);
  assert.ok(next.layout.focusedLeafId);
  assert.notEqual(next.layout.focusedLeafId, middle.id);
});

test("closing a terminal leaf reports the shells it owned", () => {
  const state = withViews("terminal");
  const leaf = dockLeaves(state.layout)[0];
  const paneId = "0123456789abcdef0123456789abcdef";
  const withPane = {
    ...state,
    terminal: {
      [leaf.id]: {
        projectKey: "/repo",
        tabs: [{
          id: "tab",
          cwd: "/repo",
          root: { kind: "pane", id: "pane", terminalId: paneId, cwd: "/repo" },
          activePaneId: "pane",
        }],
        activeTabId: "tab",
      },
    },
  };
  const { killedTerminalIds } = closeDockLeaf(withPane, leaf.id);
  assert.deepEqual(killedTerminalIds, [paneId]);
});

test("focus and resize only touch the layout", () => {
  const state = withViews("code", "terminal");
  const [first, second] = dockLeaves(state.layout);
  const focused = focusDockLeaf(state, first.id);
  assert.equal(focused.layout.focusedLeafId, first.id);
  assert.equal(focusDockLeaf(focused, first.id), focused, "no-op keeps the object");
  assert.equal(focusDockLeaf(state, "missing"), state);

  const root = state.layout.root;
  assert.equal(root.kind, "split");
  const resized = resizeDockSplit(state, root.id, [3, 1]);
  assert.equal(resized.layout.root.kind, "split");
  assert.deepEqual(resized.layout.root.sizes, [0.75, 0.25]);
  assert.equal(dockLeaves(resized.layout)[1].id, second.id);
});

test("a leaf's code workspace is its own, never the main view's", () => {
  const state = withViews("code");
  const leaf = dockLeaves(state.layout)[0];
  const next = setDockLeafCode(state, leaf.id, openCodeFileTab(state.code[leaf.id], "/repo/a.ts", "a.ts"));
  assert.equal(next.code[leaf.id].tabs.length, 1);
  assert.equal(next.code[leaf.id].activeTabId, "code:/repo/a.ts");
  assert.equal(state.code[leaf.id].tabs.length, 0, "the input state is untouched");
  assert.equal(setDockLeafCode(state, "missing", state.code[leaf.id]), state);
});

test("a restored layout without leaf state gets an empty workspace", () => {
  const state = withViews("code", "terminal");
  const bare = { ...state, code: {}, terminal: {} };
  const ensured = ensureDockLeafState(bare, context);
  assert.equal(ensured === bare, false);
  for (const leaf of dockLeaves(ensured.layout)) {
    if (leaf.view === "code") assert.ok(ensured.code[leaf.id]);
    if (leaf.view === "terminal") assert.ok(ensured.terminal[leaf.id]);
  }
  assert.equal(ensureDockLeafState(ensured, context), ensured, "nothing missing: same object");
});

test("storage that names a dock: unknown views drop, budgets hold, sizes normalize", () => {
  const stored = {
    layout: {
      focusedLeafId: "leaf-terminal",
      root: {
        kind: "split",
        id: "split-1",
        direction: "column",
        sizes: [4, 2, 0],
        children: [
          { kind: "leaf", id: "leaf-code", view: "code" },
          { kind: "leaf", id: "leaf-terminal", view: "terminal" },
          { kind: "leaf", id: "leaf-bogus", view: "bogus" },
          { kind: "leaf", id: "leaf-code", view: "code" },
        ],
      },
    },
    code: { "leaf-code": { projectKey: "/repo", root: "/repo", tabs: [{ id: "code:/repo/a.ts", filePath: "/repo/a.ts" }], activeTabId: "code:/repo/a.ts" } },
    terminal: {},
  };
  const state = sanitizeDockState(stored, "/repo");
  assert.ok(state);
  assert.equal(dockLeafCount(state.layout), 2, "an unknown view and a duplicate id are dropped");
  assert.deepEqual(dockLeaves(state.layout).map((leaf) => leaf.view), ["code", "terminal"]);
  assert.equal(state.layout.root.kind, "split");
  assert.deepEqual(state.layout.root.sizes, [2 / 3, 1 / 3]);
  assert.ok(state.code["leaf-code"]);
  assert.equal(state.layout.focusedLeafId, "leaf-terminal");

  assert.equal(sanitizeDockState({ layout: { root: null } }, "/repo").layout.root, null);
  assert.equal(sanitizeDockState("nonsense", "/repo"), null);
});

test("a stored split with one surviving child collapses into that child", () => {
  const state = sanitizeDockState({
    layout: {
      root: {
        kind: "split",
        id: "split-1",
        direction: "row",
        children: [{ kind: "leaf", id: "only", view: "diff" }],
      },
    },
  }, "/repo");
  assert.equal(state.layout.root.kind, "leaf");
  assert.equal(state.layout.root.view, "diff");
  assert.equal(state.layout.focusedLeafId, "only");
});