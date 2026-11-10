"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "@/hooks/useI18n";
import { getFileName } from "@/lib/file-paths";
import type {
  GitCommitDetail,
  GitCommitFile,
  GitFileDiffResponse,
  GitFileStatus,
  GitHistoryCommit,
  GitRepoSummary,
  GitStatusResponse,
} from "@/lib/git-types";
import type { GitTab, GitWorkspaceState } from "@/lib/git-workspace";
import { GitDiffView } from "./GitDiffView";

interface Props {
  projectRoot: string | null;
  state: GitWorkspaceState;
  active: boolean;
  isMobile: boolean;
  onChange: (state: GitWorkspaceState) => void;
}

const BAR_HEIGHT = 34;
const FILE_RAIL_WIDTH = 300;
const COMMIT_RAIL_WIDTH = 300;

function isChanged(value: string | undefined): boolean {
  const text = (value ?? "").trim();
  return text !== "" && text !== "." && text !== "?";
}

function relativePath(root: string | null, path: string): string {
  if (!root) return path;
  const normalizedRoot = root.replace(/\\/g, "/").replace(/\/$/, "");
  const normalizedPath = path.replace(/\\/g, "/");
  return normalizedPath.startsWith(`${normalizedRoot}/`)
    ? normalizedPath.slice(normalizedRoot.length + 1)
    : normalizedPath;
}

function statusColor(status: string): string {
  switch (status) {
    case "added":
    case "untracked":
      return "#4ade80";
    case "deleted":
    case "conflict":
      return "#f87171";
    case "renamed":
    case "copied":
      return "#60a5fa";
    default:
      return "#d6a84b";
  }
}

function statusLetter(status: string): string {
  switch (status) {
    case "added":
    case "untracked":
      return "A";
    case "deleted":
      return "D";
    case "renamed":
      return "R";
    case "copied":
      return "C";
    case "conflict":
      return "!";
    default:
      return "M";
  }
}

function formatDate(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

async function readJson<T>(response: Response, fallback: string): Promise<T> {
  if (!response.ok) {
    let message = `${fallback} (HTTP ${response.status})`;
    try {
      const body = await response.json() as { error?: string };
      if (body.error) message = body.error;
    } catch {
      // Keep the status-based message.
    }
    throw new Error(message);
  }
  return response.json() as Promise<T>;
}

function IconButton({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        width: 26,
        height: 26,
        padding: 0,
        border: "none",
        borderRadius: 5,
        background: "transparent",
        color: "var(--text-muted)",
        cursor: "pointer",
        flexShrink: 0,
      }}
      onMouseEnter={(event) => { event.currentTarget.style.color = "var(--text)"; }}
      onMouseLeave={(event) => { event.currentTarget.style.color = "var(--text-muted)"; }}
    >
      {children}
    </button>
  );
}

function RefreshIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M21 12a9 9 0 1 1-2.6-6.4" />
      <path d="M21 3v6h-6" />
    </svg>
  );
}

function EmptyState({ title, hint }: { title: string; hint?: string }) {
  return (
    <div style={{ height: "100%", display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 7, padding: 24, color: "var(--text-muted)", textAlign: "center" }}>
      <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.6 }} aria-hidden="true">
        <circle cx="12" cy="5" r="2.5" />
        <circle cx="6" cy="18" r="2.5" />
        <circle cx="18" cy="18" r="2.5" />
        <path d="M12 7.5v4a6.5 6.5 0 0 1-6 6.5M12 11.5a6.5 6.5 0 0 1 6 6.5" />
      </svg>
      <div style={{ fontSize: 14, color: "var(--text)" }}>{title}</div>
      {hint && <div style={{ fontSize: 12, maxWidth: 420 }}>{hint}</div>}
    </div>
  );
}

function SectionHeader({ label, count }: { label: string; count: number }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6, height: 24, padding: "0 10px", color: "var(--text-dim)", fontSize: 10, fontWeight: 700, letterSpacing: "0.04em", textTransform: "uppercase" }}>
      <span>{label}</span>
      <span style={{ fontFamily: "var(--font-mono)", fontWeight: 500 }}>{count}</span>
    </div>
  );
}

function ChangesView({
  root,
  status,
  selectedFile,
  diff,
  onSelectFile,
  isMobile,
  loading,
  t,
}: {
  root: string | null;
  status: GitStatusResponse | null;
  selectedFile: { path: string; scope: "staged" | "unstaged" | "all" } | null;
  diff: GitFileDiffResponse | null;
  onSelectFile: (path: string, scope: "staged" | "unstaged" | "all") => void;
  isMobile: boolean;
  loading: boolean;
  t: ReturnType<typeof useI18n>["t"];
}) {
  const groups = useMemo(() => {
    const staged: GitFileStatus[] = [];
    const unstaged: GitFileStatus[] = [];
    const untracked: GitFileStatus[] = [];
    const conflicts: GitFileStatus[] = [];
    for (const file of status?.files ?? []) {
      if (file.status === "conflict") {
        conflicts.push(file);
        continue;
      }
      if (file.status === "untracked") {
        untracked.push(file);
        continue;
      }
      if (isChanged(file.indexStatus)) staged.push(file);
      if (isChanged(file.worktreeStatus)) unstaged.push(file);
    }
    return { staged, unstaged, untracked, conflicts };
  }, [status]);

  const total = (status?.files.length ?? 0);
  const empty = !loading && total === 0;

  const rows = (
    <>
      {groups.conflicts.length > 0 && (
        <div>
          <SectionHeader label={t("git.conflicts")} count={groups.conflicts.length} />
          {groups.conflicts.map((file) => (
            <ChangeRow key={`conflict:${file.filePath}`} file={file} root={root} scope="all" selectedFile={selectedFile} onSelectFile={onSelectFile} />
          ))}
        </div>
      )}
      {groups.staged.length > 0 && (
        <div>
          <SectionHeader label={t("git.staged")} count={groups.staged.length} />
          {groups.staged.map((file) => (
            <ChangeRow key={`staged:${file.filePath}`} file={file} root={root} scope="staged" selectedFile={selectedFile} onSelectFile={onSelectFile} />
          ))}
        </div>
      )}
      {groups.unstaged.length > 0 && (
        <div>
          <SectionHeader label={t("git.unstaged")} count={groups.unstaged.length} />
          {groups.unstaged.map((file) => (
            <ChangeRow key={`unstaged:${file.filePath}`} file={file} root={root} scope="unstaged" selectedFile={selectedFile} onSelectFile={onSelectFile} />
          ))}
        </div>
      )}
      {groups.untracked.length > 0 && (
        <div>
          <SectionHeader label={t("git.untracked")} count={groups.untracked.length} />
          {groups.untracked.map((file) => (
            <ChangeRow key={`untracked:${file.filePath}`} file={file} root={root} scope="unstaged" selectedFile={selectedFile} onSelectFile={onSelectFile} />
          ))}
        </div>
      )}
    </>
  );

  return (
    <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: isMobile ? "column" : "row" }}>
      <div
        className="git-file-rail"
        style={{
          width: isMobile ? "100%" : FILE_RAIL_WIDTH,
          minWidth: isMobile ? 0 : FILE_RAIL_WIDTH,
          maxHeight: isMobile ? 220 : undefined,
          flexShrink: 0,
          overflow: "auto",
          borderRight: isMobile ? undefined : "1px solid var(--border)",
          borderBottom: isMobile ? "1px solid var(--border)" : undefined,
          background: "var(--bg-panel)",
          paddingBottom: 8,
        }}
      >
        {loading && total === 0 ? (
          <div style={{ padding: 12, color: "var(--text-dim)", fontSize: 11 }}>{t("git.loading")}</div>
        ) : empty ? (
          <div style={{ padding: 20, color: "var(--text-muted)", fontSize: 12, textAlign: "center", lineHeight: 1.6 }}>
            {t("git.noChanges")}
          </div>
        ) : rows}
      </div>
      <div style={{ flex: 1, minWidth: 0, minHeight: 0, overflow: "hidden" }}>
        {selectedFile ? (
          <GitDiffView
            patch={diff?.supported ? diff.patch ?? null : null}
            emptyLabel={diff?.supported ? t("git.noTextDiff") : t("git.selectFileHint")}
          />
        ) : (
          <EmptyState title={t("git.selectFile")} hint={t("git.selectFileHint")} />
        )}
      </div>
    </div>
  );
}

function ChangeRow({
  file,
  root,
  scope,
  selectedFile,
  onSelectFile,
}: {
  file: GitFileStatus;
  root: string | null;
  scope: "staged" | "unstaged" | "all";
  selectedFile: { path: string; scope: "staged" | "unstaged" | "all" } | null;
  onSelectFile: (path: string, scope: "staged" | "unstaged" | "all") => void;
}) {
  const path = relativePath(root, file.filePath);
  const name = getFileName(path);
  const parent = path.slice(0, Math.max(0, path.length - name.length - 1));
  const selected = selectedFile?.path === file.filePath && selectedFile.scope === scope;
  const color = statusColor(file.status);
  return (
    <button
      type="button"
      onClick={() => onSelectFile(file.filePath, scope)}
      title={path}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 7,
        width: "100%",
        minHeight: 28,
        padding: "3px 8px 3px 10px",
        border: "none",
        borderLeft: selected ? "2px solid var(--accent)" : "2px solid transparent",
        background: selected ? "var(--bg-selected)" : "transparent",
        color: "var(--text)",
        cursor: "pointer",
        textAlign: "left",
        fontSize: 11,
      }}
      onMouseEnter={(event) => { if (!selected) event.currentTarget.style.background = "var(--bg-hover)"; }}
      onMouseLeave={(event) => { if (!selected) event.currentTarget.style.background = "transparent"; }}
    >
      <span style={{ width: 13, flexShrink: 0, color, fontFamily: "var(--font-mono)", fontWeight: 700, textAlign: "center" }}>
        {statusLetter(file.status)}
      </span>
      <span style={{ minWidth: 0, flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        {name}
        {parent && <span style={{ marginLeft: 6, color: "var(--text-dim)", fontSize: 10 }}>{parent}</span>}
      </span>
    </button>
  );
}

function HistoryView({
  root,
  commits,
  detail,
  selectedCommitSha,
  selectedFilePath,
  commitDiff,
  loading,
  onSelectCommit,
  onSelectFile,
  isMobile,
  t,
}: {
  root: string | null;
  commits: GitHistoryCommit[];
  detail: GitCommitDetail | null;
  selectedCommitSha: string | null;
  selectedFilePath: string | null;
  commitDiff: string | null;
  loading: boolean;
  onSelectCommit: (sha: string) => void;
  onSelectFile: (file: GitCommitFile) => void;
  isMobile: boolean;
  t: ReturnType<typeof useI18n>["t"];
}) {
  const selectedFile = detail?.files.find((file) => file.path === selectedFilePath) ?? detail?.files[0] ?? null;

  return (
    <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: isMobile ? "column" : "row" }}>
      <div
        className="git-commit-rail"
        style={{
          width: isMobile ? "100%" : COMMIT_RAIL_WIDTH,
          minWidth: isMobile ? 0 : COMMIT_RAIL_WIDTH,
          maxHeight: isMobile ? 220 : undefined,
          flexShrink: 0,
          overflow: "auto",
          borderRight: isMobile ? undefined : "1px solid var(--border)",
          borderBottom: isMobile ? "1px solid var(--border)" : undefined,
          background: "var(--bg-panel)",
        }}
      >
        {loading && commits.length === 0 ? (
          <div style={{ padding: 12, color: "var(--text-dim)", fontSize: 11 }}>{t("git.loading")}</div>
        ) : commits.length === 0 ? (
          <div style={{ padding: 20, color: "var(--text-muted)", fontSize: 12, textAlign: "center" }}>{t("git.noCommits")}</div>
        ) : commits.map((commit) => {
          const selected = commit.sha === selectedCommitSha;
          return (
            <button
              key={commit.sha}
              type="button"
              onClick={() => onSelectCommit(commit.sha)}
              title={commit.subject}
              style={{
                display: "block",
                width: "100%",
                padding: "7px 10px",
                border: "none",
                borderLeft: selected ? "2px solid var(--accent)" : "2px solid transparent",
                borderBottom: "1px solid var(--border)",
                background: selected ? "var(--bg-selected)" : "transparent",
                color: "var(--text)",
                cursor: "pointer",
                textAlign: "left",
              }}
              onMouseEnter={(event) => { if (!selected) event.currentTarget.style.background = "var(--bg-hover)"; }}
              onMouseLeave={(event) => { if (!selected) event.currentTarget.style.background = "transparent"; }}
            >
              <div style={{ display: "flex", alignItems: "baseline", gap: 7, minWidth: 0 }}>
                <span style={{ flexShrink: 0, color: "var(--text-muted)", fontFamily: "var(--font-mono)", fontSize: 10 }}>{commit.shortSha}</span>
                <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontSize: 11 }}>{commit.subject}</span>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 3, minWidth: 0, color: "var(--text-dim)", fontSize: 10 }}>
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{commit.author}</span>
                <span>·</span>
                <span style={{ flexShrink: 0 }}>{formatDate(commit.date)}</span>
                {commit.refs.length > 0 && <span style={{ marginLeft: "auto", color: "var(--accent)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{commit.refs.join(", ")}</span>}
              </div>
            </button>
          );
        })}
      </div>

      <div style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column" }}>
        {!selectedCommitSha ? (
          <EmptyState title={t("git.selectCommit")} hint={t("git.selectCommitHint")} />
        ) : !detail ? (
          <div style={{ padding: 16, color: "var(--text-dim)", fontSize: 12 }}>{t("git.loading")}</div>
        ) : (
          <>
            <div style={{ flexShrink: 0, padding: "10px 14px", borderBottom: "1px solid var(--border)", background: "var(--bg)" }}>
              <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text)" }}>{detail.subject}</div>
              <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginTop: 5, color: "var(--text-dim)", fontSize: 10, fontFamily: "var(--font-mono)" }}>
                <span>{detail.shortSha}</span>
                <span>{detail.author}</span>
                <span>{formatDate(detail.date)}</span>
                <span style={{ color: "#4ade80" }}>+{detail.additions}</span>
                <span style={{ color: "#f87171" }}>-{detail.deletions}</span>
              </div>
              {detail.body && detail.body.trim() !== detail.subject && (
                <pre style={{ margin: "8px 0 0", color: "var(--text-muted)", fontFamily: "var(--font-mono)", fontSize: 11, lineHeight: 1.6, whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
                  {detail.body}
                </pre>
              )}
            </div>
            <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: isMobile ? "column" : "row" }}>
              <div
                style={{
                  width: isMobile ? "100%" : 260,
                  minWidth: isMobile ? 0 : 260,
                  maxHeight: isMobile ? 160 : undefined,
                  overflow: "auto",
                  borderRight: isMobile ? undefined : "1px solid var(--border)",
                  borderBottom: isMobile ? "1px solid var(--border)" : undefined,
                  background: "var(--bg-panel)",
                  flexShrink: 0,
                }}
              >
                {detail.files.map((file) => {
                  const selected = selectedFile?.path === file.path;
                  return (
                    <button
                      key={file.path}
                      type="button"
                      onClick={() => onSelectFile(file)}
                      title={file.path}
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 7,
                        width: "100%",
                        minHeight: 28,
                        padding: "3px 8px 3px 10px",
                        border: "none",
                        borderLeft: selected ? "2px solid var(--accent)" : "2px solid transparent",
                        background: selected ? "var(--bg-selected)" : "transparent",
                        color: "var(--text)",
                        cursor: "pointer",
                        textAlign: "left",
                        fontSize: 11,
                      }}
                    >
                      <span style={{ width: 13, flexShrink: 0, color: statusColor(file.status), fontFamily: "var(--font-mono)", fontWeight: 700 }}>{statusLetter(file.status)}</span>
                      <span style={{ minWidth: 0, flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{relativePath(root, file.path)}</span>
                      <span style={{ flexShrink: 0, color: "var(--text-dim)", fontFamily: "var(--font-mono)", fontSize: 9 }}>+{file.additions} -{file.deletions}</span>
                    </button>
                  );
                })}
              </div>
              <div style={{ flex: 1, minHeight: 0 }}>
                <GitDiffView
                  patch={commitDiff}
                  emptyLabel={selectedFile?.binary ? t("git.binaryDiff") : t("git.noTextDiff")}
                />
              </div>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

/** The project-scoped Git browser shown in the centre column. */
export function GitWorkspace({ projectRoot, state, active, isMobile, onChange }: Props) {
  const { t } = useI18n();
  const [repos, setRepos] = useState<GitRepoSummary[]>([]);
  const [reposLoading, setReposLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [refreshToken, setRefreshToken] = useState(0);
  const [status, setStatus] = useState<GitStatusResponse | null>(null);
  const [statusLoading, setStatusLoading] = useState(false);
  const [diff, setDiff] = useState<GitFileDiffResponse | null>(null);
  const [commits, setCommits] = useState<GitHistoryCommit[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [detail, setDetail] = useState<GitCommitDetail | null>(null);
  const [commitDiff, setCommitDiff] = useState<string | null>(null);
  const stateRef = useRef(state);
  stateRef.current = state;

  const update = useCallback((next: Partial<GitWorkspaceState>) => {
    const current = stateRef.current;
    onChange({ ...current, ...next });
  }, [onChange]);

  // Scan repositories whenever the project or refresh button changes.
  useEffect(() => {
    if (!active || !projectRoot) {
      setRepos([]);
      return;
    }
    const controller = new AbortController();
    setReposLoading(true);
    setError(null);
    fetch(`/api/git/repos?root=${encodeURIComponent(projectRoot)}&depth=3`, { signal: controller.signal })
      .then((response) => readJson<{ repos: GitRepoSummary[] }>(response, t("git.loadReposFailed")))
      .then((data) => {
        const list = data.repos ?? [];
        setRepos(list);
        const current = stateRef.current;
        const selected = current.selectedRepoPath && list.some((repo) => repo.path === current.selectedRepoPath)
          ? current.selectedRepoPath
          : list.find((repo) => repo.isCurrent)?.path ?? list[0]?.path ?? null;
        if (selected !== current.selectedRepoPath) {
          update({
            selectedRepoPath: selected,
            selectedFilePath: null,
            selectedFileScope: "all",
            selectedCommitSha: null,
            selectedCommitFilePath: null,
          });
        }
      })
      .catch((reason) => {
        if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => {
        if (!controller.signal.aborted) setReposLoading(false);
      });
    return () => controller.abort();
  }, [active, projectRoot, refreshToken, t, update]);

  // Refresh the selected repository's status for the Changes tab.
  useEffect(() => {
    if (!active || !state.selectedRepoPath || state.tab !== "changes") return;
    const controller = new AbortController();
    setStatusLoading(true);
    fetch(`/api/git/status?cwd=${encodeURIComponent(state.selectedRepoPath)}`, { signal: controller.signal })
      .then((response) => readJson<GitStatusResponse>(response, t("git.loadStatusFailed")))
      .then((next) => {
        setStatus(next);
        setError(null);
      })
      .catch((reason) => {
        if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => {
        if (!controller.signal.aborted) setStatusLoading(false);
      });
    return () => controller.abort();
  }, [active, state.selectedRepoPath, state.tab, refreshToken, t]);

  // Load the selected file's working-tree diff, scoped to its group.
  useEffect(() => {
    if (!active || !state.selectedRepoPath || !state.selectedFilePath || state.tab !== "changes") {
      setDiff(null);
      return;
    }
    const controller = new AbortController();
    const params = new URLSearchParams({
      cwd: state.selectedRepoPath,
      path: state.selectedFilePath,
      scope: state.selectedFileScope,
    });
    fetch(`/api/git/diff?${params.toString()}`, { signal: controller.signal })
      .then((response) => readJson<GitFileDiffResponse>(response, t("git.loadDiffFailed")))
      .then(setDiff)
      .catch(() => {
        if (!controller.signal.aborted) setDiff({ supported: false });
      });
    return () => controller.abort();
  }, [active, state.selectedRepoPath, state.selectedFilePath, state.selectedFileScope, state.tab, refreshToken, t]);

  // Load history for the selected repository.
  useEffect(() => {
    if (!active || !state.selectedRepoPath || state.tab !== "history") return;
    const controller = new AbortController();
    setHistoryLoading(true);
    fetch(`/api/git/history?cwd=${encodeURIComponent(state.selectedRepoPath)}&limit=50&skip=0`, { signal: controller.signal })
      .then((response) => readJson<{ commits: GitHistoryCommit[] }>(response, t("git.loadHistoryFailed")))
      .then((data) => {
        setCommits(data.commits ?? []);
        setError(null);
        const current = stateRef.current;
        if (current.selectedCommitSha && !data.commits.some((commit) => commit.sha === current.selectedCommitSha)) {
          update({ selectedCommitSha: null, selectedCommitFilePath: null });
        }
      })
      .catch((reason) => {
        if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => {
        if (!controller.signal.aborted) setHistoryLoading(false);
      });
    return () => controller.abort();
  }, [active, state.selectedRepoPath, state.tab, refreshToken, t, update]);

  // Load the selected commit's metadata and file list.
  useEffect(() => {
    if (!active || !state.selectedRepoPath || !state.selectedCommitSha || state.tab !== "history") {
      setDetail(null);
      return;
    }
    const controller = new AbortController();
    setDetail(null);
    const params = new URLSearchParams({
      cwd: state.selectedRepoPath,
      sha: state.selectedCommitSha,
    });
    fetch(`/api/git/commit?${params.toString()}`, { signal: controller.signal })
      .then((response) => readJson<GitCommitDetail>(response, t("git.loadCommitFailed")))
      .then((next) => {
        setDetail(next);
        const current = stateRef.current;
        if (next.files.length > 0 && !next.files.some((file) => file.path === current.selectedCommitFilePath)) {
          update({ selectedCommitFilePath: next.files[0].path });
        }
      })
      .catch((reason) => {
        if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => controller.abort();
  }, [active, state.selectedRepoPath, state.selectedCommitSha, state.tab, refreshToken, t, update]);

  const selectedCommitFile = detail?.files.find((file) => file.path === state.selectedCommitFilePath)
    ?? detail?.files[0]
    ?? null;

  // Load the selected commit file's patch.
  useEffect(() => {
    if (!active || !state.selectedRepoPath || !state.selectedCommitSha || !selectedCommitFile || state.tab !== "history") {
      setCommitDiff(null);
      return;
    }
    const controller = new AbortController();
    const params = new URLSearchParams({
      cwd: state.selectedRepoPath,
      sha: state.selectedCommitSha,
      path: selectedCommitFile.path,
    });
    if (selectedCommitFile.oldPath) params.set("oldPath", selectedCommitFile.oldPath);
    fetch(`/api/git/commit-diff?${params.toString()}`, { signal: controller.signal })
      .then((response) => readJson<{ patch: string }>(response, t("git.loadDiffFailed")))
      .then((data) => setCommitDiff(data.patch ?? null))
      .catch(() => {
        if (!controller.signal.aborted) setCommitDiff(null);
      });
    return () => controller.abort();
  }, [active, state.selectedRepoPath, state.selectedCommitSha, selectedCommitFile, state.tab, t]);

  // Refresh when the window returns to the foreground while the Git tab is open.
  useEffect(() => {
    if (!active) return;
    const refresh = () => setRefreshToken((token) => token + 1);
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [active]);

  const selectedRepo = repos.find((repo) => repo.path === state.selectedRepoPath) ?? null;
  const dirtyCount = selectedRepo
    ? selectedRepo.staged + selectedRepo.unstaged + selectedRepo.untracked + selectedRepo.conflicts
    : 0;

  if (!projectRoot) {
    return <EmptyState title={t("git.selectProject")} hint={t("git.selectProjectHint")} />;
  }

  return (
    <div style={{ width: "100%", height: "100%", minWidth: 0, minHeight: 0, flex: 1, display: "flex", flexDirection: "column", background: "var(--bg)" }}>
      <div
        className="git-workspace-toolbar"
        style={{
          display: "flex",
          alignItems: "stretch",
          height: BAR_HEIGHT,
          minWidth: 0,
          flexShrink: 0,
          borderBottom: "1px solid var(--border)",
          background: "var(--bg-panel)",
        }}
      >
        {/* The two panes' tabs are the left rail's header: their combined width
            matches the file/commit rail below them. */}
        <div
          role="tablist"
          style={{
            display: "flex",
            alignItems: "stretch",
            width: isMobile ? 160 : FILE_RAIL_WIDTH,
            minWidth: isMobile ? 160 : FILE_RAIL_WIDTH,
            flexShrink: 0,
            borderRight: "1px solid var(--border)",
          }}
        >
          {(["changes", "history"] as GitTab[]).map((tab) => {
            const selected = state.tab === tab;
            return (
              <button
                key={tab}
                type="button"
                role="tab"
                aria-selected={selected}
                onClick={() => update({ tab })}
                style={{
                  position: "relative",
                  flex: 1,
                  height: "100%",
                  padding: "0 8px",
                  border: "none",
                  background: selected ? "var(--bg-selected)" : "transparent",
                  color: selected ? "var(--text)" : "var(--text-muted)",
                  cursor: "pointer",
                  fontSize: 11,
                  fontWeight: selected ? 600 : 500,
                }}
              >
                {tab === "changes" ? t("git.changes") : t("git.history")}
                {tab === "changes" && dirtyCount > 0 && (
                  <span style={{ marginLeft: 6, color: "var(--text-dim)", fontFamily: "var(--font-mono)", fontSize: 10 }}>{dirtyCount}</span>
                )}
                {selected && <span style={{ position: "absolute", left: 0, right: 0, bottom: 0, height: 2, background: "var(--accent)" }} />}
              </button>
            );
          })}
        </div>

        <div
          className="git-repo-controls"
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "flex-end",
            gap: 8,
            flex: 1,
            minWidth: 0,
            padding: "0 8px",
            overflow: "hidden",
          }}
        >
          <select
            className="git-repo-select"
            aria-label={t("git.repository")}
            value={state.selectedRepoPath ?? ""}
            disabled={repos.length === 0}
            onChange={(event) => update({
              selectedRepoPath: event.target.value || null,
              selectedFilePath: null,
              selectedFileScope: "all",
              selectedCommitSha: null,
              selectedCommitFilePath: null,
            })}
            style={{ maxWidth: isMobile ? 140 : 260 }}
          >
            {repos.length === 0 && <option value="">{reposLoading ? t("git.loading") : t("git.noRepos")}</option>}
            {repos.map((repo) => {
              const count = repo.staged + repo.unstaged + repo.untracked + repo.conflicts;
              const label = `${repo.name}${repo.relativePath && repo.relativePath !== repo.name ? ` (${repo.relativePath})` : ""}${count ? ` · ${count}` : ""}`;
              return <option key={repo.path} value={repo.path}>{label}</option>;
            })}
          </select>

          {!isMobile && selectedRepo && (
            <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0, color: "var(--text-muted)", fontSize: 10, fontFamily: "var(--font-mono)" }}>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 4, maxWidth: 220, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <circle cx="6" cy="6" r="2.5" /><circle cx="6" cy="18" r="2.5" /><circle cx="18" cy="6" r="2.5" />
                  <path d="M6 8.5v7M8.5 6h4a5.5 5.5 0 0 1 5.5 5.5v.5" />
                </svg>
                {selectedRepo.branch ?? t("git.detached")}
              </span>
              {selectedRepo.ahead > 0 && <span style={{ color: "#4ade80" }}>↑{selectedRepo.ahead}</span>}
              {selectedRepo.behind > 0 && <span style={{ color: "#f87171" }}>↓{selectedRepo.behind}</span>}
              {dirtyCount > 0 && <span style={{ color: "var(--text)" }}>● {dirtyCount}</span>}
            </div>
          )}
          {isMobile && dirtyCount > 0 && (
            <span style={{ flexShrink: 0, color: "var(--text)", fontSize: 10, fontFamily: "var(--font-mono)" }}>● {dirtyCount}</span>
          )}

          {error && <span title={error} style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: "#f87171", fontSize: 10 }}>{error}</span>}
          <IconButton label={t("git.refresh")} onClick={() => setRefreshToken((token) => token + 1)}>
            <RefreshIcon />
          </IconButton>
        </div>
      </div>

      {repos.length === 0 && !reposLoading ? (
        <EmptyState title={t("git.noRepos")} hint={t("git.noReposHint")} />
      ) : state.tab === "changes" ? (
        <ChangesView
          root={state.selectedRepoPath}
          status={status}
          selectedFile={state.selectedFilePath ? { path: state.selectedFilePath, scope: state.selectedFileScope } : null}
          diff={diff}
          loading={statusLoading}
          isMobile={isMobile}
          t={t}
          onSelectFile={(path, scope) => update({ selectedFilePath: path, selectedFileScope: scope })}
        />
      ) : (
        <HistoryView
          root={state.selectedRepoPath}
          commits={commits}
          detail={detail}
          selectedCommitSha={state.selectedCommitSha}
          selectedFilePath={state.selectedCommitFilePath ?? selectedCommitFile?.path ?? null}
          commitDiff={commitDiff}
          loading={historyLoading}
          isMobile={isMobile}
          t={t}
          onSelectCommit={(sha) => update({ selectedCommitSha: sha, selectedCommitFilePath: null })}
          onSelectFile={(file) => update({ selectedCommitFilePath: file.path })}
        />
      )}
    </div>
  );
}
