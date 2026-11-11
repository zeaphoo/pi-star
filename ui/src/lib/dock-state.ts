/**
 * The right dock: a project-scoped container of independent views, laid out as
 * a split tree (left/right or top/bottom), in the spirit of a tiling window
 * manager. Every leaf is its own instance — its own tabs, its own selection,
 * its own shells — and nothing here is shared with the main view.
 *
 * The dock is keyed by project, so switching sessions keeps it while switching
 * projects restores that project's layout. Width and open state are the user's
 * own preferences and live outside this module (see AppShell).
 */

import { emptyCodeWorkspace, sanitizeCodeWorkspace, type CodeWorkspaceState } from "./code-workspace";
import { emptyGitWorkspace, sanitizeGitWorkspace, type GitWorkspaceState } from "./git-workspace";
import {
  collectTerminalIds,
  emptyTerminalWorkspace,
  sanitizeTerminalWorkspace,
  type TerminalWorkspaceState,
} from "./terminal-workspace";
import {
  collectTreeLeaves,
  countTreeLeaves,
  findTreeNode,
  normalizeSizes,
  removeTreeNode,
  resizeTreeSplit,
  splitTreeLeaf,
  type SplitDirection,
  type TreeOps,
} from "./split-tree";

export type DockView = "code" | "diff" | "terminal";
export const DOCK_VIEWS: readonly DockView[] = ["code", "diff", "terminal"];

export interface DockLeafNode {
  kind: "leaf";
  id: string;
  /** null: an empty pane waiting for the user to choose a view. */
  view: DockView | null;
}

export interface DockSplitNode {
  kind: "split";
  id: string;
  direction: SplitDirection;
  children: DockNode[];
  sizes: number[];
}

export type DockNode = DockLeafNode | DockSplitNode;

export interface DockLayout {
  root: DockNode | null;
  /** The leaf splits and new views attach to. */
  focusedLeafId: string | null;
}

/** One project's dock: layout plus each leaf's own view state. */
export interface DockState {
  layout: DockLayout;
  code: Record<string, CodeWorkspaceState>;
  diff: Record<string, GitWorkspaceState>;
  terminal: Record<string, TerminalWorkspaceState>;
}

/** A dock update either replaces the state or applies to the latest one. */
export type DockStateUpdater = DockState | ((current: DockState) => DockState);

/** What a new leaf's view needs to know about the project it belongs to. */
export interface DockContext {
  projectKey: string;
  root: string;
}

export const DOCK_STORAGE_KEY = "pi-star:dock:v1";
/** Enough for a wide monitor's two-by-two; past this leaves get unusable. */
export const DOCK_MAX_LEAVES = 4;
/** Each terminal leaf owns real PTYs, so they get their own smaller budget. */
export const DOCK_MAX_TERMINAL_LEAVES = 2;
export const DOCK_DEFAULT_WIDTH = 620;
export const DOCK_MIN_WIDTH = 360;
export const DOCK_MAX_WIDTH = 1200;
/** Below this viewport the dock hides and the main view keeps its own tabs. */
export const DOCK_MIN_VIEWPORT = 1024;
export const DOCK_MIN_LEAF_PX = 240;
/**
 * The chat column the dock may never eat into. The toolbar needs ~430 of it; a
 * comfortable conversation wants more, and the dock is for wide screens anyway.
 */
export const DOCK_MIN_CHAT_WIDTH = 560;
const MAX_STORED_PROJECTS = 20;

export const DOCK_TREE_OPS: TreeOps<DockNode> = {
  isLeaf: (node) => node.kind === "leaf",
  id: (node) => node.id,
  children: (node) => node.kind === "split" ? node.children : [],
  sizes: (node) => node.kind === "split" ? node.sizes : [],
  direction: (node) => node.kind === "split" ? node.direction : "row",
  makeSplit: (children, direction, sizes) => ({ kind: "split", id: randomId(), direction, children, sizes }),
  withChildren: (node, children, sizes) => node.kind === "split" ? { ...node, children, sizes } : node,
};

export function isDockView(value: unknown): value is DockView {
  return value === "code" || value === "diff" || value === "terminal";
}

export function emptyDockState(): DockState {
  return { layout: { root: null, focusedLeafId: null }, code: {}, diff: {}, terminal: {} };
}

export function dockLeaves(layout: DockLayout): DockLeafNode[] {
  const root = layout.root;
  if (!root) return [];
  return collectTreeLeaves(root, DOCK_TREE_OPS).filter((node): node is DockLeafNode => node.kind === "leaf");
}

export function dockLeafCount(layout: DockLayout): number {
  return layout.root ? countTreeLeaves(layout.root, DOCK_TREE_OPS) : 0;
}

export function countDockView(layout: DockLayout, view: DockView): number {
  return dockLeaves(layout).filter((leaf) => leaf.view === view).length;
}

export function canAddDockLeaf(state: DockState): boolean {
  return dockLeafCount(state.layout) < DOCK_MAX_LEAVES;
}

/** Whether an empty leaf may be filled with this view (the terminal budget still applies). */
export function canSetDockLeafView(state: DockState, view: DockView): boolean {
  return view !== "terminal" || countDockView(state.layout, "terminal") < DOCK_MAX_TERMINAL_LEAVES;
}

export function canAddDockView(state: DockState, view: DockView): boolean {
  return canAddDockLeaf(state) && canSetDockLeafView(state, view);
}

export function focusedDockLeaf(state: DockState): DockLeafNode | null {
  const leaves = dockLeaves(state.layout);
  return leaves.find((leaf) => leaf.id === state.layout.focusedLeafId) ?? leaves[0] ?? null;
}

/** Add a view beside the focused leaf (or as the first leaf of an empty dock). */
export function addDockView(
  state: DockState,
  view: DockView,
  direction: SplitDirection,
  context: DockContext,
): DockState {
  return insertDockLeaf(state, view, direction, context, null);
}

/** Split one leaf into a new empty pane; the user chooses its view afterwards. */
export function splitDockLeaf(
  state: DockState,
  leafId: string,
  direction: SplitDirection,
  context: DockContext,
): DockState {
  // An empty dock gets the first empty pane, ready for a view choice.
  if (!state.layout.root) return insertDockLeaf(state, null, direction, context, null);
  const leaf = findTreeNode(state.layout.root, leafId, DOCK_TREE_OPS);
  if (!leaf || leaf.kind !== "leaf") return state;
  return insertDockLeaf(state, null, direction, context, leafId);
}

/** Fill an empty leaf with one view. A leaf that already has a view never switches. */
export function setDockLeafView(
  state: DockState,
  leafId: string,
  view: DockView,
  context: DockContext,
): DockState {
  if (!state.layout.root) return state;
  const leaf = findTreeNode(state.layout.root, leafId, DOCK_TREE_OPS);
  if (!leaf || leaf.kind !== "leaf" || leaf.view !== null) return state;
  if (!canSetDockLeafView(state, view)) return state;
  const nextLeaf: DockLeafNode = { ...leaf, view };
  const withState = withLeafState(state, nextLeaf, context);
  return {
    ...withState,
    layout: {
      root: mapDockLeafView(state.layout.root, leafId, view),
      focusedLeafId: leafId,
    },
  };
}

/** Clear one leaf's view and state, leaving an empty pane in the same slot. */
export function clearDockLeafContent(
  state: DockState,
  leafId: string,
): { state: DockState; killedTerminalIds: string[] } {
  if (!state.layout.root) return { state, killedTerminalIds: [] };
  const leaf = findTreeNode(state.layout.root, leafId, DOCK_TREE_OPS);
  if (!leaf || leaf.kind !== "leaf" || leaf.view === null) return { state, killedTerminalIds: [] };
  const removedTerminal = state.terminal[leafId];
  const killedTerminalIds = removedTerminal
    ? removedTerminal.tabs.flatMap((tab) => collectTerminalIds(tab.root))
    : [];
  const code = { ...state.code };
  delete code[leafId];
  const diff = { ...state.diff };
  delete diff[leafId];
  const terminal = { ...state.terminal };
  delete terminal[leafId];
  return {
    state: {
      layout: { root: mapDockLeafView(state.layout.root, leafId, null), focusedLeafId: leafId },
      code,
      diff,
      terminal,
    },
    killedTerminalIds,
  };
}

function mapDockLeafView(node: DockNode, leafId: string, view: DockView | null): DockNode {
  if (node.kind === "leaf") return node.id === leafId ? { ...node, view } : node;
  return { ...node, children: node.children.map((child) => mapDockLeafView(child, leafId, view)) };
}

function insertDockLeaf(
  state: DockState,
  view: DockView | null,
  direction: SplitDirection,
  context: DockContext,
  targetLeafId: string | null,
): DockState {
  if (!canAddDockLeaf(state)) return state;
  if (view !== null && !canSetDockLeafView(state, view)) return state;
  const leaf: DockLeafNode = { kind: "leaf", id: randomId(), view };
  const withState = withLeafState(state, leaf, context);
  const root = withState.layout.root;
  if (!root) {
    return { ...withState, layout: { root: leaf, focusedLeafId: leaf.id } };
  }
  const target = targetLeafId ?? withState.layout.focusedLeafId ?? dockLeaves(withState.layout)[0]?.id ?? null;
  if (!target || !findTreeNode(root, target, DOCK_TREE_OPS)) return state;
  return {
    ...withState,
    layout: {
      root: splitTreeLeaf(root, target, direction, leaf, DOCK_TREE_OPS, { mergeSameDirection: true }),
      focusedLeafId: leaf.id,
    },
  };
}

function withLeafState(state: DockState, leaf: DockLeafNode, context: DockContext): DockState {
  if (leaf.view === "code") {
    return { ...state, code: { ...state.code, [leaf.id]: emptyCodeWorkspace(context.projectKey, context.root) } };
  }
  if (leaf.view === "diff") {
    return { ...state, diff: { ...state.diff, [leaf.id]: emptyGitWorkspace(context.projectKey) } };
  }
  if (leaf.view === "terminal") {
    return { ...state, terminal: { ...state.terminal, [leaf.id]: emptyTerminalWorkspace(context.projectKey) } };
  }
  return state;
}

/**
 * Close a leaf and drop its state. Terminal leaves report the shell ids they
 * owned, so the caller can end them; nothing else holds a resource.
 */
export function closeDockLeaf(
  state: DockState,
  leafId: string,
): { state: DockState; killedTerminalIds: string[] } {
  const leaf = dockLeaves(state.layout).find((item) => item.id === leafId);
  if (!leaf || !state.layout.root) return { state, killedTerminalIds: [] };
  const leaves = dockLeaves(state.layout);
  const index = leaves.findIndex((item) => item.id === leafId);
  const result = removeTreeNode(state.layout.root, leafId, DOCK_TREE_OPS);
  const removedTerminal = state.terminal[leafId];
  const killedTerminalIds = removedTerminal
    ? removedTerminal.tabs.flatMap((tab) => collectTerminalIds(tab.root))
    : [];
  const nextLeaves = result.node ? dockLeaves({ root: result.node, focusedLeafId: null }) : [];
  const focusedLeafId = nextLeaves[Math.min(index, nextLeaves.length - 1)]?.id ?? null;
  const remaining = { ...state.code };
  delete remaining[leafId];
  const remainingDiff = { ...state.diff };
  delete remainingDiff[leafId];
  const remainingTerminal = { ...state.terminal };
  delete remainingTerminal[leafId];
  return {
    state: {
      layout: { root: result.node, focusedLeafId },
      code: remaining,
      diff: remainingDiff,
      terminal: remainingTerminal,
    },
    killedTerminalIds,
  };
}

export function focusDockLeaf(state: DockState, leafId: string): DockState {
  if (state.layout.focusedLeafId === leafId) return state;
  if (!dockLeaves(state.layout).some((leaf) => leaf.id === leafId)) return state;
  return { ...state, layout: { ...state.layout, focusedLeafId: leafId } };
}

export function resizeDockSplit(state: DockState, splitId: string, sizes: readonly number[]): DockState {
  if (!state.layout.root) return state;
  const root = resizeTreeSplit(state.layout.root, splitId, sizes, DOCK_TREE_OPS);
  return { ...state, layout: { ...state.layout, root } };
}

export function setDockLeafCode(state: DockState, leafId: string, next: CodeWorkspaceState): DockState {
  if (!state.code[leafId]) return state;
  return { ...state, code: { ...state.code, [leafId]: next } };
}

export function setDockLeafDiff(state: DockState, leafId: string, next: GitWorkspaceState): DockState {
  if (!state.diff[leafId]) return state;
  return { ...state, diff: { ...state.diff, [leafId]: next } };
}

export function setDockLeafTerminal(state: DockState, leafId: string, next: TerminalWorkspaceState): DockState {
  if (!state.terminal[leafId]) return state;
  return { ...state, terminal: { ...state.terminal, [leafId]: next } };
}

/** Assign a workspace to a leaf whose state is missing (a new project's first show). */
export function ensureDockLeafState(state: DockState, context: DockContext): DockState {
  let next = state;
  for (const leaf of dockLeaves(state.layout)) {
    if (leaf.view === "code" && !next.code[leaf.id]) next = withLeafState(next, leaf, context);
    if (leaf.view === "diff" && !next.diff[leaf.id]) next = withLeafState(next, leaf, context);
    if (leaf.view === "terminal" && !next.terminal[leaf.id]) next = withLeafState(next, leaf, context);
  }
  return next;
}

function randomId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") return crypto.randomUUID();
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

// ── Persistence ──────────────────────────────────────────────────────────

function storage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

interface SanitizeBudget {
  leaves: number;
  terminals: number;
  ids: Set<string>;
}

function sanitizeDockNode(value: unknown, budget: SanitizeBudget): DockNode | null {
  if (!isRecord(value)) return null;
  const id = typeof value.id === "string" ? value.id : "";
  if (!id || budget.ids.has(id)) return null;
  if (value.kind === "leaf") {
    const view = value.view === null ? null : isDockView(value.view) ? value.view : undefined;
    if (view === undefined) return null;
    if (budget.leaves >= DOCK_MAX_LEAVES) return null;
    if (view === "terminal" && budget.terminals >= DOCK_MAX_TERMINAL_LEAVES) return null;
    budget.ids.add(id);
    budget.leaves += 1;
    if (view === "terminal") budget.terminals += 1;
    return { kind: "leaf", id, view };
  }
  if (value.kind !== "split") return null;
  budget.ids.add(id);
  const direction: SplitDirection = value.direction === "column" ? "column" : "row";
  const children: DockNode[] = [];
  for (const child of Array.isArray(value.children) ? value.children : []) {
    const node = sanitizeDockNode(child, budget);
    if (node) children.push(node);
    if (children.length >= DOCK_MAX_LEAVES) break;
  }
  if (children.length === 0) return null;
  // A split with one child is the child: never keep a useless container.
  if (children.length === 1) return children[0];
  const rawSizes = Array.isArray(value.sizes) ? value.sizes : [];
  return { kind: "split", id, direction, children, sizes: normalizeSizes(rawSizes, children.length) };
}

/** Validate one stored dock; exported so tests can exercise the reader directly. */
export function sanitizeDockState(value: unknown, projectKey: string): DockState | null {
  if (!isRecord(value)) return null;
  const layoutValue = isRecord(value.layout) ? value.layout : {};
  const root = sanitizeDockNode(layoutValue.root, { leaves: 0, terminals: 0, ids: new Set() });
  const leaves = root ? dockLeaves({ root, focusedLeafId: null }) : [];
  const leafIds = new Set(leaves.map((leaf) => leaf.id));
  const focusedLeafId = typeof layoutValue.focusedLeafId === "string" && leafIds.has(layoutValue.focusedLeafId)
    ? layoutValue.focusedLeafId
    : leaves[0]?.id ?? null;
  const code: Record<string, CodeWorkspaceState> = {};
  const diff: Record<string, GitWorkspaceState> = {};
  const terminal: Record<string, TerminalWorkspaceState> = {};
  for (const leaf of leaves) {
    if (leaf.view === null) continue;
    if (leaf.view === "code") {
      const stored = isRecord(value.code) ? sanitizeCodeWorkspace(value.code[leaf.id]) : null;
      if (stored) code[leaf.id] = stored;
    } else if (leaf.view === "diff") {
      const stored = isRecord(value.diff) ? sanitizeGitWorkspace(value.diff[leaf.id], projectKey) : null;
      if (stored) diff[leaf.id] = stored;
    } else {
      const stored = isRecord(value.terminal) ? sanitizeTerminalWorkspace(value.terminal[leaf.id], projectKey) : null;
      if (stored) terminal[leaf.id] = stored;
    }
  }
  return { layout: { root, focusedLeafId }, code, diff, terminal };
}

export function loadDockStates(): Record<string, DockState> {
  const store = storage();
  if (!store) return {};
  try {
    const raw = store.getItem(DOCK_STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed)) return {};
    const out: Record<string, DockState> = {};
    for (const [key, value] of Object.entries(parsed)) {
      const state = sanitizeDockState(value, key);
      if (state) out[key] = state;
    }
    return out;
  } catch {
    return {};
  }
}

export function saveDockStates(states: Record<string, DockState>): void {
  const store = storage();
  if (!store) return;
  try {
    const entries = Object.entries(states)
      .filter(([, state]) => state.layout.root !== null)
      .slice(-MAX_STORED_PROJECTS);
    store.setItem(DOCK_STORAGE_KEY, JSON.stringify(Object.fromEntries(entries)));
  } catch {
    // A full or unavailable storage must not break the dock.
  }
}