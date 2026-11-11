import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url);
const {
  collectTreeLeaves,
  countTreeLeaves,
  findTreeNode,
  normalizeSizes,
  removeTreeNode,
  resizeTreeSplit,
  splitTreeLeaf,
} = await jiti.import("./split-tree.ts");

/** A minimal tree: `{ id, kind: "leaf" }` or `{ id, kind: "split", children, sizes }`. */
const ops = {
  isLeaf: (node) => node.kind === "leaf",
  id: (node) => node.id,
  children: (node) => (node.kind === "split" ? node.children : []),
  sizes: (node) => (node.kind === "split" ? node.sizes : []),
  direction: (node) => (node.kind === "split" ? node.direction : "row"),
  makeSplit: (children, direction, sizes) => ({ kind: "split", id: `s${children.length}-${children[0].id}`, direction, children, sizes }),
  withChildren: (node, children, sizes) => ({ ...node, children, sizes }),
};

const leaf = (id) => ({ kind: "leaf", id });
const split = (id, direction, children, sizes = []) => ({
  kind: "split",
  id,
  direction,
  children,
  sizes: sizes.length ? sizes : children.map(() => 1 / children.length),
});

test("sizes are normalized, replacing broken entries", () => {
  assert.deepEqual(normalizeSizes([3, 1], 2), [0.75, 0.25]);
  assert.deepEqual(normalizeSizes([0, -1, Number.NaN], 3), [1 / 3, 1 / 3, 1 / 3]);
  assert.deepEqual(normalizeSizes([2], 3), [0.5, 0.25, 0.25]);
});

test("leaves are found and counted through nesting", () => {
  const tree = split("a", "row", [leaf("x"), split("b", "column", [leaf("y"), leaf("z")])]);
  assert.equal(countTreeLeaves(tree, ops), 3);
  assert.deepEqual(collectTreeLeaves(tree, ops).map((node) => node.id), ["x", "y", "z"]);
  assert.equal(findTreeNode(tree, "z", ops).id, "z");
  assert.equal(findTreeNode(tree, "missing", ops), null);
});

test("splitting a leaf nests by default and merges when asked", () => {
  const tree = split("a", "row", [leaf("x"), leaf("y")]);
  const nested = splitTreeLeaf(tree, "x", "column", leaf("w"), ops);
  assert.equal(nested.children[0].kind, "split");
  assert.equal(countTreeLeaves(nested, ops), 3);

  const merged = splitTreeLeaf(tree, "x", "row", leaf("w"), ops, { mergeSameDirection: true });
  assert.deepEqual(merged.children.map((node) => node.id), ["x", "w", "y"]);
  // The target's share is halved, the others keep theirs.
  assert.deepEqual(merged.sizes, [0.25, 0.25, 0.5]);
});

test("removing a leaf collapses single-child splits and renormalizes", () => {
  const tree = split("a", "row", [leaf("x"), split("b", "column", [leaf("y"), leaf("z")])]);
  const removed = removeTreeNode(tree, "y", ops);
  assert.equal(removed.removed, true);
  assert.equal(removed.node.kind, "split");
  assert.equal(removed.node.children[1].kind, "leaf");
  assert.equal(removed.node.children[1].id, "z");

  const gone = removeTreeNode(tree, "z", ops);
  assert.equal(countTreeLeaves(gone.node, ops), 2);

  const last = removeTreeNode(leaf("only"), "only", ops);
  assert.equal(last.node, null);
  assert.equal(last.removed, true);

  const untouched = removeTreeNode(tree, "missing", ops);
  assert.equal(untouched.removed, false);
  assert.equal(untouched.node, tree);
});

test("resizing a split keeps the rest of the tree identical", () => {
  const other = split("b", "column", [leaf("y"), leaf("z")]);
  const tree = split("a", "row", [leaf("x"), other]);
  const resized = resizeTreeSplit(tree, "b", [3, 1], ops);
  assert.deepEqual(resized.sizes, [0.5, 0.5]);
  assert.deepEqual(resized.children[1].sizes, [0.75, 0.25]);
  assert.equal(resized.children[0], tree.children[0], "untouched leaves keep their identity");
  assert.deepEqual(resizeTreeSplit(tree, "missing", [1, 1], ops).sizes, [0.5, 0.5]);
});