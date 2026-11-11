import type { FileViewerState } from "@/lib/file-viewer-state";
import type { Tab } from "./TabBar";

const AGENT_TAB_PREFIX = "agent:";

/** Tab id of a sub-agent's read-only transcript. */
export function agentTabId(sessionId: string): string {
  return `${AGENT_TAB_PREFIX}${sessionId}`;
}

/** The child session a tab shows, or null when it shows a file. */
export function agentTabSessionId(tabId: string): string | null {
  return tabId.startsWith(AGENT_TAB_PREFIX) ? tabId.slice(AGENT_TAB_PREFIX.length) : null;
}

/**
 * A sub-agent transcript tab. Its `sourceSessionId` is the child's own session:
 * a worktree-isolated child resolves files against its own cwd, not the parent's.
 */
export function openAgentTab(tabs: Tab[], sessionId: string, label: string): Tab[] {
  const id = agentTabId(sessionId);
  const existing = tabs.find((tab) => tab.id === id);
  if (existing) return existing.label === label ? tabs : tabs.map((tab) => tab.id === id ? { ...tab, label } : tab);
  return [...tabs, {
    id,
    label,
    filePath: label,
    kind: "agent",
    sourceSessionId: sessionId,
  }];
}

interface OpenFileTabInput {
  fileName: string;
  filePath: string;
  modeHint?: "diff";
  page?: number;
  sourceSessionId?: string | null;
  tabId: string;
}

export function openFileTab(tabs: Tab[], input: OpenFileTabInput): Tab[] {
  const existing = tabs.find((tab) => tab.id === input.tabId);
  if (!existing) {
    return [...tabs, {
      id: input.tabId,
      label: input.fileName,
      filePath: input.filePath,
      sourceSessionId: input.sourceSessionId,
      initialDisplayMode: input.modeHint,
      page: input.page,
      viewerState: input.modeHint ? {
        displayMode: input.modeHint,
        scrollTop: 0,
        scrollLeft: 0,
      } : undefined,
      viewerRevision: 0,
    }];
  }

  const sourceChanged = Boolean(
    input.sourceSessionId && existing.sourceSessionId !== input.sourceSessionId,
  );
  const sourceUnchanged = !sourceChanged;
  const pageChanged = existing.page !== input.page;
  if (sourceUnchanged && !input.modeHint && !pageChanged) return tabs;

  return tabs.map((tab) => {
    if (tab.id !== input.tabId) return tab;
    const next: Tab = { ...tab };
    let bumpRevision = false;
    if (sourceChanged) {
      next.sourceSessionId = input.sourceSessionId;
      bumpRevision = true;
    }
    if (pageChanged) {
      next.page = input.page;
      bumpRevision = true;
    }
    if (input.modeHint) {
      next.initialDisplayMode = input.modeHint;
      next.viewerState = {
        displayMode: input.modeHint,
        // An explicit wrap choice carries over; an unmade one stays unmade so
        // the file's own kind still decides in the diff view too.
        wrapLines: tab.viewerState?.wrapLines,
        scrollTop: 0,
        scrollLeft: 0,
      };
      bumpRevision = true;
    }
    if (bumpRevision) next.viewerRevision = (tab.viewerRevision ?? 0) + 1;
    return next;
  });
}

export function saveFileViewerState(
  tabs: Tab[],
  tabId: string,
  viewerRevision: number,
  viewerState: FileViewerState,
): Tab[] {
  const index = tabs.findIndex((tab) => tab.id === tabId);
  if (index === -1 || (tabs[index].viewerRevision ?? 0) !== viewerRevision) return tabs;

  const next = [...tabs];
  next[index] = { ...next[index], viewerState };
  return next;
}
