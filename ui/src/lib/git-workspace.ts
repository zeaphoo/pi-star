/**
 * Project-scoped Git browser state.
 *
 * The Git tab follows the selected project, not a session: switching sessions
 * inside one project keeps the chosen repository, sub-tab and selection.
 */

export type GitTab = "changes" | "history";

export interface GitWorkspaceState {
  projectKey: string;
  selectedRepoPath: string | null;
  tab: GitTab;
  selectedFilePath: string | null;
  selectedFileScope: "staged" | "unstaged" | "all";
  selectedCommitSha: string | null;
  selectedCommitFilePath: string | null;
}

const STORAGE_KEY = "pi-star:git-workspaces:v1";
const MAX_WORKSPACES = 20;

export function emptyGitWorkspace(projectKey: string): GitWorkspaceState {
  return {
    projectKey,
    selectedRepoPath: null,
    tab: "changes",
    selectedFilePath: null,
    selectedFileScope: "all",
    selectedCommitSha: null,
    selectedCommitFilePath: null,
  };
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

/** Read one stored workspace; exported for the right dock's per-leaf copies. */
export function sanitizeGitWorkspace(value: unknown, projectKey: string): GitWorkspaceState | null {
  if (!isRecord(value)) return null;
  const tab: GitTab = value.tab === "history" ? "history" : "changes";
  const scope = value.selectedFileScope;
  return {
    projectKey,
    selectedRepoPath: typeof value.selectedRepoPath === "string" ? value.selectedRepoPath : null,
    tab,
    selectedFilePath: typeof value.selectedFilePath === "string" ? value.selectedFilePath : null,
    selectedFileScope: scope === "staged" || scope === "unstaged" ? scope : "all",
    selectedCommitSha: typeof value.selectedCommitSha === "string" ? value.selectedCommitSha : null,
    selectedCommitFilePath: typeof value.selectedCommitFilePath === "string"
      ? value.selectedCommitFilePath
      : null,
  };
}

export function loadGitWorkspaces(): Record<string, GitWorkspaceState> {
  try {
    const raw = storage()?.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!isRecord(parsed)) return {};
    const out: Record<string, GitWorkspaceState> = {};
    for (const [key, value] of Object.entries(parsed)) {
      const state = sanitizeGitWorkspace(value, key);
      if (state) out[key] = state;
    }
    return out;
  } catch {
    return {};
  }
}

export function saveGitWorkspace(key: string, state: GitWorkspaceState): void {
  const store = storage();
  if (!store) return;
  try {
    const current = loadGitWorkspaces();
    delete current[key];
    current[key] = state;
    const keys = Object.keys(current);
    for (const stale of keys.slice(0, Math.max(0, keys.length - MAX_WORKSPACES))) {
      delete current[stale];
    }
    store.setItem(STORAGE_KEY, JSON.stringify(current));
  } catch {
    // Git browsing must keep working when storage is unavailable.
  }
}
