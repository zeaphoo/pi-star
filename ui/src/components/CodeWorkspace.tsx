"use client";

import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
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
import { SidebarMenu, type SidebarMenuAnchor, type SidebarMenuItem } from "./SidebarMenu";
import { copyText } from "@/lib/clipboard";
import type { CodeWorkspaceState } from "@/lib/code-workspace";
import {
  closeAllCodeTabs,
  closeCodeTab,
  closeCodeTabsToRight,
  closeOtherCodeTabs,
} from "@/lib/code-workspace";
import type { FileViewerState } from "@/lib/file-viewer-state";

interface Props {
  root: string | null;
  projectKey: string | null;
  projectName: string | null;
  state: CodeWorkspaceState | null;
  active: boolean;
  isMobile: boolean;
  refreshKey?: number;
  /**
   * Namespaces the tree's remembered width and open state. The main view and
   * every dock leaf are separate browsers, so they must not share one key.
   */
  instanceId?: string;
  onOpenFile: (filePath: string, fileName: string, page?: number) => void;
  onChange: (state: CodeWorkspaceState) => void;
  onAtMention?: (relativePath: string, isDir: boolean) => void;
  onMentionLines?: (relativePath: string, startLine: number, endLine: number) => void;
}

const TREE_DEFAULT_WIDTH = 260;
const TREE_MIN_WIDTH = 200;
const TREE_MAX_WIDTH = 420;

function treeOpenKey(instanceId: string | undefined): string {
  return instanceId ? `pi-star:code-tree-open:${instanceId}` : "pi-star:code-tree-open";
}

function treeWidthKey(instanceId: string | undefined): string {
  return instanceId ? `pi-star:code-tree-width:${instanceId}` : "pi-star:code-tree-width";
}

function readTreeOpen(instanceId: string | undefined): boolean {
  // A dock leaf (instanceId) is narrow and meant for looking at a file: it
  // starts with the tree closed and remembers whatever the user chooses.
  const fallback = instanceId === undefined;
  if (typeof window === "undefined") return fallback;
  try {
    const stored = window.localStorage.getItem(treeOpenKey(instanceId));
    return stored === null ? fallback : stored === "1";
  } catch {
    return fallback;
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

function MoreIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" aria-hidden="true">
      <path d="M5 12h.01M12 12h.01M19 12h.01" />
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
  instanceId,
  onOpenFile,
  onChange,
  onAtMention,
  onMentionLines,
}: Props) {
  const { t } = useI18n();
  const [treeOpen, setTreeOpen] = useState(() => readTreeOpen(instanceId));
  const [mobileTreeOpen, setMobileTreeOpen] = useState(false);
  const treeVisible = isMobile ? mobileTreeOpen : treeOpen;
  const setTreeVisible = useCallback((next: boolean) => {
    if (isMobile) setMobileTreeOpen(next);
    else setTreeOpen(next);
  }, [isMobile]);
  const [treeRefreshKey, setTreeRefreshKey] = useState(0);
  const [fileRefreshKey, setFileRefreshKey] = useState(0);
  const [treeRefreshing, setTreeRefreshing] = useState(false);
  const [fileRefreshing, setFileRefreshing] = useState(false);
  const treeRefreshTimer = useRef<number | null>(null);
  const fileRefreshTimer = useRef<number | null>(null);
  // The tree's own keys, as the sidebar's files panel had them: what it
  // lists (ignored files too), its search, and an upload into the folder.
  const [showHidden, setShowHidden] = useState(loadShowIgnoredFiles);
  const [fileSearchOpen, setFileSearchOpen] = useState(false);
  const [uploadBusy, setUploadBusy] = useState(false);
  const explorerRef = useRef<FileExplorerHandle>(null);
  const moreButtonRef = useRef<HTMLSpanElement>(null);
  const [tabMenu, setTabMenu] = useState<{ anchor: SidebarMenuAnchor; opener: HTMLElement | null } | null>(null);
  const treeWidthRef = useRef(TREE_DEFAULT_WIDTH);
  const treeResizer = useResizablePanel({
    ariaLabel: t("code.resizeTree"),
    cssVariable: "--code-tree-width",
    defaultWidth: TREE_DEFAULT_WIDTH,
    getMaxWidth: () => Math.min(TREE_MAX_WIDTH, Math.max(TREE_MIN_WIDTH, window.innerWidth - 360)),
    growthDirection: "right",
    maxWidth: TREE_MAX_WIDTH,
    minWidth: TREE_MIN_WIDTH,
    storageKey: treeWidthKey(instanceId),
    widthRef: treeWidthRef,
  });

  useEffect(() => {
    try {
      window.localStorage.setItem(treeOpenKey(instanceId), treeOpen ? "1" : "0");
    } catch {
      // Storage is best-effort.
    }
  }, [instanceId, treeOpen]);

  const activeTab = useMemo(
    () => state?.tabs.find((tab) => tab.id === state.activeTabId) ?? null,
    [state],
  );

  // An empty browser is unusable while the tree is hidden: there is nothing to
  // look at and no file to choose. Dock leaves start with the tree closed, so
  // open it whenever the last tab closes. Once a file is open, the user's own
  // tree preference is left alone.
  useEffect(() => {
    if (!state || state.tabs.length > 0) return;
    setTreeOpen(true);
  }, [state]);

  // A FileViewer saves its state in an unmount cleanup. That cleanup runs with
  // the viewer's last render, which can be older than the tab list: closing a
  // tab would then write the closed tab back. Read the latest state and name
  // the tab explicitly, so a cleanup for a closed tab changes nothing.
  const stateRef = useRef(state);
  stateRef.current = state;

  const handleStateChange = useCallback((tabId: string, viewerRevision: number, viewerState: FileViewerState) => {
    const current = stateRef.current;
    if (!current || !current.tabs.some((tab) => tab.id === tabId)) return;
    const tabs = saveFileViewerState(current.tabs, tabId, viewerRevision, viewerState);
    onChange({ ...current, tabs });
  }, [onChange]);

  const handleCloseTab = useCallback((tabId: string) => {
    const current = stateRef.current;
    if (!current) return;
    onChange(closeCodeTab(current, tabId));
  }, [onChange]);

  useEffect(() => () => {
    if (treeRefreshTimer.current !== null) window.clearTimeout(treeRefreshTimer.current);
    if (fileRefreshTimer.current !== null) window.clearTimeout(fileRefreshTimer.current);
  }, []);

  const refreshTree = useCallback(() => {
    setTreeRefreshing(true);
    setTreeRefreshKey((key) => key + 1);
    if (treeRefreshTimer.current !== null) window.clearTimeout(treeRefreshTimer.current);
    treeRefreshTimer.current = window.setTimeout(() => setTreeRefreshing(false), 600);
  }, []);

  const refreshFile = useCallback(() => {
    if (!activeTab) return;
    setFileRefreshing(true);
    setFileRefreshKey((key) => key + 1);
    if (fileRefreshTimer.current !== null) window.clearTimeout(fileRefreshTimer.current);
    fileRefreshTimer.current = window.setTimeout(() => setFileRefreshing(false), 600);
  }, [activeTab]);

  const openTabMenu = useCallback((anchor: SidebarMenuAnchor, opener: HTMLElement | null) => {
    setTabMenu({ anchor, opener });
  }, []);

  const openMoreMenu = useCallback(() => {
    const button = moreButtonRef.current?.querySelector<HTMLButtonElement>("button");
    if (!button) return;
    openTabMenu({ kind: "rect", rect: button.getBoundingClientRect(), align: "end" }, button);
  }, [openTabMenu]);

  const handleTabContextMenu = useCallback((tabId: string, event: ReactMouseEvent<HTMLDivElement>) => {
    const current = stateRef.current;
    if (!current) return;
    if (current.activeTabId !== tabId) onChange({ ...current, activeTabId: tabId });
    openTabMenu({ kind: "point", x: event.clientX, y: event.clientY }, event.currentTarget);
  }, [onChange, openTabMenu]);

  const closeAllTabs = useCallback(() => {
    const current = stateRef.current;
    if (current) onChange(closeAllCodeTabs(current));
  }, [onChange]);

  const closeOtherTabs = useCallback(() => {
    const current = stateRef.current;
    if (current?.activeTabId) onChange(closeOtherCodeTabs(current, current.activeTabId));
  }, [onChange]);

  const closeTabsRight = useCallback(() => {
    const current = stateRef.current;
    if (current?.activeTabId) onChange(closeCodeTabsToRight(current, current.activeTabId));
  }, [onChange]);

  const copyActivePath = useCallback(() => {
    const current = stateRef.current;
    const tab = current?.tabs.find((item) => item.id === current.activeTabId);
    if (tab) void copyText(tab.filePath);
  }, []);

  const revealActiveInTree = useCallback(() => {
    const current = stateRef.current;
    const tab = current?.tabs.find((item) => item.id === current.activeTabId);
    if (!tab) return;
    setTreeVisible(true);
    requestAnimationFrame(() => requestAnimationFrame(() => {
      explorerRef.current?.revealPath(tab.filePath);
    }));
  }, [setTreeVisible]);

  const tabMenuItems: SidebarMenuItem[] = useMemo(() => {
    const tabs = state?.tabs ?? [];
    const activeIndex = tabs.findIndex((tab) => tab.id === state?.activeTabId);
    return [
      {
        type: "item",
        id: "close-all",
        label: t("code.closeAllTabs"),
        disabled: tabs.length === 0,
        onSelect: closeAllTabs,
      },
      {
        type: "item",
        id: "close-others",
        label: t("code.closeOtherTabs"),
        disabled: tabs.length <= 1,
        onSelect: closeOtherTabs,
      },
      {
        type: "item",
        id: "close-right",
        label: t("code.closeTabsToRight"),
        disabled: activeIndex === -1 || activeIndex >= tabs.length - 1,
        onSelect: closeTabsRight,
      },
      { type: "separator", id: "separator" },
      {
        type: "item",
        id: "copy-path",
        label: t("code.copyPath"),
        disabled: activeIndex === -1,
        onSelect: copyActivePath,
      },
      {
        type: "item",
        id: "reveal",
        label: t("code.revealInTree"),
        disabled: activeIndex === -1 || !root,
        onSelect: revealActiveInTree,
      },
    ];
  }, [closeAllTabs, closeOtherTabs, closeTabsRight, copyActivePath, revealActiveInTree, root, state, t]);

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
          label={t("code.refreshTree")}
          disabled={!root || treeRefreshing}
          onClick={refreshTree}
        >
          <span style={{ display: "flex", animation: treeRefreshing ? "spin 0.8s linear infinite" : undefined }}>
            <RefreshIcon />
          </span>
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
            alignItems: "stretch",
            height: "var(--top-bar-height)",
            padding: 0,
            boxSizing: "border-box",
            borderBottom: "1px solid var(--border)",
            background: "var(--bg-panel)",
            flexShrink: 0,
          }}
        >
          <div style={{ display: "flex", alignItems: "center", padding: "0 4px 0 6px", flexShrink: 0 }}>
            <IconButton
              label={treeVisible ? t("code.hideTree") : t("code.showTree")}
              onClick={() => setTreeVisible(!treeVisible)}
              pressed={treeVisible}
            >
              <TreeIcon />
            </IconButton>
          </div>
          <div style={{ flex: 1, minWidth: 0, display: "flex", alignItems: "stretch" }}>
            {state && state.tabs.length > 0 ? (
              <TabBar
                tabs={state.tabs}
                activeTabId={state.activeTabId ?? ""}
                onSelectTab={(id) => onChange({ ...state, activeTabId: id })}
                onCloseTab={handleCloseTab}
                onReorderTab={handleReorderTab}
                scrollControls
                onTabContextMenu={handleTabContextMenu}
              />
            ) : <div style={{ flex: 1 }} />}
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 2, padding: "0 6px", flexShrink: 0 }}>
            <IconButton
              label={t("code.refreshFile")}
              disabled={!activeTab || fileRefreshing}
              onClick={refreshFile}
            >
              <span style={{ display: "flex", animation: fileRefreshing ? "spin 0.8s linear infinite" : undefined }}>
                <RefreshIcon />
              </span>
            </IconButton>
            <span ref={moreButtonRef} style={{ display: "flex" }}>
              <IconButton
                label={t("code.tabActions")}
                disabled={!state || state.tabs.length === 0}
                onClick={openMoreMenu}
              >
                <MoreIcon />
              </IconButton>
            </span>
          </div>
        </div>

        <div style={{ flex: 1, minHeight: 0, overflow: "hidden" }}>
          {activeTab ? (
            <FileViewer
              key={`${projectKey ?? root}:${activeTab.id}:${activeTab.viewerRevision ?? 0}`}
              filePath={activeTab.filePath}
              cwd={root ?? undefined}
              sourceSessionId={null}
              refreshKey={refreshKey + fileRefreshKey}
              gitRefreshKey={refreshKey + fileRefreshKey}
              initialDisplayMode={activeTab.initialDisplayMode}
              initialPage={activeTab.page}
              initialState={activeTab.viewerState}
              watchEnabled={active}
              onStateChange={(viewerState) => handleStateChange(
                activeTab.id,
                activeTab.viewerRevision ?? 0,
                viewerState,
              )}
              onMentionLines={onMentionLines}
              onAtMention={onAtMention}
              onOpenFile={(filePath, page) => openFile(filePath, getFileName(filePath), page)}
            />
          ) : (
            <CodeEmptyState projectName={projectName} />
          )}
        </div>
      </div>
      <SidebarMenu
        open={tabMenu !== null}
        anchor={tabMenu?.anchor ?? null}
        sheet={isMobile}
        ariaLabel={t("code.tabActions")}
        title={t("code.tabActions")}
        items={tabMenuItems}
        cancelLabel={t("i18n.cancel")}
        onClose={() => setTabMenu(null)}
        returnFocusTo={tabMenu?.opener ?? null}
        width={220}
      />
    </div>
  );
}
