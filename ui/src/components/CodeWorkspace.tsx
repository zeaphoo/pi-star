"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { FileExplorer, type FileExplorerHandle } from "./FileExplorer";
import { FileViewer } from "./FileViewer";
import { TabBar } from "./TabBar";
import { useI18n } from "@/hooks/useI18n";
import { useResizablePanel } from "@/hooks/useResizablePanel";
import { getFileName } from "@/lib/file-paths";
import { movePanelTab } from "@/lib/panel-tab-order";
import { saveFileViewerState } from "./file-tab-state";
import { loadShowIgnoredFiles, saveShowIgnoredFiles } from "@/lib/sidebar-prefs";
import { EyeIcon, SearchIcon, UploadIcon } from "./SidebarIcons";
import type { CodeWorkspaceState } from "@/lib/code-workspace";
import { closeCodeTab } from "@/lib/code-workspace";
import type { FileViewerState } from "@/lib/file-viewer-state";

interface Props {
  root: string | null;
  projectKey: string | null;
  projectName: string | null;
  state: CodeWorkspaceState | null;
  active: boolean;
  isMobile: boolean;
  refreshKey?: number;
  onOpenFile: (filePath: string, fileName: string, page?: number) => void;
  onChange: (state: CodeWorkspaceState) => void;
  onAtMention?: (relativePath: string, isDir: boolean) => void;
  onMentionLines?: (relativePath: string, startLine: number, endLine: number) => void;
}

const TREE_OPEN_KEY = "pi-star:code-tree-open";
const TREE_DEFAULT_WIDTH = 260;
const TREE_MIN_WIDTH = 200;
const TREE_MAX_WIDTH = 420;

function readTreeOpen(): boolean {
  if (typeof window === "undefined") return true;
  try {
    const stored = window.localStorage.getItem(TREE_OPEN_KEY);
    return stored === null ? true : stored === "1";
  } catch {
    return true;
  }
}

function IconButton({
  label,
  onClick,
  children,
  pressed,
  disabled = false,
}: {
  label: string;
  onClick: () => void;
  children: ReactNode;
  pressed?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={pressed}
      disabled={disabled}
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
        background: pressed ? "var(--bg-selected)" : "transparent",
        color: pressed ? "var(--text)" : "var(--text-muted)",
        cursor: disabled ? "default" : "pointer",
        opacity: disabled ? 0.45 : 1,
        flexShrink: 0,
      }}
      onMouseEnter={(event) => { if (!disabled) event.currentTarget.style.color = "var(--text)"; }}
      onMouseLeave={(event) => { event.currentTarget.style.color = pressed ? "var(--text)" : "var(--text-muted)"; }}
    >
      {children}
    </button>
  );
}

function TreeIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 4h6v6H4zM14 14h6v6h-6zM14 4h6v6h-6z" />
      <path d="M10 7h4M7 10v7h7" />
    </svg>
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

function CodeEmptyState({ projectName }: { projectName: string | null }) {
  const { t } = useI18n();
  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 8,
        padding: 24,
        color: "var(--text-muted)",
        textAlign: "center",
      }}
    >
      <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.65 }} aria-hidden="true">
        <path d="m8 6-6 6 6 6" />
        <path d="m16 6 6 6-6 6" />
      </svg>
      <div style={{ fontSize: 14, color: "var(--text)" }}>
        {projectName ? t("code.noFileOpen") : t("code.selectProject")}
      </div>
      {projectName && <div style={{ fontSize: 12 }}>{t("code.noFileHint")}</div>}
    </div>
  );
}

/**
 * The project-scoped code browser rendered in the centre column while the
 * chat view is hidden. The session sidebar stays mounted beside it.
 */
export function CodeWorkspace({
  root,
  projectKey,
  projectName,
  state,
  active,
  isMobile,
  refreshKey = 0,
  onOpenFile,
  onChange,
  onAtMention,
  onMentionLines,
}: Props) {
  const { t } = useI18n();
  const [treeOpen, setTreeOpen] = useState(readTreeOpen);
  const [mobileTreeOpen, setMobileTreeOpen] = useState(false);
  const treeVisible = isMobile ? mobileTreeOpen : treeOpen;
  const setTreeVisible = useCallback((next: boolean) => {
    if (isMobile) setMobileTreeOpen(next);
    else setTreeOpen(next);
  }, [isMobile]);
  const [treeRefreshKey, setTreeRefreshKey] = useState(0);
  // The tree's own keys, as the sidebar's files panel had them: what it
  // lists (ignored files too), its search, and an upload into the folder.
  const [showHidden, setShowHidden] = useState(loadShowIgnoredFiles);
  const [fileSearchOpen, setFileSearchOpen] = useState(false);
  const [uploadBusy, setUploadBusy] = useState(false);
  const explorerRef = useRef<FileExplorerHandle>(null);
  const treeWidthRef = useRef(TREE_DEFAULT_WIDTH);
  const treeResizer = useResizablePanel({
    ariaLabel: t("code.resizeTree"),
    cssVariable: "--code-tree-width",
    defaultWidth: TREE_DEFAULT_WIDTH,
    getMaxWidth: () => Math.min(TREE_MAX_WIDTH, Math.max(TREE_MIN_WIDTH, window.innerWidth - 360)),
    growthDirection: "right",
    maxWidth: TREE_MAX_WIDTH,
    minWidth: TREE_MIN_WIDTH,
    storageKey: "pi-star:code-tree-width",
    widthRef: treeWidthRef,
  });

  useEffect(() => {
    try {
      window.localStorage.setItem(TREE_OPEN_KEY, treeOpen ? "1" : "0");
    } catch {
      // Storage is best-effort.
    }
  }, [treeOpen]);

  const activeTab = useMemo(
    () => state?.tabs.find((tab) => tab.id === state.activeTabId) ?? null,
    [state],
  );

  const handleStateChange = useCallback((viewerState: FileViewerState) => {
    if (!state || !activeTab) return;
    const tabs = saveFileViewerState(
      state.tabs,
      activeTab.id,
      activeTab.viewerRevision ?? 0,
      viewerState,
    );
    onChange({ ...state, tabs });
  }, [activeTab, onChange, state]);

  const handleCloseTab = useCallback((tabId: string) => {
    if (!state) return;
    onChange(closeCodeTab(state, tabId));
  }, [onChange, state]);

  const handleReorderTab = useCallback((source: string, target: string, after: boolean) => {
    if (!state) return;
    const order = movePanelTab(state.tabs.map((tab) => tab.id), source, target, after);
    const byId = new Map(state.tabs.map((tab) => [tab.id, tab]));
    onChange({
      ...state,
      tabs: order.map((id) => byId.get(id)).filter((tab): tab is NonNullable<typeof tab> => Boolean(tab)),
    });
  }, [onChange, state]);

  const openFile = useCallback((filePath: string, fileName?: string, page?: number) => {
    onOpenFile(filePath, fileName ?? getFileName(filePath), page);
  }, [onOpenFile]);

  const tree = treeVisible ? (
    <div
      ref={treeResizer.panelRef}
      className="code-workspace-tree"
      style={{
        width: treeResizer.width,
        flexShrink: 0,
        display: "flex",
        flexDirection: "column",
        minWidth: 0,
        background: "var(--bg-panel)",
        ...(isMobile ? {
          position: "absolute",
          top: 0,
          bottom: 0,
          left: 0,
          width: "min(320px, 86vw)",
          zIndex: 32,
          boxShadow: "8px 0 28px rgba(0,0,0,0.22)",
        } : null),
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 6,
          height: 34,
          padding: "0 8px",
          boxSizing: "border-box",
          borderBottom: "1px solid var(--border)",
          flexShrink: 0,
        }}
      >
        <span
          title={root ?? undefined}
          style={{
            flex: 1,
            minWidth: 0,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            fontSize: 12,
            fontWeight: 600,
            color: "var(--text)",
          }}
        >
          {projectName ?? t("code.files")}
        </span>
        <IconButton
          label={t("sidebar.uploadFilesTitle")}
          disabled={!root || uploadBusy}
          onClick={() => explorerRef.current?.openUploadPicker()}
        >
          <UploadIcon size={14} />
        </IconButton>
        <IconButton
          label={t("sidebar.showIgnoredFiles")}
          pressed={showHidden}
          onClick={() => {
            const next = !showHidden;
            setShowHidden(next);
            saveShowIgnoredFiles(next);
          }}
        >
          <EyeIcon size={14} />
        </IconButton>
        <IconButton
          label={t("sidebar.searchFiles")}
          pressed={fileSearchOpen}
          onClick={() => setFileSearchOpen((open) => !open)}
        >
          <SearchIcon size={14} />
        </IconButton>
        <IconButton
          label={t("code.hideTree")}
          onClick={() => setTreeVisible(false)}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="m15 18-6-6 6-6" />
          </svg>
        </IconButton>
      </div>
      <div className="scrollbar-subtle" style={{ flex: 1, minHeight: 0, overflow: "auto" }}>
        {root && (
          <FileExplorer
            ref={explorerRef}
            cwd={root}
            onOpenFile={(filePath, fileName) => openFile(filePath, fileName)}
            refreshKey={refreshKey + treeRefreshKey}
            changesCollapsed
            onAtMention={onAtMention}
            onUploadBusyChange={setUploadBusy}
            fileSearchOpen={fileSearchOpen}
            onFileSearchOpenChange={setFileSearchOpen}
            showHidden={showHidden}
          />
        )}
      </div>
    </div>
  ) : null;

  return (
    <div
      className="code-workspace"
      style={{
        position: "relative",
        display: "flex",
        width: "100%",
        height: "100%",
        minHeight: 0,
        overflow: "hidden",
        background: "var(--bg)",
      }}
    >
      {tree}
      {treeVisible && !isMobile && (
        <div
          {...treeResizer.separatorProps}
          className={`panel-resize-handle code-tree-resize-handle${treeResizer.isResizing ? " is-resizing" : ""}`}
          data-resize-handle="code-tree"
          title={`${t("code.resizeTree")}: ${t("layout.resizeHint")}`}
        />
      )}
      {treeVisible && isMobile && (
        <div
          aria-hidden="true"
          onClick={() => setTreeVisible(false)}
          style={{ position: "absolute", inset: 0, zIndex: 31, background: "rgba(0,0,0,0.32)" }}
        />
      )}

      <div style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column" }}>
        <div
          className="code-workspace-toolbar"
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            height: 34,
            padding: "0 8px",
            boxSizing: "border-box",
            borderBottom: "1px solid var(--border)",
            background: "var(--bg-panel)",
            flexShrink: 0,
          }}
        >
          <IconButton
            label={treeVisible ? t("code.hideTree") : t("code.showTree")}
            onClick={() => setTreeVisible(!treeVisible)}
            pressed={treeVisible}
          >
            <TreeIcon />
          </IconButton>
          <span
            title={root ?? undefined}
            style={{
              flex: 1,
              minWidth: 0,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              fontSize: 11,
              color: "var(--text-muted)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {projectName ?? ""}
            {root && projectName ? <span style={{ color: "var(--text-dim)" }}> · {root}</span> : null}
          </span>
          <IconButton label={t("code.refresh")} onClick={() => setTreeRefreshKey((key) => key + 1)}>
            <RefreshIcon />
          </IconButton>
        </div>

        {state && state.tabs.length > 0 && (
          <div style={{ display: "flex", alignItems: "stretch", flexShrink: 0, borderBottom: "1px solid var(--border)" }}>
            <div style={{ flex: 1, overflow: "hidden", minWidth: 0 }}>
              <TabBar
                tabs={state.tabs}
                activeTabId={state.activeTabId ?? ""}
                onSelectTab={(id) => onChange({ ...state, activeTabId: id })}
                onCloseTab={handleCloseTab}
                onReorderTab={handleReorderTab}
              />
            </div>
          </div>
        )}

        <div style={{ flex: 1, minHeight: 0, overflow: "hidden" }}>
          {activeTab ? (
            <FileViewer
              key={`${projectKey ?? root}:${activeTab.id}:${activeTab.viewerRevision ?? 0}`}
              filePath={activeTab.filePath}
              cwd={root ?? undefined}
              sourceSessionId={null}
              gitRefreshKey={refreshKey + treeRefreshKey}
              initialDisplayMode={activeTab.initialDisplayMode}
              initialPage={activeTab.page}
              initialState={activeTab.viewerState}
              watchEnabled={active}
              onStateChange={handleStateChange}
              onMentionLines={onMentionLines}
              onAtMention={onAtMention}
              onOpenFile={(filePath, page) => openFile(filePath, getFileName(filePath), page)}
            />
          ) : (
            <CodeEmptyState projectName={projectName} />
          )}
        </div>
      </div>
    </div>
  );
}
