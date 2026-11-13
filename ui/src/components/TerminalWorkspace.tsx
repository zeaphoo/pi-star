"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "@/hooks/useI18n";
import { useTerminalTheme } from "@/hooks/useTerminalTheme";
import { openStackedDialog } from "@/lib/stacked-dialog";
import {
  closeTerminalPane,
  collectTerminalIds,
  countTerminalPanes,
  emptyTerminalWorkspace,
  findTerminalPane,
  maxTerminalPanes,
  maxTerminalTabs,
  newTerminalWorkspaceTab,
  replaceTerminalPaneId,
  resizeTerminalSplit,
  setTerminalActivePane,
  setTerminalPaneCwd,
  setTerminalPaneRestored,
  splitTerminalPane,
  type TerminalPaneState,
  type TerminalSplitDirection,
  type TerminalSplitState,
  type TerminalTreeNode,
  type TerminalWorkspaceState,
  type TerminalWorkspaceTab,
} from "@/lib/terminal-workspace";
import { TerminalPanel, type TerminalStatus } from "./TerminalPanel";

interface Props {
  projectRoot: string | null;
  defaultCwd: string | null;
  state: TerminalWorkspaceState;
  active: boolean;
  isMobile: boolean;
  onChange: (state: TerminalWorkspaceState) => void;
}

const TOOLBAR_HEIGHT = 34;
const SASH_SIZE = 5;
const MIN_PANE_RATIO = 0.12;

function killTerminal(id: string): void {
  void fetch(`/api/terminal/${encodeURIComponent(id)}`, {
    method: "DELETE",
    keepalive: true,
  }).catch(() => {
    // The shell may already have exited; removing the pane still succeeds.
  });
}

function tabLabel(tab: TerminalWorkspaceTab, index: number, t: (key: string, params?: Record<string, string | number>) => string): string {
  return tab.title || t("terminal.tabTitle", { index: index + 1 });
}

function IconButton({
  label,
  title,
  onClick,
  disabled,
  children,
}: {
  label: string;
  title?: string;
  onClick: () => void;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={title ?? label}
      onClick={onClick}
      disabled={disabled}
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        width: 26,
        height: 26,
        padding: 0,
        border: "none",
        borderRadius: 4,
        background: "transparent",
        color: disabled ? "var(--text-dim)" : "var(--text-muted)",
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.45 : 1,
        flexShrink: 0,
      }}
      onMouseEnter={(event) => { if (!disabled) event.currentTarget.style.color = "var(--text)"; }}
      onMouseLeave={(event) => { event.currentTarget.style.color = disabled ? "var(--text-dim)" : "var(--text-muted)"; }}
    >
      {children}
    </button>
  );
}

function SplitIcon({ direction }: { direction: TerminalSplitDirection }) {
  return direction === "row" ? (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="3" y="4" width="18" height="16" rx="1.5" />
      <line x1="12" y1="4" x2="12" y2="20" />
    </svg>
  ) : (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="4" y="3" width="16" height="18" rx="1.5" />
      <line x1="4" y1="12" x2="20" y2="12" />
    </svg>
  );
}

function EmptyTerminal({ hasProject, onOpen, t }: { hasProject: boolean; onOpen: () => void; t: ReturnType<typeof useI18n>["t"] }) {
  return (
    <div style={{ height: "100%", display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 10, padding: 24, color: "var(--text-muted)", textAlign: "center" }}>
      <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.6 }} aria-hidden="true">
        <rect x="3" y="4" width="18" height="16" rx="2" />
        <polyline points="7 9 10 12 7 15" />
        <line x1="13" y1="15" x2="17" y2="15" />
      </svg>
      <div style={{ fontSize: 14, color: "var(--text)" }}>{hasProject ? t("terminal.emptyTitle") : t("terminal.selectProject")}</div>
      <div style={{ fontSize: 12, maxWidth: 420 }}>{hasProject ? t("terminal.emptyHint") : t("terminal.selectProjectHint")}</div>
      {hasProject && (
        <button
          type="button"
          onClick={onOpen}
          style={{ marginTop: 4, padding: "6px 12px", border: "1px solid var(--border)", borderRadius: 5, background: "var(--bg-panel)", color: "var(--text)", cursor: "pointer", fontSize: 12 }}
        >
          {t("terminal.open")}
        </button>
      )}
    </div>
  );
}

function SplitSash({
  direction,
  containerRef,
  index,
  sizes,
  onResize,
}: {
  direction: TerminalSplitDirection;
  containerRef: React.RefObject<HTMLDivElement | null>;
  index: number;
  sizes: number[];
  onResize: (sizes: number[]) => void;
}) {
  const startRef = useRef<{ position: number; sizes: number[] } | null>(null);

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.pointerType === "mouse" && event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    startRef.current = {
      position: direction === "row" ? event.clientX : event.clientY,
      sizes: [...sizes],
    };
  };

  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const start = startRef.current;
    const container = containerRef.current;
    if (!start || !container) return;
    const rect = container.getBoundingClientRect();
    const total = direction === "row" ? rect.width : rect.height;
    if (total <= 0) return;
    const position = direction === "row" ? event.clientX : event.clientY;
    const delta = (position - start.position) / total;
    const first = start.sizes[index] + delta;
    const second = start.sizes[index + 1] - delta;
    if (first < MIN_PANE_RATIO || second < MIN_PANE_RATIO) return;
    const next = [...start.sizes];
    next[index] = first;
    next[index + 1] = second;
    onResize(next);
  };

  const finish = (event: React.PointerEvent<HTMLDivElement>) => {
    startRef.current = null;
    try {
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
    } catch {
      // Capture may already be released.
    }
  };

  return (
    <div
      role="separator"
      aria-orientation={direction === "row" ? "vertical" : "horizontal"}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={finish}
      onPointerCancel={finish}
      onLostPointerCapture={() => { startRef.current = null; }}
      style={{
        flex: `0 0 ${SASH_SIZE}px`,
        alignSelf: "stretch",
        cursor: direction === "row" ? "col-resize" : "row-resize",
        touchAction: "none",
        background: "var(--terminal-split-border)",
        position: "relative",
        zIndex: 1,
      }}
    />
  );
}

function TerminalPane({
  pane,
  active,
  exited,
  onActivate,
  onExpired,
  onCwdChange,
  onStarted,
  onStatusChange,
  onExit,
  onRestart,
  onClose,
  canClose,
  fallbackCwd,
}: {
  pane: TerminalPaneState;
  active: boolean;
  exited: boolean;
  onActivate: () => void;
  onExpired: () => void;
  onCwdChange: (cwd: string) => void;
  onStarted: () => void;
  onStatusChange: (status: TerminalStatus) => void;
  onExit: (exitCode: number) => void;
  onRestart: () => void;
  onClose: () => void;
  canClose: boolean;
  fallbackCwd: string | null;
}) {
  const { t } = useI18n();
  const tab = useMemo(() => ({
    id: pane.terminalId,
    cwd: pane.cwd,
    restored: pane.restored,
  }), [pane.cwd, pane.restored, pane.terminalId]);

  // The actions belong to this pane, not to the window: the old toolbar put
  // the close X at the far right edge, where it read as "close all".
  const actions = (
    <div className="terminal-pane-actions">
      <button
        type="button"
        className="terminal-pane-action"
        title={t("terminal.restart")}
        aria-label={t("terminal.restart")}
        onClick={onRestart}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M20 11a8 8 0 1 0-2.34 5.66" /><polyline points="20 4 20 11 13 11" />
        </svg>
      </button>
      {canClose && (
        <button
          type="button"
          className="terminal-pane-action"
          title={t("terminal.closePane")}
          aria-label={t("terminal.closePane")}
          onClick={onClose}
        >
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
            <line x1="6" y1="6" x2="18" y2="18" /><line x1="18" y1="6" x2="6" y2="18" />
          </svg>
        </button>
      )}
    </div>
  );

  return (
    <div
      className="terminal-pane"
      data-active={active ? "true" : undefined}
      data-exited={exited ? "true" : undefined}
      style={{
        flex: 1,
        minWidth: 0,
        minHeight: 0,
        display: "flex",
        outline: active ? "1px solid var(--terminal-split-active)" : "1px solid var(--terminal-split-border)",
        outlineOffset: active ? -1 : 0,
        background: "var(--terminal-split-bg)",
      }}
    >
      <TerminalPanel
        key={pane.terminalId}
        tab={tab}
        active={active}
        onActivate={onActivate}
        onExpired={onExpired}
        onCwdChange={onCwdChange}
        onStarted={onStarted}
        onStatusChange={onStatusChange}
        onExit={onExit}
        fallbackCwd={fallbackCwd}
        onClosed={() => {}}
        onCloseError={() => {}}
        showExtraKeys={active}
        actions={actions}
      />
    </div>
  );
}

function TerminalNodeView({
  node,
  tab,
  activePaneId,
  paneStatuses,
  onActivatePane,
  onExpiredPane,
  onCwdChangePane,
  onStartedPane,
  onStatusChangePane,
  onExitPane,
  onRestartPane,
  onClosePane,
  canClosePane,
  fallbackCwd,
  onResizeSplit,
}: {
  node: TerminalTreeNode;
  tab: TerminalWorkspaceTab;
  activePaneId: string | null;
  paneStatuses: Record<string, TerminalStatus>;
  onActivatePane: (paneId: string) => void;
  onExpiredPane: (paneId: string) => void;
  onCwdChangePane: (paneId: string, cwd: string) => void;
  onStartedPane: (paneId: string) => void;
  onStatusChangePane: (paneId: string, status: TerminalStatus) => void;
  onExitPane: (paneId: string, exitCode: number) => void;
  onRestartPane: (paneId: string) => void;
  onClosePane: (paneId: string) => void;
  canClosePane: boolean;
  fallbackCwd: string | null;
  onResizeSplit: (splitId: string, sizes: number[]) => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);

  if (node.kind === "pane") {
    return (
      <TerminalPane
        pane={node}
        active={node.id === activePaneId}
        exited={paneStatuses[node.id] === "exited" || paneStatuses[node.id] === "error"}
        onActivate={() => onActivatePane(node.id)}
        onExpired={() => onExpiredPane(node.id)}
        onCwdChange={(cwd) => onCwdChangePane(node.id, cwd)}
        onStarted={() => onStartedPane(node.id)}
        onStatusChange={(status) => onStatusChangePane(node.id, status)}
        onExit={(exitCode) => onExitPane(node.id, exitCode)}
        onRestart={() => onRestartPane(node.id)}
        onClose={() => onClosePane(node.id)}
        canClose={canClosePane}
        fallbackCwd={fallbackCwd}
      />
    );
  }

  const split: TerminalSplitState = node;
  return (
    <div
      ref={containerRef}
      style={{
        flex: 1,
        minWidth: 0,
        minHeight: 0,
        display: "flex",
        flexDirection: split.direction,
      }}
    >
      {split.children.map((child, index) => (
        <div key={child.kind === "pane" ? child.id : child.id} style={{ display: "contents" }}>
          <div
            style={{
              flexGrow: split.sizes[index] ?? 1 / split.children.length,
              flexBasis: 0,
              minWidth: 0,
              minHeight: 0,
              display: "flex",
              overflow: "hidden",
            }}
          >
            <TerminalNodeView
              node={child}
              tab={tab}
              activePaneId={activePaneId}
              paneStatuses={paneStatuses}
              onActivatePane={onActivatePane}
              onExpiredPane={onExpiredPane}
              onCwdChangePane={onCwdChangePane}
              onStartedPane={onStartedPane}
              onStatusChangePane={onStatusChangePane}
              onExitPane={onExitPane}
              onRestartPane={onRestartPane}
              onClosePane={onClosePane}
              canClosePane={canClosePane}
              fallbackCwd={fallbackCwd}
              onResizeSplit={onResizeSplit}
            />
          </div>
          {index < split.children.length - 1 && (
            <SplitSash
              direction={split.direction}
              containerRef={containerRef}
              index={index}
              sizes={split.sizes}
              onResize={(sizes) => onResizeSplit(split.id, sizes)}
            />
          )}
        </div>
      ))}
    </div>
  );
}

/** Project-scoped terminal workspace with multiple tabs and recursive splits. */
export function TerminalWorkspace({ projectRoot, defaultCwd, state, active, isMobile, onChange }: Props) {
  const { t } = useI18n();
  const terminalTheme = useTerminalTheme();
  const autoCreatedForRef = useRef<Set<string>>(new Set());
  const [closingTabId, setClosingTabId] = useState<string | null>(null);
  const [paneStatuses, setPaneStatuses] = useState<Record<string, TerminalStatus>>({});
  const closeDialogRef = useRef<HTMLDivElement>(null);
  const workspace = state ?? emptyTerminalWorkspace(projectRoot ?? "");
  const activeTab = workspace.tabs.find((tab) => tab.id === workspace.activeTabId) ?? null;
  // Two panes can recover from a lost PTY in the same tick. Each callback used
  // to close over the same workspace snapshot, so the later `onChange` wrote
  // back the earlier pane's dead terminal id: one pane recovered, the other
  // stayed blank. Mutations now read and advance this ref, so they stack.
  const workspaceRef = useRef(workspace);
  workspaceRef.current = workspace;

  const commit = useCallback((next: TerminalWorkspaceState) => {
    workspaceRef.current = next;
    onChange(next);
  }, [onChange]);

  const updateTab = useCallback((tabId: string, update: (tab: TerminalWorkspaceTab) => TerminalWorkspaceTab | null) => {
    const current = workspaceRef.current;
    const tabs = current.tabs.flatMap((tab) => {
      if (tab.id !== tabId) return [tab];
      const next = update(tab);
      return next ? [next] : [];
    });
    const activeTabId = tabs.some((tab) => tab.id === current.activeTabId)
      ? current.activeTabId
      : tabs[0]?.id ?? null;
    commit({ ...current, tabs, activeTabId });
  }, [commit]);

  const openTab = useCallback(() => {
    const current = workspaceRef.current;
    if (!defaultCwd || current.tabs.length >= maxTerminalTabs()) return;
    const currentTab = current.tabs.find((tab) => tab.id === current.activeTabId) ?? null;
    const activePane = currentTab?.activePaneId
      ? findTerminalPane(currentTab.root, currentTab.activePaneId)
      : null;
    const cwd = activePane?.cwd ?? currentTab?.cwd ?? defaultCwd;
    const tab = newTerminalWorkspaceTab(cwd);
    commit({ ...current, tabs: [...current.tabs, tab], activeTabId: tab.id });
  }, [commit, defaultCwd]);

  // Opening the Terminal view for the first time should show a shell, but a
  // deliberately emptied workspace stays empty until the user asks for one.
  // The guard is per project, so switching projects can auto-open their shell.
  useEffect(() => {
    if (!active || !defaultCwd) return;
    const key = projectRoot ?? defaultCwd;
    if (autoCreatedForRef.current.has(key)) return;
    autoCreatedForRef.current.add(key);
    if (workspace.tabs.length === 0) openTab();
  }, [active, defaultCwd, openTab, projectRoot, workspace.tabs.length]);

  const selectTab = (tabId: string) => {
    commit({ ...workspaceRef.current, activeTabId: tabId });
  };

  const closeTab = (tabId: string) => {
    const current = workspaceRef.current;
    const tab = current.tabs.find((item) => item.id === tabId);
    if (!tab) return;
    for (const id of collectTerminalIds(tab.root)) killTerminal(id);
    const index = current.tabs.findIndex((item) => item.id === tabId);
    const tabs = current.tabs.filter((item) => item.id !== tabId);
    const nextActive = current.activeTabId === tabId
      ? tabs[Math.min(index, tabs.length - 1)]?.id ?? null
      : current.activeTabId;
    commit({ ...current, tabs, activeTabId: nextActive });
    setClosingTabId(null);
  };

  const requestCloseTab = (tabId: string) => {
    // Closing a tab kills every shell in it. The confirmation is drawn by the
    // app, not by window.confirm, so it follows the same theme and focus rules
    // as the rest of the dialogs.
    setClosingTabId(tabId);
  };

  useEffect(() => {
    if (!closingTabId) return;
    return openStackedDialog(document, closeDialogRef.current, () => setClosingTabId(null));
  }, [closingTabId]);

  const activatePane = (paneId: string) => {
    if (!activeTab) return;
    updateTab(activeTab.id, (tab) => setTerminalActivePane(tab, paneId));
  };

  const splitPane = (direction: TerminalSplitDirection) => {
    if (!activeTab?.activePaneId) return;
    updateTab(activeTab.id, (tab) => splitTerminalPane(tab, tab.activePaneId!, direction));
  };

  const closePane = (paneId: string) => {
    if (!activeTab) return;
    const pane = findTerminalPane(activeTab.root, paneId);
    if (pane) killTerminal(pane.terminalId);
    updateTab(activeTab.id, (tab) => closeTerminalPane(tab, paneId));
  };

  const restartPane = (paneId: string) => {
    if (!activeTab) return;
    const pane = findTerminalPane(activeTab.root, paneId);
    if (pane) killTerminal(pane.terminalId);
    updateTab(activeTab.id, (tab) => replaceTerminalPaneId(tab, paneId));
  };

  // Typing `exit` ends the shell: close that pane, the same way a terminal
  // window does. A non-zero exit is kept on screen for inspection, with the
  // pane actions offering a restart.
  const handlePaneExit = (paneId: string, exitCode: number) => {
    if (exitCode === 0) closePane(paneId);
  };

  // The server lost this PTY (usually an app restart). Keep the pane and its
  // layout, but create a fresh shell in the last known directory instead of
  // painting a dead "Terminal expired or closed" error.
  const recoverPane = (paneId: string) => {
    if (!activeTab) return;
    updateTab(activeTab.id, (tab) => replaceTerminalPaneId(tab, paneId));
  };

  const changePaneCwd = (paneId: string, cwd: string) => {
    if (!activeTab || !cwd) return;
    const pane = findTerminalPane(activeTab.root, paneId);
    if (!pane || pane.cwd === cwd) return;
    updateTab(activeTab.id, (tab) => setTerminalPaneCwd(tab, paneId, cwd));
  };

  const markPaneStarted = (paneId: string) => {
    if (!activeTab) return;
    const pane = findTerminalPane(activeTab.root, paneId);
    if (!pane || pane.restored) return;
    updateTab(activeTab.id, (tab) => setTerminalPaneRestored(tab, paneId, true));
  };

  const resizeSplit = (splitId: string, sizes: number[]) => {
    if (!activeTab) return;
    updateTab(activeTab.id, (tab) => ({
      ...tab,
      root: resizeTerminalSplit(tab.root, splitId, sizes),
    }));
  };

  const changePaneStatus = useCallback((paneId: string, status: TerminalStatus) => {
    setPaneStatuses((current) => current[paneId] === status ? current : { ...current, [paneId]: status });
  }, []);

  const hasProject = Boolean(projectRoot && defaultCwd);
  const maxPanes = activeTab ? countTerminalPanes(activeTab.root) >= maxTerminalPanes() : false;
  const canSplit = Boolean(activeTab?.activePaneId) && !maxPanes;
  const canClosePane = Boolean(activeTab && countTerminalPanes(activeTab.root) > 1);
  const pendingCloseTab = closingTabId
    ? workspace.tabs.find((tab) => tab.id === closingTabId) ?? null
    : null;
  const chrome = terminalTheme === "dark"
    ? { bg: "#111318", border: "#2f3540", active: "#60a5fa" }
    : { bg: "#fbfbfc", border: "#e5e7eb", active: "#2563eb" };

  return (
    <div
      style={{
        width: "100%",
        height: "100%",
        minWidth: 0,
        minHeight: 0,
        flex: 1,
        display: "flex",
        flexDirection: "column",
        background: "var(--terminal-split-bg)",
        "--terminal-split-bg": chrome.bg,
        "--terminal-split-border": chrome.border,
        "--terminal-split-active": chrome.active,
      } as React.CSSProperties}
    >
      <div
        className="terminal-workspace-toolbar"
        style={{
          display: "flex",
          alignItems: "stretch",
          height: TOOLBAR_HEIGHT,
          minWidth: 0,
          flexShrink: 0,
          borderBottom: "1px solid var(--border)",
          background: "var(--bg-panel)",
        }}
      >
        <div
          role="tablist"
          aria-label={t("terminal.title")}
          className="terminal-tabstrip"
          style={{ display: "flex", alignItems: "stretch", flex: 1, minWidth: 0, overflowX: "auto", overflowY: "hidden", scrollbarWidth: "none" }}
        >
          {workspace.tabs.map((tab, index) => {
            const selected = tab.id === workspace.activeTabId;
            const activePane = tab.activePaneId ? findTerminalPane(tab.root, tab.activePaneId) : null;
            const status = activePane ? paneStatuses[activePane.id] ?? "connecting" : "connecting";
            return (
              <div
                key={tab.id}
                role="tab"
                aria-selected={selected}
                tabIndex={selected ? 0 : -1}
                title={tab.cwd}
                onClick={() => selectTab(tab.id)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    selectTab(tab.id);
                  }
                }}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                  minWidth: 96,
                  maxWidth: 180,
                  padding: "0 6px 0 10px",
                  borderRight: "1px solid var(--border)",
                  background: selected ? "var(--bg)" : "var(--bg-panel)",
                  color: selected ? "var(--text)" : "var(--text-muted)",
                  cursor: "pointer",
                  fontSize: 11,
                  whiteSpace: "nowrap",
                  flexShrink: 0,
                }}
              >
                <span style={{ flexShrink: 0, opacity: selected ? 1 : 0.7, display: "flex" }}>
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <polyline points="4 17 10 11 4 5" /><line x1="12" y1="19" x2="20" y2="19" />
                  </svg>
                </span>
                <span className={`terminal-tab-status is-${status}`} title={t(`terminal.${status}`)} />
                <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }} title={tab.cwd}>
                  {tabLabel(tab, index, t)}
                </span>
                <button
                  type="button"
                  onClick={(event) => { event.stopPropagation(); requestCloseTab(tab.id); }}
                  title={t("terminal.closeTab")}
                  aria-label={t("terminal.closeTab")}
                  style={{ display: "flex", alignItems: "center", justifyContent: "center", width: 20, height: 20, padding: 0, border: "none", borderRadius: 3, background: "transparent", color: "var(--text-dim)", cursor: "pointer", flexShrink: 0 }}
                >
                  <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
                    <line x1="2" y1="2" x2="8" y2="8" /><line x1="8" y1="2" x2="2" y2="8" />
                  </svg>
                </button>
              </div>
            );
          })}
          <button
            type="button"
            onClick={openTab}
            disabled={!hasProject || workspace.tabs.length >= maxTerminalTabs()}
            title={t("terminal.newTab")}
            aria-label={t("terminal.newTab")}
            style={{ display: "flex", alignItems: "center", justifyContent: "center", width: 30, minWidth: 30, border: "none", borderRight: "1px solid var(--border)", background: "transparent", color: hasProject && workspace.tabs.length < maxTerminalTabs() ? "var(--text-muted)" : "var(--text-dim)", cursor: hasProject && workspace.tabs.length < maxTerminalTabs() ? "pointer" : "not-allowed", flexShrink: 0 }}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
              <line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" />
            </svg>
          </button>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 2, padding: "0 6px", flexShrink: 0 }}>
          <IconButton label={t("terminal.splitRight")} onClick={() => splitPane("row")} disabled={!canSplit}>
            <SplitIcon direction="row" />
          </IconButton>
          <IconButton label={t("terminal.splitDown")} onClick={() => splitPane("column")} disabled={!canSplit}>
            <SplitIcon direction="column" />
          </IconButton>
        </div>
      </div>

      {!hasProject ? (
        <EmptyTerminal hasProject={false} onOpen={openTab} t={t} />
      ) : !activeTab ? (
        <EmptyTerminal hasProject onOpen={openTab} t={t} />
      ) : (
        <div style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", padding: 1 }}>
          <TerminalNodeView
            node={activeTab.root}
            tab={activeTab}
            activePaneId={activeTab.activePaneId}
            paneStatuses={paneStatuses}
            onActivatePane={activatePane}
            onExpiredPane={recoverPane}
            onCwdChangePane={changePaneCwd}
            onStartedPane={markPaneStarted}
            onStatusChangePane={changePaneStatus}
            onExitPane={handlePaneExit}
            onRestartPane={restartPane}
            onClosePane={closePane}
            canClosePane={canClosePane}
            fallbackCwd={defaultCwd ?? projectRoot}
            onResizeSplit={resizeSplit}
          />
        </div>
      )}

      {pendingCloseTab && (
        <div
          role="presentation"
          className="terminal-close-backdrop"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) setClosingTabId(null);
          }}
        >
          <div
            ref={closeDialogRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="terminal-close-title"
            aria-describedby="terminal-close-description"
            tabIndex={-1}
            className="terminal-close-dialog"
          >
            <div id="terminal-close-title" className="terminal-close-title">
              {t("terminal.closeTabTitle")}
            </div>
            <div id="terminal-close-description" className="terminal-close-body">
              {t("terminal.closeTabBody")}
            </div>
            <div className="terminal-close-actions">
              <button type="button" className="terminal-close-button" onClick={() => setClosingTabId(null)}>
                {t("terminal.cancel")}
              </button>
              <button
                type="button"
                className="terminal-close-button is-primary"
                onClick={() => {
                  const id = closingTabId;
                  setClosingTabId(null);
                  if (id) closeTab(id);
                }}
              >
                {t("terminal.closeTab")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
