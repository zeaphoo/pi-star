/**
 * Generic split-tree operations shared by the terminal workspace and the right
 * dock. A tree is either a leaf or a split container holding two or more
 * children with relative sizes; both are addressed by a stable id.
 *
 * The operations are pure: the caller keeps ids, payloads and any per-leaf
 * state, the adapter only teaches the tree shape.
 */

export type SplitDirection = "row" | "column";

export interface TreeOps<N> {
  isLeaf(node: N): boolean;
  id(node: N): string;
  /** Children of a split; empty for a leaf. */
  children(node: N): readonly N[];
  /** Relative sizes of a split's children; empty for a leaf. */
  sizes(node: N): readonly number[];
  /** Direction of a split; "row" for a leaf, which is never asked. */
  direction(node: N): SplitDirection;
  makeSplit(children: N[], direction: SplitDirection, sizes: number[]): N;
  /** Replace a split's children/sizes, keeping its id and direction. */
  withChildren(node: N, children: N[], sizes: number[]): N;
}

/** Sizes that sum to 1, one per child, replacing missing or broken entries. */
export function normalizeSizes(sizes: readonly number[], count: number): number[] {
  const values = Array.from({ length: count }, (_, index) => {
    const value = Number(sizes[index]);
    return Number.isFinite(value) && value > 0 ? value : 1;
  });
  const total = values.reduce((sum, value) => sum + value, 0) || 1;
  return values.map((value) => value / total);
}

export function findTreeNode<N>(root: N, id: string, ops: TreeOps<N>): N | null {
  if (ops.id(root) === id) return root;
  if (ops.isLeaf(root)) return null;
  for (const child of ops.children(root)) {
    const found = findTreeNode(child, id, ops);
    if (found) return found;
  }
  return null;
}

export function collectTreeLeaves<N>(root: N, ops: TreeOps<N>): N[] {
  if (ops.isLeaf(root)) return [root];
  return ops.children(root).flatMap((child) => collectTreeLeaves(child, ops));
}

export function countTreeLeaves<N>(root: N, ops: TreeOps<N>): number {
  return ops.isLeaf(root) ? 1 : ops.children(root).reduce((total, child) => total + countTreeLeaves(child, ops), 0);
}

export interface SplitLeafOptions {
  /**
   * Put the sibling next to the target when the enclosing split already runs
   * the requested way, instead of nesting a split inside a split.
   */
  mergeSameDirection?: boolean;
}

/** Replace one leaf with a split holding the old leaf and `sibling`. */
export function splitTreeLeaf<N>(
  root: N,
  leafId: string,
  direction: SplitDirection,
  sibling: N,
  ops: TreeOps<N>,
  options: SplitLeafOptions = {},
): N {
  const merge = options.mergeSameDirection === true;
  const walk = (node: N): N => {
    if (ops.id(node) === leafId) return ops.makeSplit([node, sibling], direction, [0.5, 0.5]);
    if (ops.isLeaf(node)) return node;
    const children = [...ops.children(node)];
    const sizes = [...ops.sizes(node)];
    const index = children.findIndex((child) => findTreeNode(child, leafId, ops) !== null);
    if (index === -1) return node;
    if (merge && ops.direction(node) === direction && ops.id(children[index]) === leafId) {
      // The target is a direct child: halve its share and insert beside it.
      const share = (sizes[index] ?? 1 / children.length) / 2;
      sizes[index] = share;
      const nextChildren = [...children.slice(0, index + 1), sibling, ...children.slice(index + 1)];
      const nextSizes = [...sizes.slice(0, index + 1), share, ...sizes.slice(index + 1)];
      return ops.withChildren(node, nextChildren, normalizeSizes(nextSizes, nextChildren.length));
    }
    children[index] = walk(children[index]);
    return ops.withChildren(node, children, sizes);
  };
  return walk(root);
}

/** Remove a node; single-child splits collapse into their child. */
export function removeTreeNode<N>(
  root: N,
  id: string,
  ops: TreeOps<N>,
): { node: N | null; removed: boolean } {
  if (ops.id(root) === id) return { node: null, removed: true };
  if (ops.isLeaf(root)) return { node: root, removed: false };
  const children = ops.children(root);
  const sizes = ops.sizes(root);
  let removed = false;
  const nextChildren: N[] = [];
  const nextSizes: number[] = [];
  children.forEach((child, index) => {
    const result = removeTreeNode(child, id, ops);
    removed ||= result.removed;
    if (result.node !== null) {
      nextChildren.push(result.node);
      nextSizes.push(sizes[index] ?? 1 / children.length);
    }
  });
  if (!removed) return { node: root, removed: false };
  if (nextChildren.length === 0) return { node: null, removed: true };
  if (nextChildren.length === 1) return { node: nextChildren[0], removed: true };
  return {
    node: ops.withChildren(root, nextChildren, normalizeSizes(nextSizes, nextChildren.length)),
    removed: true,
  };
}

export function resizeTreeSplit<N>(
  root: N,
  splitId: string,
  sizes: readonly number[],
  ops: TreeOps<N>,
): N {
  if (ops.isLeaf(root)) return root;
  const children = [...ops.children(root)];
  if (ops.id(root) === splitId) return ops.withChildren(root, children, normalizeSizes(sizes, children.length));
  return ops.withChildren(
    root,
    children.map((child) => resizeTreeSplit(child, splitId, sizes, ops)),
    [...ops.sizes(root)],
  );
}