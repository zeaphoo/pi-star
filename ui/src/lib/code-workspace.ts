import type { Tab } from "@/components/TabBar";
import { openFileTab } from "@/components/file-tab-state";

/**
 * Project-scoped code browser state.
 *
 * Unlike the right-hand file panel, a code workspace is keyed by the project
 * (and the worktree it is browsing), never by a session id. Switching between
 * sessions of the same project therefore keeps the open tabs, while switching
 * projects restores that project's previous workspace.
 */

export type MainView = "chat" | "code" | "git" | "terminal";

export interface CodeWorkspaceState {
  projectKey: string;
  root: string;
  tabs: Tab[];
  activeTabId: string | null;
}

const WORKSPACES_KEY = "pi-star:code-workspaces:v1";
const MAIN_VIEW_KEY = "pi-star:main-view";
const MAX_WORKSPACES = 20;
const MAX_TABS = 20;

/** Stable key for a worktree-scoped code workspace. */
export function makeCodeWorkspaceKey(projectKey: string | null | undefined, root: string): string {
  return `${projectKey ?? root}::${root}`;
}

export function emptyCodeWorkspace(projectKey: string, root: string): CodeWorkspaceState {
  return { projectKey, root, tabs: [], activeTabId: null };
}

function storage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/**
 * The main-view preference, with `?view=code` taking precedence over the last
 * choice remembered in this browser.
 */
export function loadMainView(): MainView {
  if (typeof window !== "undefined") {
    try {
      const view = new URL(window.location.href).searchParams.get("view");
      if (view === "code" || view === "chat" || view === "git" || view === "terminal") return view;
    } catch {
      // Fall through to the remembered value.
    }
  }
  try {
    return storage()?.getItem(MAIN_VIEW_KEY) === "code" ? "code" : "chat";
  } catch {
    return "chat";
  }
}

export function saveMainView(view: MainView): void {
  try {
    storage()?.setItem(MAIN_VIEW_KEY, view);
  } catch {
    // Storage is best-effort.
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function sanitizeTab(value: unknown): Tab | null {
  if (!isRecord(value)) return null;
  const id = typeof value.id === "string" ? value.id : "";
  const label = typeof value.label === "string" ? value.label : "";
  const filePath = typeof value.filePath === "string" ? value.filePath : "";
  if (!id || !filePath) return null;
  const tab: Tab = {
    id,
    label: label || filePath.replace(/\\/g, "/").split("/").pop() || filePath,
    filePath,
    sourceSessionId: null,
  };
  if (typeof value.page === "number" && Number.isFinite(value.page)) tab.page = value.page;
  if (typeof value.viewerRevision === "number" && Number.isFinite(value.viewerRevision)) {
    tab.viewerRevision = value.viewerRevision;
  }
  if (isRecord(value.viewerState)) {
    const mode = value.viewerState.displayMode;
    if (mode === "source" || mode === "preview" || mode === "diff") {
      tab.viewerState = {
        displayMode: mode,
        ...(typeof value.viewerState.wrapLines === "boolean" ? { wrapLines: value.viewerState.wrapLines } : {}),
        scrollTop: typeof value.viewerState.scrollTop === "number" ? value.viewerState.scrollTop : 0,
        scrollLeft: typeof value.viewerState.scrollLeft === "number" ? value.viewerState.scrollLeft : 0,
      };
    }
  }
  return tab;
}

/** Read one stored workspace; exported for the right dock's per-leaf copies. */
export function sanitizeCodeWorkspace(value: unknown): CodeWorkspaceState | null {
  if (!isRecord(value)) return null;
  const projectKey = typeof value.projectKey === "string" ? value.projectKey : "";
  const root = typeof value.root === "string" ? value.root : "";
  if (!root) return null;
  const tabs = Array.isArray(value.tabs)
    ? value.tabs.map(sanitizeTab).filter((tab): tab is Tab => tab !== null).slice(-MAX_TABS)
    : [];
  const activeTabId = typeof value.activeTabId === "string" ? value.activeTabId : null;
  return {
    projectKey: projectKey || root,
    root,
    tabs,
    activeTabId: activeTabId && tabs.some((tab) => tab.id === activeTabId)
      ? activeTabId
      : tabs.at(-1)?.id ?? null,
  };
}

export function loadCodeWorkspaces(): Record<string, CodeWorkspaceState> {
  try {
    const raw = storage()?.getItem(WORKSPACES_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed)) return {};
    const out: Record<string, CodeWorkspaceState> = {};
    for (const [key, value] of Object.entries(parsed)) {
      const state = sanitizeCodeWorkspace(value);
      if (state) out[key] = state;
    }
    return out;
  } catch {
    return {};
  }
}

function trimWorkspaces(states: Record<string, CodeWorkspaceState>): Record<string, CodeWorkspaceState> {
  const keys = Object.keys(states);
  if (keys.length <= MAX_WORKSPACES) return states;
  const out: Record<string, CodeWorkspaceState> = {};
  for (const key of keys.slice(-MAX_WORKSPACES)) out[key] = states[key];
  return out;
}

export function saveCodeWorkspace(key: string, state: CodeWorkspaceState): void {
  const store = storage();
  if (!store) return;
  try {
    const current = loadCodeWorkspaces();
    // Delete first so the updated workspace becomes the most recent entry.
    delete current[key];
    current[key] = state;
    store.setItem(WORKSPACES_KEY, JSON.stringify(trimWorkspaces(current)));
  } catch {
    // A full or unavailable storage must not break browsing.
  }
}

/** Open a file in the code workspace, reusing an existing tab when present. */
export function openCodeFileTab(
  state: CodeWorkspaceState,
  filePath: string,
  fileName: string,
  page?: number,
): CodeWorkspaceState {
  const tabId = `code:${filePath}`;
  const tabs = openFileTab(state.tabs, {
    fileName,
    filePath,
    tabId,
    page,
    sourceSessionId: null,
  });
  return { ...state, tabs, activeTabId: tabId };
}

/** Close a tab and pick a sensible neighbour when the active one closes. */
export function closeCodeTab(state: CodeWorkspaceState, tabId: string): CodeWorkspaceState {
  const index = state.tabs.findIndex((tab) => tab.id === tabId);
  if (index === -1) return state;
  const tabs = state.tabs.filter((tab) => tab.id !== tabId);
  const activeTabId = state.activeTabId === tabId
    ? tabs[Math.min(index, tabs.length - 1)]?.id ?? null
    : state.activeTabId;
  return { ...state, tabs, activeTabId };
}
