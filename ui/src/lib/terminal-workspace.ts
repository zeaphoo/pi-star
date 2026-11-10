/**
 * Project-scoped multi-tab, split-pane terminal layout.
 *
 * A tab is a recursive split tree. Leaves hold the server-side terminal id;
 * layout and ids survive a page reload, while the PTYs themselves are owned by
 * the Rust terminal manager.
 */

export type TerminalSplitDirection = "row" | "column";

export interface TerminalPaneState {
  kind: "pane";
  id: string;
  terminalId: string;
  cwd: string;
  /** A pane loaded from storage may still have a live PTY behind its id. */
  restored?: boolean;
}

export interface TerminalSplitState {
  kind: "split";
  id: string;
  direction: TerminalSplitDirection;
  children: TerminalTreeNode[];
  sizes: number[];
}

export type TerminalTreeNode = TerminalPaneState | TerminalSplitState;

export interface TerminalWorkspaceTab {
  id: string;
  title?: string;
  cwd: string;
  root: TerminalTreeNode;
  activePaneId: string | null;
}

export interface TerminalWorkspaceState {
  projectKey: string;
  tabs: TerminalWorkspaceTab[];
  activeTabId: string | null;
}

const STORAGE_KEY = "pi-star:terminal-workspaces:v1";
const MAX_WORKSPACES = 20;
const MAX_TABS = 6;
const MAX_PANES = 4;

export function maxTerminalPanes(): number {
  return MAX_PANES;
}

export function maxTerminalTabs(): number {
  return MAX_TABS;
}

function randomId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

export function newTerminalId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.getRandomValues === "function") {
    return Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) => byte.toString(16).padStart(2, "0")).join("");
  }
  return Array.from({ length: 32 }, () => Math.floor(Math.random() * 16).toString(16)).join("");
}

export function newTerminalPane(cwd: string, restored = false): TerminalPaneState {
  return {
    kind: "pane",
    id: randomId(),
    terminalId: newTerminalId(),
    cwd,
    ...(restored ? { restored: true } : {}),
  };
}

export function newTerminalWorkspaceTab(cwd: string): TerminalWorkspaceTab {
  const pane = newTerminalPane(cwd);
  return {
    id: randomId(),
    cwd,
    root: pane,
    activePaneId: pane.id,
  };
}

export function emptyTerminalWorkspace(projectKey: string): TerminalWorkspaceState {
  return { projectKey, tabs: [], activeTabId: null };
}

export function findTerminalPane(node: TerminalTreeNode, paneId: string): TerminalPaneState | null {
  if (node.kind === "pane") return node.id === paneId ? node : null;
  for (const child of node.children) {
    const found = findTerminalPane(child, paneId);
    if (found) return found;
  }
  return null;
}

export function firstTerminalPane(node: TerminalTreeNode): TerminalPaneState {
  if (node.kind === "pane") return node;
  for (const child of node.children) {
    const found = firstTerminalPane(child);
    if (found) return found;
  }
  return node as unknown as TerminalPaneState;
}

export function countTerminalPanes(node: TerminalTreeNode): number {
  if (node.kind === "pane") return 1;
  return node.children.reduce((total, child) => total + countTerminalPanes(child), 0);
}

export function collectTerminalIds(node: TerminalTreeNode): string[] {
  if (node.kind === "pane") return [node.terminalId];
  return node.children.flatMap(collectTerminalIds);
}

/** Replace one leaf with a split holding the old and a new pane. */
export function splitTerminalPane(
  tab: TerminalWorkspaceTab,
  paneId: string,
  direction: TerminalSplitDirection,
): TerminalWorkspaceTab {
  if (countTerminalPanes(tab.root) >= MAX_PANES) return tab;
  const target = findTerminalPane(tab.root, paneId);
  if (!target) return tab;
  const added = newTerminalPane(target.cwd);
  const split = (node: TerminalTreeNode): TerminalTreeNode => {
    if (node.kind === "pane") {
      if (node.id !== paneId) return node;
      return {
        kind: "split",
        id: randomId(),
        direction,
        children: [node, added],
        sizes: [0.5, 0.5],
      };
    }
    return { ...node, children: node.children.map(split) };
  };
  return { ...tab, root: split(tab.root), activePaneId: added.id };
}

function removeTerminalNode(
  node: TerminalTreeNode,
  paneId: string,
): { node: TerminalTreeNode | null; removed: boolean } {
  if (node.kind === "pane") {
    return node.id === paneId ? { node: null, removed: true } : { node, removed: false };
  }
  let removed = false;
  const children: TerminalTreeNode[] = [];
  const sizes: number[] = [];
  node.children.forEach((child, index) => {
    const result = removeTerminalNode(child, paneId);
    removed ||= result.removed;
    if (result.node) {
      children.push(result.node);
      sizes.push(node.sizes[index] ?? 1 / node.children.length);
    }
  });
  if (!removed) return { node, removed: false };
  if (children.length === 0) return { node: null, removed: true };
  if (children.length === 1) return { node: children[0], removed: true };
  const total = sizes.reduce((sum, size) => sum + size, 0) || 1;
  return {
    node: { ...node, children, sizes: sizes.map((size) => size / total) },
    removed: true,
  };
}

/** Remove a pane, collapse a single-child split, and pick a new active pane. */
export function closeTerminalPane(
  tab: TerminalWorkspaceTab,
  paneId: string,
): TerminalWorkspaceTab | null {
  const result = removeTerminalNode(tab.root, paneId);
  if (!result.node) return null;
  const activePaneId = tab.activePaneId === paneId
    ? firstTerminalPane(result.node).id
    : tab.activePaneId;
  return { ...tab, root: result.node, activePaneId };
}

export function setTerminalActivePane(
  tab: TerminalWorkspaceTab,
  paneId: string,
): TerminalWorkspaceTab {
  if (!findTerminalPane(tab.root, paneId)) return tab;
  return { ...tab, activePaneId: paneId };
}

/** Give a pane a fresh terminal id after its PTY was restarted. */
export function replaceTerminalPaneId(
  tab: TerminalWorkspaceTab,
  paneId: string,
): TerminalWorkspaceTab {
  const next = (node: TerminalTreeNode): TerminalTreeNode => {
    if (node.kind === "pane") {
      return node.id === paneId
        ? { ...node, terminalId: newTerminalId(), restored: false }
        : node;
    }
    return { ...node, children: node.children.map(next) };
  };
  return { ...tab, root: next(tab.root) };
}

export function resizeTerminalSplit(
  node: TerminalTreeNode,
  splitId: string,
  sizes: number[],
): TerminalTreeNode {
  if (node.kind === "pane") return node;
  if (node.id === splitId) {
    const total = sizes.reduce((sum, size) => sum + size, 0) || 1;
    return { ...node, sizes: sizes.map((size) => size / total) };
  }
  return { ...node, children: node.children.map((child) => resizeTerminalSplit(child, splitId, sizes)) };
}

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

function sanitizeNode(value: unknown, remaining: { count: number }): TerminalTreeNode | null {
  if (!isRecord(value) || remaining.count >= MAX_PANES) return null;
  if (value.kind === "pane") {
    const id = typeof value.id === "string" ? value.id : "";
    const terminalId = typeof value.terminalId === "string" ? value.terminalId : "";
    const cwd = typeof value.cwd === "string" ? value.cwd : "";
    if (!id || !/^[a-f0-9]{32}$/.test(terminalId) || !cwd) return null;
    remaining.count += 1;
    return { kind: "pane", id, terminalId, cwd, restored: true };
  }
  if (value.kind === "split") {
    const id = typeof value.id === "string" ? value.id : "";
    const direction: TerminalSplitDirection = value.direction === "column" ? "column" : "row";
    if (!id || !Array.isArray(value.children)) return null;
    const children: TerminalTreeNode[] = [];
    for (const child of value.children) {
      const node = sanitizeNode(child, remaining);
      if (node) children.push(node);
      if (children.length >= 4) break;
    }
    if (children.length < 2) return children[0] ?? null;
    const rawSizes = Array.isArray(value.sizes) ? value.sizes : [];
    const sizes = children.map((_, index) => {
      const size = Number(rawSizes[index]);
      return Number.isFinite(size) && size > 0 ? size : 1 / children.length;
    });
    const total = sizes.reduce((sum, size) => sum + size, 0) || 1;
    return {
      kind: "split",
      id,
      direction,
      children,
      sizes: sizes.map((size) => size / total),
    };
  }
  return null;
}

function sanitizeTab(value: unknown): TerminalWorkspaceTab | null {
  if (!isRecord(value)) return null;
  const id = typeof value.id === "string" ? value.id : "";
  const cwd = typeof value.cwd === "string" ? value.cwd : "";
  if (!id || !cwd) return null;
  const root = sanitizeNode(value.root, { count: 0 });
  if (!root) return null;
  const activePaneId = typeof value.activePaneId === "string" && findTerminalPane(root, value.activePaneId)
    ? value.activePaneId
    : firstTerminalPane(root).id;
  return {
    id,
    ...(typeof value.title === "string" && value.title ? { title: value.title } : {}),
    cwd,
    root,
    activePaneId,
  };
}

function sanitizeWorkspace(value: unknown, projectKey: string): TerminalWorkspaceState | null {
  if (!isRecord(value)) return null;
  const tabs = (Array.isArray(value.tabs) ? value.tabs : [])
    .map(sanitizeTab)
    .filter((tab): tab is TerminalWorkspaceTab => tab !== null)
    .slice(0, MAX_TABS);
  const activeTabId = typeof value.activeTabId === "string" && tabs.some((tab) => tab.id === value.activeTabId)
    ? value.activeTabId
    : tabs[0]?.id ?? null;
  return { projectKey, tabs, activeTabId };
}

export function loadTerminalWorkspaces(): Record<string, TerminalWorkspaceState> {
  try {
    const raw = storage()?.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed)) return {};
    const out: Record<string, TerminalWorkspaceState> = {};
    for (const [key, value] of Object.entries(parsed)) {
      const workspace = sanitizeWorkspace(value, key);
      if (workspace) out[key] = workspace;
    }
    return out;
  } catch {
    return {};
  }
}

export function saveTerminalWorkspace(key: string, state: TerminalWorkspaceState): void {
  const store = storage();
  if (!store) return;
  try {
    const current = loadTerminalWorkspaces();
    delete current[key];
    current[key] = state;
    const keys = Object.keys(current);
    for (const stale of keys.slice(0, Math.max(0, keys.length - MAX_WORKSPACES))) {
      delete current[stale];
    }
    store.setItem(STORAGE_KEY, JSON.stringify(current));
  } catch {
    // Terminal browsing must keep working when storage is unavailable.
  }
}
