"use client";

import { useState, useCallback, useRef, useEffect, useLayoutEffect, useMemo } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { useGlobalKeyboardShortcuts } from "@/hooks/useKeyboardShortcuts";
import { SessionSidebar, type SelectSessionOptions, type SessionSidebarControl } from "./SessionSidebar";
import { ChatWindow } from "./ChatWindow";
import { NewSessionContextBar, type NewSessionContextControl } from "./NewSessionContextBar";
import type { ChatScrollPosition } from "@/lib/chat-scroll-position";
import type { Tab } from "./TabBar";
import { MainViewTabs } from "./MainViewTabs";
import { TraceView } from "./TraceView";
import { CodeWorkspace } from "./CodeWorkspace";
import { GitWorkspace } from "./GitWorkspace";
import { TerminalWorkspace } from "./TerminalWorkspace";
import {
  emptyGitWorkspace,
  loadGitWorkspaces,
  saveGitWorkspace,
  type GitWorkspaceState,
} from "@/lib/git-workspace";
import {
  emptyTerminalWorkspace,
  loadTerminalWorkspaces,
  saveTerminalWorkspace,
  type TerminalWorkspaceState,
} from "@/lib/terminal-workspace";
import {
  emptyCodeWorkspace,
  loadCodeWorkspaces,
  loadMainView,
  makeCodeWorkspaceKey,
  openCodeFileTab,
  saveCodeWorkspace,
  saveMainView,
  type CodeWorkspaceState,
  type MainView,
} from "@/lib/code-workspace";
import { agentTabId, agentTabSessionId, openAgentTab, openFileTab, saveFileViewerState } from "./file-tab-state";
import { SettingsPanel, SettingsSectionIcon } from "./SettingsPanel";
import { ProjectTrustDialog, type ProjectTrustFailure } from "./ProjectTrustDialog";
import { BranchNavigator, hasSessionBranches } from "./BranchNavigator";
import { AgentSessionPanel } from "./AgentSessionPanel";
import { DebugPanel, DebugPanelBoundary } from "./DebugPanel";
import { ArtifactsIcon, EMPTY_ARTIFACT_PANE, SessionArtifactsPane, type ArtifactPaneState } from "./SessionArtifactsPane";
import { RightDock } from "./RightDock";
import { DockPanelIcon } from "./dock-icons";
import { useTheme } from "@/hooks/useTheme";
import { useCodeThemePreference } from "@/hooks/useCodeTheme";
import { useFontPreferences } from "@/hooks/useFontPreferences";
import { useFileIconStyle } from "@/hooks/useFileIconStyle";
import { useI18n } from "@/hooks/useI18n";
import { useIsMobile, useIsNarrowMobile, useViewportWidth } from "@/hooks/useIsMobile";
import { useViewportHeight } from "@/hooks/useViewportHeight";
import { useResizablePanel } from "@/hooks/useResizablePanel";
import { useAudio } from "@/hooks/useAudio";
import { copyText } from "@/lib/clipboard";
import { sendAgentCommand } from "@/lib/agent-client";
import { getFileName } from "@/lib/file-paths";
import { buildAtMentionText, buildFileLineMentionText } from "@/lib/file-fuzzy";
import {
  claimExtensionAttentionNotification,
  shouldShowBrowserNotification,
  showBrowserNotification,
} from "@/lib/browser-notifications";
import { startPageVisibilityTracking } from "@/lib/page-debug";
import type { DebugClientSession } from "@/lib/debug-types";
import { getInitialNavigation, linkedSessionId, withTabOpen } from "@/lib/initial-navigation";
import { clearTabOpen, clearTabOpenSession, getTabOpen, setTabOpenNewSession, setTabOpenSession } from "@/lib/tab-session";
import { mergeCatalogRow } from "./session-catalog-helpers";
import { getDraft, parkedNewSessionDraftKey, rekeyDraft } from "@/lib/draft-store";
import {
  contextForCwd,
  type NewSessionContext,
  type NewSessionMove,
  type NewSessionOptions,
  type NewSessionTarget,
  type ProjectChoice,
} from "@/lib/new-session-context";
import {
  clearLastOpen,
  getLastOpenSession,
  setLastOpenSession,
  workspaceKeyOf,
} from "@/lib/workspace-memory";
import {
  getSidebarMaxWidth,
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
} from "@/lib/panel-layout";
import {
  DOCK_DEFAULT_WIDTH,
  DOCK_MAX_WIDTH,
  DOCK_MIN_CHAT_WIDTH,
  DOCK_MIN_VIEWPORT,
  DOCK_MIN_WIDTH,
  emptyDockState,
  ensureDockLeafState,
  loadDockStates,
  saveDockStates,
  type DockContext,
  type DockState,
  type DockStateUpdater,
} from "@/lib/dock-state";
import type { BlockingExtensionUiRequest, SessionInfo, SessionTreeNode } from "@/lib/types";
import type { McpErrorResponse, ProjectTrustStatus } from "@/lib/api-types";
import type { ChatInputHandle } from "./ChatInput";
import type { AgentEndInfo, NewSessionChoices } from "@/hooks/useAgentSession";
import type { SessionStatsInfo } from "@/lib/pi-types";
import { formatActiveDuration, formatCompactCount, formatCostShort } from "@/lib/session-stats-format";
import type { FileViewerState } from "@/lib/file-viewer-state";
import { PRESET_READ_ONLY, type ToolEntry } from "@/lib/tool-presets";
import { getAgentsBadge, getSessionFamily, isHiddenRelation } from "@/lib/session-family";
import { getLastSettingsSection, settingsSectionRequiresProject, type SettingsSection } from "@/lib/settings-navigation";
import { focusIfLost } from "@/lib/stacked-dialog";
import { loadSessionArtifacts, mergeSessionArtifacts, type SessionArtifact } from "@/lib/session-artifacts";

type SessionCopyField = "file" | "id" | "projectDir" | "gitBranch" | "gitWorktree";
type AutoNameStatus =
  | { kind: "idle" }
  | { kind: "naming" }
  | { kind: "success" }
  | { kind: "error"; message: string };

// app/globals.css: 36px, 44px on a phone or touch screen.
const TOP_BAR_ICON_BUTTON_SIZE = "var(--top-bar-height)";
const AGENT_PANEL_WIDTH = 420;
const ARTIFACTS_OPEN_KEY = "pi-star:artifacts-open";
const ARTIFACT_PANES_KEY = "pi-star:artifacts-panes:v1";
const MAX_STORED_ARTIFACT_PANES = 30;
const ARTIFACTS_DEFAULT_WIDTH = 420;
const ARTIFACTS_MIN_WIDTH = 300;
const ARTIFACTS_MAX_WIDTH = 720;
// Below this much room for the chat, the artifacts pane overlays instead of splitting.
const ARTIFACTS_DOCK_MIN_WIDTH = 760;
const DOCK_OPEN_KEY = "pi-star:dock-open";
// pi's built-in tools that never change a file; any other tool may (#1144).
const READ_ONLY_TOOL_NAMES = new Set(PRESET_READ_ONLY);
const TOOL_END_REFRESH_MS = 1000;

/** File tabs parked per session, for the artifacts pane; viewer state is not persisted. */
function loadStoredArtifactPanes(): Record<string, ArtifactPaneState> {
  if (typeof window === "undefined") return {};
  try {
    const raw = window.localStorage.getItem(ARTIFACT_PANES_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const panes: Record<string, ArtifactPaneState> = {};
    for (const [key, value] of Object.entries(parsed)) {
      if (!value || typeof value !== "object" || Array.isArray(value)) continue;
      const entry = value as { tabs?: unknown; activeTabId?: unknown };
      const tabs = Array.isArray(entry.tabs) ? entry.tabs.flatMap((tab): Tab[] => {
        if (!tab || typeof tab !== "object" || Array.isArray(tab)) return [];
        const filePath = typeof (tab as { filePath?: unknown }).filePath === "string" ? (tab as { filePath: string }).filePath : "";
        const id = typeof (tab as { id?: unknown }).id === "string" ? (tab as { id: string }).id : "";
        // Sub-agent tabs are transient: their child sessions need not survive a reload.
        if (!filePath || !id || agentTabSessionId(id)) return [];
        const page = typeof (tab as { page?: unknown }).page === "number" ? (tab as { page: number }).page : undefined;
        const sourceSessionId = typeof (tab as { sourceSessionId?: unknown }).sourceSessionId === "string"
          ? (tab as { sourceSessionId: string }).sourceSessionId
          : null;
        return [{
          id,
          filePath,
          label: typeof (tab as { label?: unknown }).label === "string" ? (tab as { label: string }).label : getFileName(filePath),
          page,
          sourceSessionId,
        }];
      }) : [];
      if (tabs.length === 0) continue;
      const activeTabId = typeof entry.activeTabId === "string" && tabs.some((tab) => tab.id === entry.activeTabId)
        ? entry.activeTabId
        : null;
      panes[key] = { tabs, activeTabId };
    }
    return panes;
  } catch {
    return {};
  }
}

function saveStoredArtifactPanes(panes: Record<string, ArtifactPaneState>): void {
  if (typeof window === "undefined") return;
  try {
    const stored = Object.entries(panes)
      .filter(([key, pane]) => !key.startsWith("new:") && pane.tabs.length > 0)
      .slice(-MAX_STORED_ARTIFACT_PANES)
      .map(([key, pane]) => [key, {
        activeTabId: pane.activeTabId,
        tabs: pane.tabs
          .filter(({ id }) => !agentTabSessionId(id))
          .map(({ id, label, filePath, page, sourceSessionId }) => ({ id, label, filePath, page, sourceSessionId })),
      }] as const);
    window.localStorage.setItem(ARTIFACT_PANES_KEY, JSON.stringify(Object.fromEntries(stored)));
  } catch {
    // Storage is best-effort.
  }
}

export function AppShell() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [mainView, setMainView] = useState<MainView>(() => loadMainView());
  const [codeMounted, setCodeMounted] = useState(() => loadMainView() === "code");
  const [gitMounted, setGitMounted] = useState(() => loadMainView() === "git");
  const [terminalMounted, setTerminalMounted] = useState(() => loadMainView() === "terminal");
  const [traceMounted, setTraceMounted] = useState(() => loadMainView() === "trace");
  const handleMainViewChange = useCallback((next: MainView) => {
    setMainView(next);
    if (next === "trace") setTraceMounted(true);
    if (next === "code") setCodeMounted(true);
    if (next === "git") setGitMounted(true);
    if (next === "terminal") setTerminalMounted(true);
    saveMainView(next);
    if (typeof window !== "undefined") {
      const url = new URL(window.location.href);
      if (next === "code") url.searchParams.set("view", "code");
      else url.searchParams.delete("view");
      router.replace(`${url.pathname}${url.search}`, { scroll: false });
    }
  }, [router]);
  const [initialNavigation, setInitialNavigation] = useState(() => getInitialNavigation(searchParams));
  // Keep the system-theme subscription mounted for the lifetime of the app.
  useTheme();
  // Restore fonts even when Settings and the chat composer have not been opened.
  useFontPreferences();
  // Apply the saved file-icon palette before the tree mounts.
  useFileIconStyle();
  // Keep the code palette (and its cross-tab sync) live for the whole app, not
  // only while Settings is open; index.html restores it before first paint.
  useCodeThemePreference();
  const { locale, t: translate } = useI18n();
  const isMobile = useIsMobile();
  const isNarrowMobile = useIsNarrowMobile();
  useViewportHeight();

  // Audio ownership lives here (not in ChatWindow) so the completion tone can
  // also fire for tasks finishing in a non-active workspace whose ChatWindow
  // is not mounted. ChatWindow receives the audio callbacks as props.
  const { soundEnabled, onSoundToggle, playDoneSound, unlockAudio, holdAudio, soundEnabledRef } = useAudio();
  const [quoteSelectionEnabled, setQuoteSelectionEnabled] = useState(false);
  useEffect(() => {
    try {
      setQuoteSelectionEnabled(localStorage.getItem("pi-quote-selection-enabled") === "true");
    } catch {
      // Browser storage is best-effort.
    }
  }, []);
  const handleQuoteSelectionChange = useCallback((enabled: boolean) => {
    setQuoteSelectionEnabled(enabled);
    try {
      localStorage.setItem("pi-quote-selection-enabled", String(enabled));
    } catch {
      // Keep the current page usable when storage is unavailable.
    }
  }, []);
  const notifiedAttentionRequestIdsRef = useRef(new Set<string>());
  // Sessions that finished while the tab was out of sight (#1484): the browser
  // Notification fires for the selected session, which the sidebar's unread
  // markers exclude ("already looking" — false while hidden). The favicon keeps
  // their badge until the tab is opened again; never persisted, never dotted.
  const [hiddenFinishedSessionIds, setHiddenFinishedSessionIds] = useState<Set<string>>(new Set());
  const handleBackgroundTaskDone = useCallback(() => {
    if (soundEnabledRef.current) playDoneSound();
  }, [playDoneSound, soundEnabledRef]);
  const [selectedSession, setSelectedSession] = useState<SessionInfo | null>(null);
  // Latest selection readable from async callbacks whose captured state is
  // stale (e.g. a delete that completes after the user navigated away).
  const selectedSessionRef = useRef(selectedSession);
  selectedSessionRef.current = selectedSession;
  const [sessionCatalog, setSessionCatalog] = useState<SessionInfo[]>([]);
  const handleSessionsChange = useCallback((sessions: SessionInfo[]) => {
    setSessionCatalog(sessions);
    // The sidebar hydrates metadata after the selected session has already
    // mounted. Merge that update into the active session without changing the
    // ChatWindow key or restarting its history load.
    setSelectedSession((current) => {
      if (!current) return current;
      const refreshed = sessions.find((session) => session.id === current.id);
      return refreshed ? mergeCatalogRow(current, refreshed) : current;
    });
  }, []);
  const sessionsWithSelection = useMemo(() => {
    if (!selectedSession) return sessionCatalog;
    return [
      ...sessionCatalog.filter((session) => session.id !== selectedSession.id),
      selectedSession,
    ];
  }, [selectedSession, sessionCatalog]);
  const activeSessionFamily = useMemo(
    () => getSessionFamily(sessionsWithSelection, selectedSession?.id),
    [selectedSession?.id, sessionsWithSelection],
  );
  const hasSubagentSessions = Boolean(activeSessionFamily?.subagents.length);
  const [runningSessionIds, setRunningSessionIds] = useState<Set<string>>(() => new Set());
  const handleRunningSessionIdsChange = useCallback((ids: Set<string>) => {
    setRunningSessionIds((previous) => {
      if (previous.size === ids.size && [...ids].every((id) => previous.has(id))) return previous;
      return ids;
    });
  }, []);
  const agentsBadge = activeSessionFamily ? getAgentsBadge(activeSessionFamily.subagents, runningSessionIds) : null;
  const agentsButtonLabel = agentsBadge?.running
    ? translate("agentSwitcher.titleRunning", { count: agentsBadge.count })
    : translate("agentSwitcher.title");
  // The temporary id distinguishes consecutive fresh composers in one cwd.
  const [newSessionCwd, setNewSessionCwd] = useState<string | null>(null);
  const [newSessionDraftId, setNewSessionDraftId] = useState("initial");
  const activeNewSessionDraftKeyRef = useRef<string | null>(null);
  // The bar above a fresh composer (NewSessionContextBar): the sidebar reports
  // what it shows and carries out its moves, which remount the composer. Its
  // model and reasoning picks, as it last reported them, go along with the
  // draft to the composer that replaces it, which takes them once.
  const sidebarControlRef = useRef<SessionSidebarControl | null>(null);
  const [sidebarNewSessionContext, setSidebarNewSessionContext] = useState<NewSessionContext | null>(null);
  // The bar's last move until the sidebar reports that cwd (a commit later),
  // so the bar of the new composer shows the target's project, and a worktree
  // it just created, from its first frame.
  const newSessionMoveRef = useRef<NewSessionMove | null>(null);
  const handleSidebarNewSessionContext = useCallback((context: NewSessionContext | null) => {
    if (context && context.cwd === newSessionMoveRef.current?.cwd) newSessionMoveRef.current = null;
    setSidebarNewSessionContext(context);
  }, []);
  const newSessionBarFocusRef = useRef<NewSessionContextControl | null>(null);
  const newSessionChoicesRef = useRef<NewSessionChoices | null>(null);
  const [carriedNewSessionChoices, setCarriedNewSessionChoices] = useState<NewSessionChoices | null>(null);
  const handleNewSessionChoicesChange = useCallback((choices: NewSessionChoices) => {
    newSessionChoicesRef.current = choices;
    setCarriedNewSessionChoices(null);
  }, []);
  const [initialCwdStatus, setInitialCwdStatus] = useState<"idle" | "validating" | "ready" | "error">(
    () => initialNavigation.requestedCwd ? "validating" : "idle",
  );
  const [initialCwdError, setInitialCwdError] = useState<string | null>(null);
  const [initialCwdAttempt, setInitialCwdAttempt] = useState(0);
  const [refreshKey, setRefreshKey] = useState(0);
  const [sessionKey, setSessionKey] = useState(0);
  const sessionScrollPositionsRef = useRef(new Map<string, ChatScrollPosition>());
  const handleSessionScrollPositionChange = useCallback((sessionId: string, position: ChatScrollPosition) => {
    sessionScrollPositionsRef.current.set(sessionId, position);
  }, []);
  const [searchTarget, setSearchTarget] = useState<{ sessionId: string; entryId: string; blockIndex?: number } | null>(null);
  const handleSearchTargetHandled = useCallback((target: { sessionId: string; entryId: string }) => {
    setSearchTarget((current) => current === target ? null : current);
  }, []);
  const [explorerRefreshKey, setExplorerRefreshKey] = useState(0);
  const [settingsSection, setSettingsSection] = useState<SettingsSection | null>(null);
  // The plain Settings button opens a phone's Settings on its list of sections; shortcuts open theirs.
  const [settingsOpenAtHome, setSettingsOpenAtHome] = useState(false);
  const [modelsRefreshKey, setModelsRefreshKey] = useState(0);
  const [projectTrust, setProjectTrust] = useState<ProjectTrustStatus | null>(null);
  const [projectTrustDialogOpen, setProjectTrustDialogOpen] = useState(false);
  const [projectTrustBusy, setProjectTrustBusy] = useState(false);
  const [projectTrustError, setProjectTrustError] = useState<ProjectTrustFailure | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(() => !initialNavigation.sidebarCollapsed);
  const desktopSidebarOpenRef = useRef(!initialNavigation.sidebarCollapsed);
  // The right dock: a project-scoped workbench of independent views. Open and
  // expanded are the user's own preference; the layout and every leaf's state
  // are parked per project (see lib/dock-state.ts).
  const [dockOpen, setDockOpen] = useState(false);
  const [dockExpanded, setDockExpanded] = useState(false);
  const [dockStates, setDockStates] = useState<Record<string, DockState>>(() => loadDockStates());
  const dockPrefsRestoredRef = useRef(false);
  const viewportWidth = useViewportWidth();
  // Whether there is room for the dock at all: it needs its own minimum and
  // must leave the chat one. Set after the sidebar's width is known (below).
  const [canDock, setCanDock] = useState(false);
  useEffect(() => {
    if (!dockOpen || !canDock) setDockExpanded(false);
  }, [canDock, dockOpen]);
  // Whether the dock was open is the user's preference, read after hydration so
  // the server's markup and the first client render agree.
  useEffect(() => {
    try {
      if (window.localStorage.getItem(DOCK_OPEN_KEY) === "1") setDockOpen(true);
    } catch {
      // Storage is best-effort.
    }
    dockPrefsRestoredRef.current = true;
  }, []);
  useEffect(() => {
    if (!dockPrefsRestoredRef.current) return;
    try {
      window.localStorage.setItem(DOCK_OPEN_KEY, dockOpen ? "1" : "0");
    } catch {
      // Storage is best-effort.
    }
  }, [dockOpen]);
  // The session's own artifacts pane: remembered open state, and file tabs
  // parked per session (a switch never carries one session's files to another).
  const [artifactsOpen, setArtifactsOpen] = useState(false);
  const artifactsPrefsRestoredRef = useRef(false);
  const artifactsButtonRef = useRef<HTMLButtonElement>(null);
  const [artifactPanes, setArtifactPanes] = useState<Record<string, ArtifactPaneState>>({});
  const [liveArtifacts, setLiveArtifacts] = useState<SessionArtifact[]>([]);
  const [historyArtifacts, setHistoryArtifacts] = useState<SessionArtifact[]>([]);
  const [artifactsScanning, setArtifactsScanning] = useState(false);
  const artifactScanCacheRef = useRef(new Map<string, SessionArtifact[]>());
  const [mobileToolbarMoreOpen, setMobileToolbarMoreOpen] = useState(false);
  const moreControlsLabel = mobileToolbarMoreOpen ? translate("chat.close") : translate("chat.moreControls");
  const [mobileSidebarReady, setMobileSidebarReady] = useState(false);
  const sidebarWidthRef = useRef(SIDEBAR_DEFAULT_WIDTH);
  const dockWidthRef = useRef(DOCK_DEFAULT_WIDTH);
  const artifactsWidthRef = useRef(ARTIFACTS_DEFAULT_WIDTH);
  const getResponsiveSidebarMaxWidth = useCallback(
    () => typeof window === "undefined"
      ? SIDEBAR_MAX_WIDTH
      : getSidebarMaxWidth({
        viewportWidth: window.innerWidth,
        panelOpen: dockOpen,
        panelWidth: dockWidthRef.current,
      }),
    [dockOpen],
  );
  const sidebarResizer = useResizablePanel({
    ariaLabel: translate("layout.resizeSidebar"),
    cssVariable: "--sidebar-width",
    defaultWidth: SIDEBAR_DEFAULT_WIDTH,
    getMaxWidth: getResponsiveSidebarMaxWidth,
    growthDirection: "right",
    maxWidth: SIDEBAR_MAX_WIDTH,
    minWidth: SIDEBAR_MIN_WIDTH,
    storageKey: "pi-sidebar-width",
    widthRef: sidebarWidthRef,
  });
  useEffect(() => {
    const room = viewportWidth - (sidebarOpen ? sidebarResizer.width : 0);
    setCanDock(!isMobile && viewportWidth >= DOCK_MIN_VIEWPORT && room >= DOCK_MIN_WIDTH + DOCK_MIN_CHAT_WIDTH);
  }, [isMobile, sidebarOpen, sidebarResizer.width, viewportWidth]);
  const dockResizer = useResizablePanel({
    ariaLabel: translate("dock.resize"),
    cssVariable: "--dock-width",
    defaultWidth: DOCK_DEFAULT_WIDTH,
    // The chat keeps a workable column: the dock takes what is left of it.
    getMaxWidth: () => Math.min(
      DOCK_MAX_WIDTH,
      Math.max(DOCK_MIN_WIDTH, window.innerWidth - (sidebarOpen ? sidebarWidthRef.current : 0) - DOCK_MIN_CHAT_WIDTH),
    ),
    growthDirection: "left",
    maxWidth: DOCK_MAX_WIDTH,
    minWidth: DOCK_MIN_WIDTH,
    storageKey: "pi-star:dock-width",
    widthRef: dockWidthRef,
  });
  const artifactsResizer = useResizablePanel({
    ariaLabel: translate("artifacts.resize"),
    cssVariable: "--artifacts-width",
    defaultWidth: ARTIFACTS_DEFAULT_WIDTH,
    getMaxWidth: () => {
      const reserved = (sidebarOpen ? sidebarWidthRef.current : 0)
        + (dockOpen && canDock ? dockWidthRef.current : 0)
        + 360;
      return Math.min(ARTIFACTS_MAX_WIDTH, Math.max(ARTIFACTS_MIN_WIDTH, window.innerWidth - reserved));
    },
    growthDirection: "left",
    maxWidth: ARTIFACTS_MAX_WIDTH,
    minWidth: ARTIFACTS_MIN_WIDTH,
    storageKey: "pi-star:artifacts-width",
    widthRef: artifactsWidthRef,
  });
  const reclampSidebarWidth = sidebarResizer.reclampWidth;

  // Centre the view switch in whatever the groups leave, and drop its labels
  // when that gap gets tight.
  useEffect(() => {
    if (isMobile) return;
    const bar = topBarRef.current;
    if (!bar) return;
    const update = () => {
      const width = bar.clientWidth;
      const leftInset = topBarLeftRef.current?.offsetWidth ?? 0;
      const rightInset = topBarRightRef.current?.offsetWidth ?? 0;
      const slotWidth = tabSlotRef.current?.offsetWidth ?? 0;
      // Centred like upstream, but never under either group's buttons.
      const maxLeft = Math.max(leftInset, width - rightInset - slotWidth);
      const left = Math.min(Math.max((width - slotWidth) / 2, leftInset), maxLeft);
      // The decision reads the chat column's width, never the groups' own: their
      // labels collapse with `compact`, so measuring them would oscillate. Below
      // this the labelled groups and the labelled switch no longer fit together.
      const compact = width < 940;
      setTabSlot((current) => (
        Math.abs(current.left - left) < 0.5 && current.compact === compact
          ? current
          : { left, compact }
      ));
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(bar);
    if (topBarLeftRef.current) observer.observe(topBarLeftRef.current);
    if (topBarRightRef.current) observer.observe(topBarRightRef.current);
    return () => observer.disconnect();
  }, [isMobile]);

  // The pane remembers whether it was open; the read runs after hydration so
  // the server's markup and the first client render agree.
  useEffect(() => {
    try {
      if (window.localStorage.getItem(ARTIFACTS_OPEN_KEY) === "1") setArtifactsOpen(true);
    } catch {
      // Storage is best-effort.
    }
    artifactsPrefsRestoredRef.current = true;
  }, []);
  useEffect(() => {
    if (!artifactsPrefsRestoredRef.current) return;
    try {
      window.localStorage.setItem(ARTIFACTS_OPEN_KEY, artifactsOpen ? "1" : "0");
    } catch {
      // Storage is best-effort.
    }
  }, [artifactsOpen]);

  // File tabs follow their session across reloads too (paths and the active
  // one; a viewer's scroll state is not worth the storage).
  const artifactPanesRestoredRef = useRef(false);
  useEffect(() => {
    const stored = loadStoredArtifactPanes();
    if (Object.keys(stored).length > 0) setArtifactPanes(stored);
    artifactPanesRestoredRef.current = true;
  }, []);
  useEffect(() => {
    if (!artifactPanesRestoredRef.current) return;
    // A viewer reports scroll state as it goes: write after it settles, not
    // on every report.
    const timer = setTimeout(() => saveStoredArtifactPanes(artifactPanes), 400);
    return () => clearTimeout(timer);
  }, [artifactPanes]);
  const reclampArtifactsWidth = artifactsResizer.reclampWidth;
  useEffect(() => {
    if (artifactsOpen) reclampArtifactsWidth();
  }, [artifactsOpen, dockOpen, sidebarOpen, reclampArtifactsWidth]);
  // On mobile the sidebar is an overlay drawer; hide it by default so the chat
  // is visible on load. Runs once the breakpoint resolves after hydration.
  // Mobile drawer actions must not change the remembered desktop preference.
  useEffect(() => {
    setSidebarOpen(isMobile ? false : desktopSidebarOpenRef.current);
  }, [isMobile]);
  useEffect(() => {
    setMobileSidebarReady(true);
  }, []);
  // The dock's width is saved; the sidebar has to give way when it opens.
  const reclampDockWidth = dockResizer.reclampWidth;
  useEffect(() => {
    if (!dockOpen) return;
    reclampSidebarWidth();
    reclampDockWidth();
  }, [dockOpen, reclampDockWidth, reclampSidebarWidth]);
  const chatInputRef = useRef<ChatInputHandle | null>(null);
  const [pendingQuotePrompt, setPendingQuotePrompt] = useState<{ sessionId: string; text: string } | null>(null);
  const topBarRef = useRef<HTMLDivElement>(null);
  // The two toolbar groups the centred view switch must stay between: it is
  // absolutely positioned, so on a narrow chat (an open dock) it would else
  // draw over their buttons.
  const topBarLeftRef = useRef<HTMLDivElement>(null);
  const topBarRightRef = useRef<HTMLDivElement>(null);
  const tabSlotRef = useRef<HTMLDivElement>(null);
  const [tabSlot, setTabSlot] = useState({ left: 0, compact: false });
  const mobileToolbarRef = useRef<HTMLDivElement>(null);
  // Branch navigator state — populated by ChatWindow via onBranchDataChange
  const [branchTree, setBranchTree] = useState<SessionTreeNode[]>([]);
  const [branchActiveLeafId, setBranchActiveLeafId] = useState<string | null>(null);
  const [branchSwitchLocked, setBranchSwitchLocked] = useState(false);
  const branchLeafChangeFnRef = useRef<((leafId: string | null) => void) | null>(null);
  const sessionHasBranches = hasSessionBranches(branchTree);

  const handleBranchDataChange = useCallback((tree: SessionTreeNode[], activeLeafId: string | null, onLeafChange: (leafId: string | null) => void, locked: boolean) => {
    setBranchTree(tree);
    setBranchActiveLeafId(activeLeafId);
    setBranchSwitchLocked(locked);
    branchLeafChangeFnRef.current = onLeafChange;
  }, []);

  const handleBranchLeafChange = useCallback((leafId: string | null) => {
    branchLeafChangeFnRef.current?.(leafId);
  }, []);

  const [systemPrompt, setSystemPrompt] = useState<string | null>(null);
  const [systemTools, setSystemTools] = useState<ToolEntry[] | null>(null);
  const [systemInfoLoading, setSystemInfoLoading] = useState(false);
  const systemInfoLoaderRef = useRef<(() => Promise<void>) | null>(null);
  const systemInfoLoadIdRef = useRef(0);
  const handleSystemPromptChange = useCallback((prompt: string | null) => {
    setSystemPrompt(prompt);
    setSystemInfoLoading(false);
  }, []);

  const handleSystemToolsChange = useCallback((tools: ToolEntry[] | null) => {
    setSystemTools(tools);
  }, []);

  const handleSystemInfoLoaderChange = useCallback((loader: (() => Promise<void>) | null) => {
    systemInfoLoadIdRef.current += 1;
    systemInfoLoaderRef.current = loader;
    setSystemInfoLoading(false);
  }, []);

  // Settings › Session context reads on demand: the chat owns the request (it
  // may have to start the session first), this only asks it to run.
  const loadSystemInfo = useCallback(() => {
    const load = systemInfoLoaderRef.current;
    if (!load || systemInfoLoading) return;
    const loadId = ++systemInfoLoadIdRef.current;
    setSystemInfoLoading(true);
    void load()
      .catch((error) => { console.error("Failed to load system information:", error); })
      .finally(() => {
        if (systemInfoLoadIdRef.current === loadId) setSystemInfoLoading(false);
      });
  }, [systemInfoLoading]);

  // The debug panel's reader of the chat's own state: kept in a ref, so registering renders
  // nothing, and read only by the open panel (DebugPanel), never on AppShell's renders.
  const debugSnapshotReaderRef = useRef<(() => DebugClientSession) | null>(null);
  const handleDebugSnapshotReaderChange = useCallback((reader: (() => DebugClientSession) | null) => {
    debugSnapshotReaderRef.current = reader;
  }, []);
  // The panel reports the page's visibility history from the first mount on, opened or not.
  useEffect(() => {
    startPageVisibilityTracking();
  }, []);

  // Session stats (tokens + cost) — populated by ChatWindow, displayed in top bar
  const [sessionStats, setSessionStats] = useState<SessionStatsInfo | null>(null);
  const [autoNameStatus, setAutoNameStatus] = useState<AutoNameStatus>({ kind: "idle" });
  const autoNameTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const activeSessionIdRef = useRef<string | null>(selectedSession?.id ?? null);
  activeSessionIdRef.current = selectedSession?.id ?? null;
  const handleSessionStatsChange = useCallback((stats: SessionStatsInfo | null) => {
    setSessionStats(stats);
  }, []);
  const [copiedSessionField, setCopiedSessionField] = useState<SessionCopyField | null>(null);
  const sessionCopyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const handleCopySessionField = useCallback((field: SessionCopyField, value: string) => {
    void copyText(value).then(() => {
      if (sessionCopyTimerRef.current) clearTimeout(sessionCopyTimerRef.current);
      setCopiedSessionField(field);
      sessionCopyTimerRef.current = setTimeout(() => setCopiedSessionField(null), 1400);
    });
  }, []);

  useEffect(() => {
    return () => {
      if (sessionCopyTimerRef.current) clearTimeout(sessionCopyTimerRef.current);
      if (autoNameTimerRef.current) clearTimeout(autoNameTimerRef.current);
    };
  }, []);

  // Context usage — populated by ChatWindow, displayed in top bar
  const [contextUsage, setContextUsage] = useState<{ percent: number | null; contextWindow: number; tokens: number | null } | null>(null);
  const handleContextUsageChange = useCallback((usage: { percent: number | null; contextWindow: number; tokens: number | null } | null) => {
    setContextUsage(usage);
  }, []);

  // Single active panel — only one dropdown open at a time
  const [activeTopPanel, setActiveTopPanel] = useState<"agents" | "branches" | "session" | "debug" | null>(null);
  const [topPanelPos, setTopPanelPos] = useState<{ top: number; left: number; width: number } | null>(null);

  useEffect(() => {
    if (!sessionHasBranches) {
      setActiveTopPanel((panel) => panel === "branches" ? null : panel);
    }
  }, [sessionHasBranches]);

  useEffect(() => {
    if (!hasSubagentSessions) {
      setActiveTopPanel((panel) => panel === "agents" ? null : panel);
    }
  }, [hasSubagentSessions]);

  useEffect(() => {
    if (!dockExpanded) return;
    setActiveTopPanel(null);
  }, [dockExpanded]);

  const toggleTopPanel = useCallback((
    panel: "agents" | "branches" | "session" | "debug",
    keepMobileToolbarOpen = false,
  ) => {
    if (isMobile) setSidebarOpen(false);
    setActiveTopPanel((cur) => cur === panel ? null : panel);
    if (isMobile && isNarrowMobile && keepMobileToolbarOpen) setMobileToolbarMoreOpen(true);
  }, [isMobile, isNarrowMobile]);

  // The Debug button on screen (one of its three places at a time); Escape gives focus back to it.
  const debugButtonRef = useRef<HTMLButtonElement | null>(null);

  // Escape inside the debug panel; it never closes another panel that took its place. With focus
  // in the panel, that focus leaves the page with it: once the panel is gone (the next frame), the
  // Debug button gets it back, unless the user moved it meanwhile (focusIfLost).
  const closeDebugPanel = useCallback((restoreFocus: boolean) => {
    setActiveTopPanel((panel) => panel === "debug" ? null : panel);
    if (restoreFocus) requestAnimationFrame(() => focusIfLost(document, debugButtonRef.current));
  }, []);

  const openSessionStatsPanel = useCallback(() => {
    if (isMobile) setSidebarOpen(false);
    setMobileToolbarMoreOpen(false);
    setActiveTopPanel("session");
  }, [isMobile]);

  // The composer opens Settings too: a bare /mcp opens Settings › MCP (useAgentSession).
  const openSettingsSection = useCallback((section: SettingsSection) => {
    setSettingsOpenAtHome(false);
    setSettingsSection(section);
  }, []);

  const handleSidebarToggle = useCallback(() => {
    if (isMobile) {
      setActiveTopPanel(null);
      setMobileToolbarMoreOpen(false);
    }
    setSidebarOpen((open) => {
      const next = !open;
      if (!isMobile) desktopSidebarOpenRef.current = next;
      return next;
    });
  }, [isMobile]);

  const handleMobileToolbarMoreToggle = useCallback(() => {
    setSidebarOpen(false);
    setActiveTopPanel(null);
    setMobileToolbarMoreOpen((open) => !open);
  }, []);

  const handleDockToggle = useCallback(() => {
    setActiveTopPanel(null);
    setMobileToolbarMoreOpen(false);
    setDockOpen((open) => !open);
  }, []);

  const handleDockExpandToggle = useCallback(() => {
    setActiveTopPanel(null);
    setDockExpanded((expanded) => !expanded);
  }, []);

  useEffect(() => {
    if (!mobileToolbarMoreOpen) return;

    const handlePointerDown = (event: PointerEvent) => {
      const toolbar = mobileToolbarRef.current;
      if (toolbar && event.composedPath().includes(toolbar)) return;
      setMobileToolbarMoreOpen(false);
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      setMobileToolbarMoreOpen(false);
    };

    document.addEventListener("pointerdown", handlePointerDown, true);
    document.addEventListener("keydown", handleKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown, true);
      document.removeEventListener("keydown", handleKeyDown, true);
    };
  }, [mobileToolbarMoreOpen]);

  useEffect(() => {
    setMobileToolbarMoreOpen(false);
  }, [isMobile, isNarrowMobile, selectedSession?.id, newSessionDraftId]);

  useEffect(() => {
    if (!activeTopPanel || !topBarRef.current) return;
    const update = () => {
      const topBarRect = topBarRef.current!.getBoundingClientRect();
      if (activeTopPanel === "agents") {
        setTopPanelPos({
          top: topBarRect.bottom,
          left: topBarRect.left,
          width: Math.min(AGENT_PANEL_WIDTH, topBarRect.width),
        });
        return;
      }
      setTopPanelPos({ top: topBarRect.bottom, left: topBarRect.left, width: topBarRect.width });
    };
    update();
    const ro = new ResizeObserver(update);
    ro.observe(topBarRef.current);
    return () => ro.disconnect();
  }, [activeTopPanel, isMobile]);

  // Project workspaces: the main view's Code/Git/Terminal state. The right dock
  // keeps its own copies of these, per leaf (lib/dock-state.ts).
  const [codeWorkspaces, setCodeWorkspaces] = useState<Record<string, CodeWorkspaceState>>(() => loadCodeWorkspaces());
  const [gitWorkspaces, setGitWorkspaces] = useState<Record<string, GitWorkspaceState>>(() => loadGitWorkspaces());
  const [terminalWorkspaces, setTerminalWorkspaces] = useState<Record<string, TerminalWorkspaceState>>(() => loadTerminalWorkspaces());

  // Same @mention format as the chat input's @ autocomplete, so the agent's
  // read tool resolves it the same way (it strips the @ prefix).
  const handleAtMention = useCallback((relativePath: string, isDir: boolean) => {
    chatInputRef.current?.insertText(buildAtMentionText(relativePath, isDir));
    if (isMobile) setSidebarOpen(false);
  }, [isMobile]);


  const handleFileLineMention = useCallback((relativePath: string, startLine: number, endLine: number) => {
    chatInputRef.current?.insertText(buildFileLineMentionText(relativePath, startLine, endLine));
    if (isMobile) setSidebarOpen(false);
  }, [isMobile]);

  const initialSessionId = initialNavigation.sessionId;
  const initialLinkedSessionId = linkedSessionId(initialNavigation);
  const [activeCwd, setActiveCwd] = useState<string | null>(null);
  const [activeProjectKey, setActiveProjectKey] = useState<string | null>(null);
  const activeProjectKeyRef = useRef<string | null>(null);
  // True once the initial ?session= URL param has been resolved (or confirmed absent)
  const [initialSessionRestored, setInitialSessionRestored] = useState<boolean>(() => !initialSessionId);
  // sessionStorage is empty during SSR. Applying the tab's remembered session
  // in the useState initializer made the first client tree differ from the
  // server HTML (sidebar "select project" vs ""). Restore after mount instead.
  useLayoutEffect(() => {
    const next = withTabOpen(initialNavigation, getTabOpen());
    if (next === initialNavigation) return;
    setInitialNavigation(next);
    if (next.sessionId) setInitialSessionRestored(false);
  }, [initialNavigation]);
  // Suppresses sessionKey bump in handleCwdChange during the initial URL restore
  const suppressCwdBumpRef = useRef(false);
  // Set when the session the URL linked to is gone: the project the sidebar
  // adopts next keeps a fresh composer instead of restoring its last session.
  const skipWorkspaceRestoreRef = useRef(false);
  // Guards the async workspace restore so a slow response from an earlier
  // switch cannot resurrect a session into a project the user already left.
  const workspaceRestoreTokenRef = useRef(0);

  const invalidateWorkspaceRestore = useCallback(() => {
    workspaceRestoreTokenRef.current += 1;
  }, []);

  // Persist every active-session transition, including new and forked sessions
  // that bypass the sidebar selection handler. Transient sessions do not yet
  // carry projectKey, so use the active project identity until hydration.
  // The workspace memory is shared by every tab; the tab memory keeps this
  // tab's own session so a reload does not follow another tab's last pick.
  // New session is a selection too: remember the composer cwd so reload stays
  // on that UI instead of resurrecting the previous chat.
  useEffect(() => {
    if (selectedSession) {
      const projectKey = selectedSession.projectKey
        ?? activeProjectKeyRef.current
        ?? workspaceKeyOf(selectedSession);
      setLastOpenSession(projectKey, selectedSession.id);
      setTabOpenSession(selectedSession.id);
      return;
    }
    if (newSessionCwd) setTabOpenNewSession(newSessionCwd);
  }, [newSessionCwd, selectedSession]);

  useEffect(() => {
    const requestedCwd = initialNavigation.requestedCwd;
    if (!requestedCwd) return;

    const controller = new AbortController();
    setInitialCwdStatus("validating");
    setInitialCwdError(null);

    void fetch("/api/cwd/validate", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ cwd: requestedCwd }),
      signal: controller.signal,
    })
      .then(async (response) => {
        const data = await response.json().catch(() => ({})) as { cwd?: string; error?: string };
        if (!response.ok || !data.cwd) {
          throw new Error(data.error ?? `HTTP ${response.status}`);
        }

        // The sidebar will notify us when it adopts this cwd. Avoid remounting
        // the just-created empty chat during that initial synchronization.
        suppressCwdBumpRef.current = true;
        const draftId = `initial:${requestedCwd}`;
        setNewSessionDraftId(draftId);
        activeNewSessionDraftKeyRef.current = `new:${draftId}:${data.cwd}`;
        setNewSessionCwd(data.cwd);
        setInitialCwdStatus("ready");
        if (!new URLSearchParams(window.location.search).get("cwd")) {
          router.replace(`?cwd=${encodeURIComponent(data.cwd)}`, { scroll: false });
        }
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted) return;
        // A remembered new-session cwd can go stale between launches (moved or
        // deleted project). It must not trap startup on the error page: forget
        // it and fall back to the normal workspace selection. An explicit
        // ?cwd= deep link still gets the retryable error UI below.
        if (initialNavigation.requestedCwdFromTab) {
          clearTabOpen();
          setInitialCwdError(null);
          setInitialCwdStatus("idle");
          setInitialNavigation((navigation) => ({
            ...navigation,
            requestedCwd: null,
            requestedCwdFromTab: false,
          }));
          return;
        }
        setInitialCwdError(error instanceof Error ? error.message : String(error));
        setInitialCwdStatus("error");
      });

    return () => controller.abort();
  }, [initialCwdAttempt, initialNavigation, router]);

  const dismissInitialWorkspace = useCallback(() => {
    clearTabOpen();
    setInitialCwdError(null);
    setInitialCwdStatus("idle");
    setInitialNavigation((navigation) => ({
      ...navigation,
      requestedCwd: null,
      requestedCwdFromTab: false,
    }));
    const url = new URL(window.location.href);
    url.searchParams.delete("cwd");
    router.replace(`${url.pathname}${url.search}`, { scroll: false });
  }, [router]);

  const retryInitialWorkspace = useCallback(() => {
    setInitialCwdStatus("validating");
    setInitialCwdAttempt((attempt) => attempt + 1);
  }, []);

  // Restore the workspace's last open session after switching to it. Called
  // from handleCwdChange once the outgoing context has been reset. The session
  // is looked up against the live list so a deleted or drifted session falls
  // back to the default welcome page instead of erroring.
  const restoreWorkspaceContext = useCallback((projectKey: string, cwd: string) => {
    const token = ++workspaceRestoreTokenRef.current;
    const lastOpenSessionId = getLastOpenSession(projectKey);
    if (!lastOpenSessionId) return;
    const adopt = (d: { sessions: SessionInfo[] } | null) => {
      if (token !== workspaceRestoreTokenRef.current) return; // stale switch
      const s = d?.sessions.find((x) => x.id === lastOpenSessionId);
      if (!s) {
        // The list loaded but the remembered session is gone — forget it.
        // When the list itself failed (d === null) keep the memory so a
        // later switch retries the restore.
        if (d) clearLastOpen(projectKey);
        return;
      }
      if (workspaceKeyOf(s) !== projectKey) {
        // Defensive: the remembered session drifted out of this workspace.
        clearLastOpen(projectKey);
        return;
      }
      // Keep the temporary composer's draft in its cwd, even when the
      // remembered session belongs to another worktree of this project.
      const activeDraftKey = activeNewSessionDraftKeyRef.current;
      if (activeDraftKey) {
        rekeyDraft(activeDraftKey, parkedNewSessionDraftKey(cwd));
      }
      activeNewSessionDraftKeyRef.current = null;
      // Selecting the session must remount the chat with the session
      // present: useAgentSession loads content in a mount-only effect, so
      // the null-session welcome mount from the switch would never load
      // the restored session's messages.
      setSelectedSession(s);
      setSessionKey((k) => k + 1);
      if (new URLSearchParams(window.location.search).get("session") !== s.id) {
        router.replace(`?session=${encodeURIComponent(s.id)}`, { scroll: false });
      }
    };
    // Fast path: the sidebar already delivered the catalogue — restore
    // without waiting on a fresh /api/sessions round trip.
    if (sessionCatalog.length > 0) {
      adopt({ sessions: sessionCatalog });
      return;
    }
    void fetch("/api/sessions")
      .then((r) => (r.ok ? (r.json() as Promise<{ sessions: SessionInfo[] }>) : null))
      .then(adopt)
      .catch(() => {
        // Network hiccup: keep the remembered session for a later retry.
      });
  }, [router, sessionCatalog]);

  const handleCwdChange = useCallback((
    cwd: string | null,
    projectRoot?: string | null,
    projectKey?: string | null,
  ) => {
    invalidateWorkspaceRestore();
    const currentFreshCwd = newSessionCwd ?? activeCwd;
    setActiveCwd(cwd);
    // Skip if cwd is null (initial mount).
    if (!cwd) return;
    const skipWorkspaceRestore = skipWorkspaceRestoreRef.current;
    skipWorkspaceRestoreRef.current = false;
    const newProject = projectKey ?? projectRoot ?? cwd;
    const currentProject = activeProjectKeyRef.current
      ?? (selectedSession ? workspaceKeyOf(selectedSession) : null);
    activeProjectKeyRef.current = newProject;
    setActiveProjectKey(newProject);

    // Keep the project identity in sync during the initial URL restore without
    // remounting the just-created or restored chat.
    if (suppressCwdBumpRef.current) {
      suppressCwdBumpRef.current = false;
      return;
    }
    // The server may hydrate a normalized key after a custom cwd is already
    // active. Updating identity for the exact same cwd is not a user switch.
    if (currentFreshCwd === cwd && currentProject !== newProject) return;
    // Existing sessions stay open when the worktree selector moves within the
    // same project. A fresh composer must remount when its effective cwd moves,
    // otherwise its already-created runtime would keep sending to the old cwd.
    if (
      currentProject === newProject
      && (selectedSession !== null || currentFreshCwd === cwd)
    ) {
      return;
    }
    // Close any session that belongs to a different project — it no longer
    // matches the selected project directory.
    const previousDraftKey = activeNewSessionDraftKeyRef.current;
    if (previousDraftKey && currentFreshCwd) {
      rekeyDraft(previousDraftKey, parkedNewSessionDraftKey(currentFreshCwd));
    }
    const draftId = typeof crypto.randomUUID === "function"
      ? crypto.randomUUID()
      : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
    const draftKey = `new:${draftId}:${cwd}`;
    rekeyDraft(parkedNewSessionDraftKey(cwd), draftKey);
    setNewSessionDraftId(draftId);
    activeNewSessionDraftKeyRef.current = draftKey;
    setSelectedSession(null);
    setNewSessionCwd((prev) => {
      if (prev && prev !== cwd) return null;
      return prev;
    });
    setSessionKey((k) => k + 1);
    setBranchTree([]);
    setBranchActiveLeafId(null);
    setBranchSwitchLocked(false);
    setSystemPrompt(null);
    setSystemTools(null);
    setSystemInfoLoading(false);
    // The debug panel stays open across the switch: it follows whichever session is shown.
    setActiveTopPanel((panel) => panel === "debug" ? panel : null);
    if (currentProject !== newProject) {
      // Restore the workspace we switched to: its last open session, or keep
      // the default welcome page when none is remembered.
      if (!skipWorkspaceRestore) restoreWorkspaceContext(newProject, cwd);
    }
    router.replace(typeof window !== "undefined" ? window.location.pathname : "/", { scroll: false });
  }, [activeCwd, invalidateWorkspaceRestore, newSessionCwd, router, selectedSession, restoreWorkspaceContext]);

  const handleSelectSession = useCallback((session: SessionInfo, isRestore = false, entryId?: string, blockIndex?: number, options?: SelectSessionOptions) => {
    setSearchTarget(entryId ? { sessionId: session.id, entryId, blockIndex } : null);
    invalidateWorkspaceRestore();
    const activeDraftKey = activeNewSessionDraftKeyRef.current;
    const activeDraftCwd = newSessionCwd ?? (selectedSession === null ? activeCwd : null);
    if (activeDraftKey && activeDraftCwd) {
      rekeyDraft(activeDraftKey, parkedNewSessionDraftKey(activeDraftCwd));
    }
    activeNewSessionDraftKeyRef.current = null;
    // Adopt an explicitly selected session before the sidebar reports its cwd.
    const projectKey = workspaceKeyOf(session);
    if (activeProjectKeyRef.current !== projectKey) {
      setActiveTopPanel((panel) => panel === "debug" ? panel : null);
    }
    activeProjectKeyRef.current = projectKey;
    setActiveProjectKey(projectKey);
    // Re-clicking the already-open session must not remount the chat and
    // re-run the full load/positioning cycle. Only skip when the effective
    // cwd context already matches — otherwise a pending cwd move still needs
    // the full re-select flow.
    if (!isRestore && selectedSession) {
      const sameProject =
        workspaceKeyOf(selectedSession) === workspaceKeyOf(session);
      if (selectedSession.id === session.id && sameProject) {
        if (isMobile) setSidebarOpen(false);
        return;
      }
    }
    setNewSessionCwd(null);
    setSelectedSession(session);
    setSessionKey((k) => k + 1);
    setBranchTree([]);
    setBranchActiveLeafId(null);
    setBranchSwitchLocked(false);
    branchLeafChangeFnRef.current = null;
    setSystemPrompt(null);
    setSystemTools(null);
    setSystemInfoLoading(false);
    setInitialSessionRestored(true);
    // On mobile, collapse the overlay drawer so the chat is revealed after pick
    // (unless the sidebar still has something to show: a fork's row and toast).
    if (isMobile && !isRestore && !options?.keepSidebarOpen) setSidebarOpen(false);
    if (isRestore) {
      // Suppress the redundant sessionKey bump that would come from the
      // onCwdChange effect firing after setSelectedCwd in the sidebar
      suppressCwdBumpRef.current = true;
    }
    // Skip router.replace when the URL already has this session — calling
    // replace in production Next.js triggers a Suspense remount loop.
    // Tab-memory restore lands on `/` and must write `?session=` so reload
    // and copy-link keep this session.
    if (!isRestore || new URLSearchParams(window.location.search).get("session") !== session.id) {
      router.replace(`?session=${encodeURIComponent(session.id)}`, { scroll: false });
    }
  }, [activeCwd, invalidateWorkspaceRestore, router, isMobile, newSessionCwd, selectedSession]);

  const handleNewSession = useCallback((sessionId: string, cwd: string, projectKey?: string | null, options?: NewSessionOptions) => {
    invalidateWorkspaceRestore();
    const draftKey = `new:${sessionId}:${cwd}`;
    // Leaving a fresh composer for another cwd parks its draft there, as a
    // workspace switch does; New in the same cwd still starts empty. The bar
    // above the composer moves the composer instead: its draft (the live
    // text, as promoteNewSession takes it) and its model picks go along.
    const activeDraftKey = activeNewSessionDraftKeyRef.current;
    const activeDraftCwd = newSessionCwd ?? (selectedSession === null ? activeCwd : null);
    if (options?.carryComposer && selectedSession === null && activeDraftKey) {
      const input = chatInputRef.current;
      if (input) input.rekeyDraft(activeDraftKey, draftKey);
      else rekeyDraft(activeDraftKey, draftKey);
      setCarriedNewSessionChoices(newSessionChoicesRef.current);
    } else if (activeDraftKey && activeDraftCwd && activeDraftCwd !== cwd) {
      rekeyDraft(activeDraftKey, parkedNewSessionDraftKey(activeDraftCwd));
    }
    // Adopt the target project before the sidebar reports its cwd, as an
    // explicit session pick does. Without a key (Ctrl+Alt+N) the current cwd
    // keeps its project.
    const targetProject = projectKey ?? (cwd === activeCwd ? activeProjectKeyRef.current : null) ?? cwd;
    activeProjectKeyRef.current = targetProject;
    setActiveProjectKey(targetProject);
    // A draft parked in this cwd comes back, unless one was carried here: it
    // stays parked for the next time, never merged into what was carried.
    if (!getDraft(draftKey)) rekeyDraft(parkedNewSessionDraftKey(cwd), draftKey);
    activeNewSessionDraftKeyRef.current = draftKey;
    setNewSessionDraftId(sessionId);
    setSelectedSession(null);
    setNewSessionCwd(cwd);
    setSessionKey((k) => k + 1);
    setBranchTree([]);
    setBranchActiveLeafId(null);
    setBranchSwitchLocked(false);
    setSystemPrompt(null);
    setSystemTools(null);
    setSystemInfoLoading(false);
    setActiveTopPanel((panel) => panel === "debug" ? panel : null);
    if (isMobile) setSidebarOpen(false);
    router.replace(`?cwd=${encodeURIComponent(cwd)}`, { scroll: false });
    // The fresh composer takes the caret once it renders (#1313), unless the
    // move came from the new-session bar, whose control keeps focus instead
    // (newSessionBarFocusRef), or a phone would raise the keyboard for it.
    // A dialog still open over the page (Ctrl+Alt+N under Settings) keeps it.
    if (!isMobile && !newSessionBarFocusRef.current) {
      requestAnimationFrame(() => {
        if (document.activeElement?.closest("[aria-modal='true']")) return;
        chatInputRef.current?.focus();
      });
    }
  }, [activeCwd, invalidateWorkspaceRestore, isMobile, newSessionCwd, router, selectedSession]);

  // Global keyboard shortcuts (handles Esc, Ctrl+Alt+N etc.)
  useGlobalKeyboardShortcuts({
    onNewSession: (cwd: string) => handleNewSession(`kb-${Date.now()}`, cwd),
    activeCwd,
  });

  // Client-built transient SessionInfo (new session / fork) lacks the
  // server-computed projectKey, which the same-project check in
  // handleCwdChange relies on. Hydrate it from the session list so switching
  // worktrees right after creating a session doesn't close the chat.
  const hydrateSelectedSession = useCallback((sessionId: string) => {
    void fetch("/api/sessions", { cache: "no-store" })
      .then((r) => (r.ok ? (r.json() as Promise<{ sessions: SessionInfo[] }>) : null))
      .then((d) => {
        const full = d?.sessions.find((s) => s.id === sessionId);
        if (!full) return;
        setSelectedSession((prev) => (
          prev?.id === sessionId
            ? { ...prev, ...full, transient: full.transient ?? false }
            : prev
        ));
      })
      .catch(() => {});
  }, []);

  // Called by ChatWindow when a new session gets its real id from pi
  const handleSessionCreated = useCallback((session: SessionInfo, sourceDraftKey: string) => {
    setRefreshKey((k) => k + 1);
    if (activeNewSessionDraftKeyRef.current !== sourceDraftKey) return;
    invalidateWorkspaceRestore();
    activeNewSessionDraftKeyRef.current = null;
    setNewSessionCwd(null);
    setSelectedSession(session);
    hydrateSelectedSession(session.id);
    router.replace(`?session=${encodeURIComponent(session.id)}`, { scroll: false });
  }, [invalidateWorkspaceRestore, router, hydrateSelectedSession]);

  const deliverSessionNotification = useCallback(({
    targetSession,
    title,
    body,
    tag,
  }: {
    targetSession: SessionInfo | null;
    title: string;
    body: string;
    tag?: string;
  }) => {
    if (!("Notification" in window)) return;

    const fire = () => {
      const sessionUrl = targetSession ? `/?session=${encodeURIComponent(targetSession.id)}` : "/";
      void showBrowserNotification({
        title,
        body,
        sessionUrl,
        tag,
        onClick: () => {
          window.focus();
          if (targetSession) handleSelectSession(targetSession);
        },
      });
    };

    if (Notification.permission === "granted") {
      fire();
    } else if (Notification.permission === "default") {
      void Notification.requestPermission().then((p) => {
        if (p === "granted") {
          fire();
            }
      });
    }
  }, [handleSelectSession, locale]);

  const handleAgentEnd = useCallback((end: AgentEndInfo) => {
    // The sidebar's reload merges the selected session's new row into it
    // (handleSessionsChange), so no list fetch of its own here.
    setRefreshKey((k) => k + 1);
    setExplorerRefreshKey((k) => k + 1);

    if (end.aborted || end.subagentsActive || isHiddenRelation(selectedSession?.relation)) return;
    if (!shouldShowBrowserNotification()) return;
    const targetSession = selectedSession;
    if (targetSession) {
      setHiddenFinishedSessionIds((prev) => {
        if (prev.has(targetSession.id)) return prev;
        const next = new Set(prev);
        next.add(targetSession.id);
        return next;
      });
    }
    deliverSessionNotification({
      targetSession,
      title: targetSession?.name ?? translate("i18n.sessionComplete"),
      body: translate("i18n.taskFinished"),
      tag: targetSession ? `pi-session-complete:${targetSession.id}` : "pi-session-complete",
    });
  }, [deliverSessionNotification, selectedSession, translate]);

  // The badge is for an unseen tab: opening it puts the finished chat on screen.
  useEffect(() => {
    const clearIfSeen = () => {
      if (document.visibilityState !== "visible" || !document.hasFocus()) return;
      setHiddenFinishedSessionIds((prev) => (prev.size === 0 ? prev : new Set()));
    };
    clearIfSeen();
    document.addEventListener("visibilitychange", clearIfSeen);
    window.addEventListener("focus", clearIfSeen);
    return () => {
      document.removeEventListener("visibilitychange", clearIfSeen);
      window.removeEventListener("focus", clearIfSeen);
    };
  }, []);

  const handleAttentionNeeded = useCallback((request: BlockingExtensionUiRequest) => {
    if (isHiddenRelation(selectedSession?.relation)) return;
    if (!shouldShowBrowserNotification()) return;
    if (!claimExtensionAttentionNotification(request, notifiedAttentionRequestIdsRef.current)) return;

    deliverSessionNotification({
      targetSession: selectedSession,
      title: translate("i18n.attentionNeeded"),
      body: request.method === "custom"
        ? translate("i18n.extensionInputNeeded")
        : request.title,
      tag: `pi-extension-ui:${request.id}`,
    });
  }, [deliverSessionNotification, selectedSession, translate]);

  const handleAutoName = useCallback(async () => {
    const sessionId = selectedSession?.id;
    if (!sessionId || autoNameStatus.kind === "naming") return;
    if (autoNameTimerRef.current) clearTimeout(autoNameTimerRef.current);
    setActiveTopPanel(null);
    setAutoNameStatus({ kind: "naming" });

    try {
      const response = await fetch(`/api/sessions/${encodeURIComponent(sessionId)}/auto-name`, {
        method: "POST",
      });
      const body = (await response.json().catch(() => ({}))) as { title?: string; error?: string };
      if (!response.ok || !body.title) {
        throw new Error(body.error || `HTTP ${response.status}`);
      }

      const title = body.title.trim();
      setRefreshKey((key) => key + 1);
      if (activeSessionIdRef.current !== sessionId) return;
      setSelectedSession((current) => current?.id === sessionId ? { ...current, name: title } : current);
      setSessionStats((current) => current?.sessionId === sessionId ? { ...current, sessionName: title } : current);
      setAutoNameStatus({ kind: "success" });
      autoNameTimerRef.current = setTimeout(() => setAutoNameStatus({ kind: "idle" }), 1800);
    } catch (error) {
      if (activeSessionIdRef.current !== sessionId) return;
      const message = error instanceof Error ? error.message : String(error);
      setAutoNameStatus({ kind: "error", message });
      autoNameTimerRef.current = setTimeout(() => setAutoNameStatus({ kind: "idle" }), 5000);
    }
  }, [autoNameStatus.kind, selectedSession?.id]);

  useEffect(() => {
    if (autoNameTimerRef.current) clearTimeout(autoNameTimerRef.current);
    setAutoNameStatus({ kind: "idle" });
  }, [selectedSession?.id]);

  const handleExplorerRefresh = useCallback(() => {
    setExplorerRefreshKey((k) => k + 1);
  }, []);

  // A run's writes show in the file tree as they happen, not only when it ends
  // (#1144): at most one refresh a second, after any tool but the read-only ones.
  const toolEndRefreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const handleToolEnd = useCallback((toolName: string) => {
    if (READ_ONLY_TOOL_NAMES.has(toolName) || toolEndRefreshTimerRef.current) return;
    toolEndRefreshTimerRef.current = setTimeout(() => {
      toolEndRefreshTimerRef.current = null;
      setExplorerRefreshKey((k) => k + 1);
    }, TOOL_END_REFRESH_MS);
  }, []);
  useEffect(() => () => {
    if (toolEndRefreshTimerRef.current) clearTimeout(toolEndRefreshTimerRef.current);
  }, []);

  const handleSessionForked = useCallback((newSessionId: string) => {
    invalidateWorkspaceRestore();
    activeNewSessionDraftKeyRef.current = null;
    setRefreshKey((k) => k + 1);
    setSessionKey((k) => k + 1);
    setNewSessionCwd(null);
    setSelectedSession((prev) => ({
      ...(prev ?? { path: "", cwd: "", created: "", modified: "", messageCount: 0, firstMessage: "" }),
      id: newSessionId,
      transient: false,
    }));
    hydrateSelectedSession(newSessionId);
    router.replace(`?session=${encodeURIComponent(newSessionId)}`, { scroll: false });
  }, [invalidateWorkspaceRestore, router, hydrateSelectedSession]);

  const handleAskInNewChat = useCallback(async (
    prompt: string,
    sourceSessionId: string,
    sourceEntryId: string,
  ) => {
    const result = await sendAgentCommand<{ newSessionId?: string }>(sourceSessionId, {
      type: "fork_branch",
      entryId: sourceEntryId,
    });
    if (!result?.newSessionId) throw new Error(translate("chat.quoteForkFailed"));
    setPendingQuotePrompt({ sessionId: result.newSessionId, text: prompt });
    handleSessionForked(result.newSessionId);
  }, [handleSessionForked, translate]);

  // The sidebar calls this when the initial session is not in its list.
  const codeRoot = activeCwd ?? selectedSession?.cwd ?? newSessionCwd ?? null;
  const codeProjectIdentity = activeProjectKey ?? selectedSession?.projectKey ?? codeRoot;
  const codeWorkspaceKey = codeRoot
    ? makeCodeWorkspaceKey(codeProjectIdentity, codeRoot)
    : null;
  const codeWorkspace = useMemo(() => {
    if (!codeRoot || !codeWorkspaceKey) return null;
    return codeWorkspaces[codeWorkspaceKey]
      ?? emptyCodeWorkspace(codeProjectIdentity ?? codeRoot, codeRoot);
  }, [codeProjectIdentity, codeRoot, codeWorkspaceKey, codeWorkspaces]);

  const updateCodeWorkspace = useCallback((next: CodeWorkspaceState) => {
    if (!codeWorkspaceKey) return;
    setCodeWorkspaces((prev) => ({ ...prev, [codeWorkspaceKey]: next }));
    saveCodeWorkspace(codeWorkspaceKey, next);
  }, [codeWorkspaceKey]);

  const openCodeFile = useCallback((filePath: string, fileName: string, page?: number) => {
    if (!codeRoot || !codeWorkspace) return;
    updateCodeWorkspace(openCodeFileTab(codeWorkspace, filePath, fileName, page));
    setMainView("code");
    setCodeMounted(true);
    saveMainView("code");
  }, [codeRoot, codeWorkspace, updateCodeWorkspace]);

  const gitProjectRoot = activeProjectKey ?? selectedSession?.projectRoot ?? codeRoot;
  const gitWorkspace = useMemo(() => {
    if (!gitProjectRoot) return emptyGitWorkspace("");
    return gitWorkspaces[gitProjectRoot] ?? emptyGitWorkspace(gitProjectRoot);
  }, [gitProjectRoot, gitWorkspaces]);

  const updateGitWorkspace = useCallback((next: GitWorkspaceState) => {
    if (!gitProjectRoot) return;
    setGitWorkspaces((prev) => ({ ...prev, [gitProjectRoot]: next }));
    saveGitWorkspace(gitProjectRoot, next);
  }, [gitProjectRoot]);

  // Terminals belong to the project, not to the cwd of whichever session is
  // open. A pane keeps its own live cwd after this initial default.
  const terminalDefaultCwd = gitProjectRoot;
  const terminalWorkspace = useMemo(() => {
    if (!gitProjectRoot) return emptyTerminalWorkspace("");
    return terminalWorkspaces[gitProjectRoot] ?? emptyTerminalWorkspace(gitProjectRoot);
  }, [gitProjectRoot, terminalWorkspaces]);

  const updateTerminalWorkspace = useCallback((next: TerminalWorkspaceState) => {
    if (!gitProjectRoot) return;
    setTerminalWorkspaces((prev) => ({ ...prev, [gitProjectRoot]: next }));
    saveTerminalWorkspace(gitProjectRoot, next);
  }, [gitProjectRoot]);

  // ── Right dock ─────────────────────────────────────────────────────────
  // A project's own workbench. Every leaf is an independent instance of Code,
  // Diff or Terminal, laid out in a split tree; nothing is shared with the main
  // view, so the two can show different things at once (lib/dock-state.ts).
  const dockProjectKey = gitProjectRoot;
  const dockContext: DockContext | null = dockProjectKey
    ? { projectKey: dockProjectKey, root: codeRoot ?? dockProjectKey }
    : null;
  const dockState = useMemo(
    () => (dockProjectKey ? dockStates[dockProjectKey] ?? emptyDockState() : null),
    [dockProjectKey, dockStates],
  );
  const dockVisible = canDock && dockOpen && dockContext !== null && dockState !== null;
  const dockFullWidth = dockVisible && dockExpanded;
  const updateDockState = useCallback((next: DockStateUpdater) => {
    if (!dockProjectKey) return;
    setDockStates((prev) => {
      const current = prev[dockProjectKey] ?? emptyDockState();
      const value = typeof next === "function" ? next(current) : next;
      if (value === current) return prev;
      return { ...prev, [dockProjectKey]: value };
    });
  }, [dockProjectKey]);
  // A restored layout can name a leaf whose state was dropped: give it one.
  useEffect(() => {
    if (!dockProjectKey || !dockContext || !dockState?.layout.root) return;
    const ensured = ensureDockLeafState(dockState, dockContext);
    if (ensured !== dockState) updateDockState(ensured);
  }, [dockContext, dockProjectKey, dockState, updateDockState]);
  useEffect(() => {
    saveDockStates(dockStates);
  }, [dockStates]);
  // The artifacts pane overlays the chat once the chat column gets narrow, so
  // the dock and the sidebar never squeeze it into a sliver.
  const artifactsAvailableWidth = viewportWidth
    - (sidebarOpen ? sidebarResizer.width : 0)
    - (dockVisible ? dockResizer.width : 0);
  const artifactsOverlay = isMobile || artifactsAvailableWidth < ARTIFACTS_DOCK_MIN_WIDTH;

  const handleInitialRestoreDone = useCallback(() => {
    setInitialSessionRestored(true);
    if (initialLinkedSessionId) skipWorkspaceRestoreRef.current = true;
  }, [initialLinkedSessionId]);

  const handleSessionDeleted = useCallback((sessionId: string) => {
    invalidateWorkspaceRestore();
    setRefreshKey((k) => k + 1);
    // The DELETE can outlive a session switch: this callback's captured
    // selectedSession is from the delete click. Read the latest selection
    // and only fall back to the empty composer when the user is still on
    // the deleted session at the moment removal completes.
    const active = selectedSessionRef.current;
    if (active?.id === sessionId) {
      clearTabOpenSession(sessionId);
      const cwd = active.cwd;
      const draftId = typeof crypto.randomUUID === "function"
        ? crypto.randomUUID()
        : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
      setNewSessionDraftId(draftId);
      activeNewSessionDraftKeyRef.current = cwd ? `new:${draftId}:${cwd}` : null;
      setSelectedSession(null);
      setNewSessionCwd(cwd ?? null);
      setSessionKey((k) => k + 1);
      setBranchTree([]);
      setBranchActiveLeafId(null);
      setBranchSwitchLocked(false);
      setSystemPrompt(null);
      setSystemTools(null);
      setSystemInfoLoading(false);
      setActiveTopPanel((panel) => panel === "debug" ? panel : null);
      router.replace(cwd ? `?cwd=${encodeURIComponent(cwd)}` : (typeof window !== "undefined" ? window.location.pathname : "/"), { scroll: false });
    }
  }, [invalidateWorkspaceRestore, router]);

  // Show chat area if a session is selected, or if we have a cwd to start a new session in
  const effectiveNewSessionCwd = newSessionCwd ?? (selectedSession === null && activeCwd ? activeCwd : null);
  const newSessionDraftKey = selectedSession === null && effectiveNewSessionCwd
    ? `new:${newSessionDraftId}:${effectiveNewSessionCwd}`
    : null;

  // ── Session artifacts pane ─────────────────────────────────────────────
  const artifactPaneKey = selectedSession?.id ?? effectiveNewSessionCwd;
  const artifactPane = (artifactPaneKey ? artifactPanes[artifactPaneKey] : undefined) ?? EMPTY_ARTIFACT_PANE;
  const sessionArtifacts = useMemo(
    () => mergeSessionArtifacts(historyArtifacts, liveArtifacts),
    [historyArtifacts, liveArtifacts],
  );
  const artifactsCwd = selectedSession?.cwd ?? effectiveNewSessionCwd ?? null;

  const updateArtifactPane = useCallback((change: (current: ArtifactPaneState) => ArtifactPaneState) => {
    const key = artifactPaneKey;
    if (!key) return;
    setArtifactPanes((panes) => {
      const current = panes[key] ?? EMPTY_ARTIFACT_PANE;
      const next = change(current);
      return next === current ? panes : { ...panes, [key]: next };
    });
  }, [artifactPaneKey]);

  // Sub-agent work opens as a transcript tab in the session's artifacts pane,
  // beside the files, instead of taking over the whole chat.
  const handleOpenSubagentTab = useCallback((sessionId: string, label: string) => {
    if (!artifactPaneKey) return;
    updateArtifactPane((current) => ({
      tabs: openAgentTab(current.tabs, sessionId, label),
      activeTabId: agentTabId(sessionId),
    }));
    setArtifactsOpen(true);
    // The pane lives on the chat's right: a sub-agent's work is session-scoped.
    if (mainView !== "chat") {
      setMainView("chat");
      saveMainView("chat");
    }
    if (isMobile) setSidebarOpen(false);
  }, [artifactPaneKey, isMobile, mainView, updateArtifactPane]);

  const openSubagentTabIds = useMemo(() => {
    const ids = new Set<string>();
    if (!artifactsOpen) return ids;
    for (const tab of artifactPane.tabs) {
      const sessionId = agentTabSessionId(tab.id);
      if (sessionId) ids.add(sessionId);
    }
    return ids;
  }, [artifactPane.tabs, artifactsOpen]);

  // The agent switcher's row button: opens the transcript tab, or closes it
  // when the pane already shows it.
  const handleToggleSubagentTab = useCallback((sessionId: string, label: string) => {
    const tabId = agentTabId(sessionId);
    if (artifactPane.tabs.some((tab) => tab.id === tabId)) {
      updateArtifactPane((current) => {
        const tabs = current.tabs.filter((tab) => tab.id !== tabId);
        if (tabs.length === current.tabs.length) return current;
        return {
          tabs,
          activeTabId: current.activeTabId === tabId ? (tabs.at(-1)?.id ?? null) : current.activeTabId,
        };
      });
      return;
    }
    if (isMobile) setActiveTopPanel(null);
    handleOpenSubagentTab(sessionId, label);
  }, [artifactPane.tabs, handleOpenSubagentTab, isMobile, updateArtifactPane]);

  const handleOpenArtifactFile = useCallback((
    filePath: string,
    page?: number,
    sourceSessionId?: string | null,
  ) => {
    if (!artifactPaneKey) return;
    const tabId = `file:${filePath}`;
    updateArtifactPane((current) => ({
      tabs: openFileTab(current.tabs, {
        fileName: getFileName(filePath),
        filePath,
        page,
        sourceSessionId: sourceSessionId ?? selectedSession?.id ?? null,
        tabId,
      }),
      activeTabId: tabId,
    }));
    setArtifactsOpen(true);
    if (isMobile) setSidebarOpen(false);
  }, [artifactPaneKey, isMobile, selectedSession?.id, updateArtifactPane]);

  const handleArtifactViewerStateChange = useCallback((tabId: string, revision: number, viewerState: FileViewerState) => {
    updateArtifactPane((current) => {
      const tabs = saveFileViewerState(current.tabs, tabId, revision, viewerState);
      return tabs === current.tabs ? current : { ...current, tabs };
    });
  }, [updateArtifactPane]);

  const handleOpenArtifactInCode = useCallback((filePath: string) => {
    openCodeFile(filePath, getFileName(filePath));
    setMainView("code");
    saveMainView("code");
  }, [openCodeFile]);

  const handleOpenArtifactsCodeView = useCallback(() => {
    setMainView("code");
    saveMainView("code");
  }, []);

  const closeArtifacts = useCallback(() => {
    const pane = artifactsResizer.panelRef.current;
    const focusWasInside = Boolean(pane && pane.contains(document.activeElement));
    setArtifactsOpen(false);
    if (focusWasInside) artifactsButtonRef.current?.focus({ preventScroll: true });
  }, [artifactsResizer.panelRef]);

  const handleArtifactsToggle = useCallback(() => {
    setArtifactsOpen((open) => {
      const next = !open;
      if (next && isMobile) setSidebarOpen(false);
      return next;
    });
  }, [isMobile]);

  const handleArtifactsChange = useCallback((files: SessionArtifact[]) => setLiveArtifacts(files), []);

  // The pane's content follows the session: drop the previous session's live
  // list at once (its ChatWindow remounts), and show a cached scan if there is
  // one before the fresh scan lands.
  // Layout effect: it must clear before the new ChatWindow's passive effect
  // reports its live list, or the report would be wiped in the same commit.
  // Keyed on the ChatWindow's own lifetime: a new session that is promoted
  // keeps its live list (same instance), while a switch remounts it.
  useLayoutEffect(() => {
    setLiveArtifacts([]);
    const cacheKey = selectedSession ? `${selectedSession.id}:${branchActiveLeafId ?? ""}` : null;
    setHistoryArtifacts(cacheKey ? (artifactScanCacheRef.current.get(cacheKey) ?? []) : []);
    setArtifactsScanning(false);
  }, [sessionKey]);

  // A branch switch changes which writes are on the active leaf: the live
  // transcript reports itself again, while the scanned list starts over.
  useLayoutEffect(() => {
    const cacheKey = selectedSession ? `${selectedSession.id}:${branchActiveLeafId ?? ""}` : null;
    setHistoryArtifacts(cacheKey ? (artifactScanCacheRef.current.get(cacheKey) ?? []) : []);
  }, [branchActiveLeafId, selectedSession?.id]);

  // Opening the pane scans the session's whole transcript (paged, outside the
  // chat's own message window); closing it aborts the scan.
  useEffect(() => {
    const session = selectedSession;
    if (!artifactsOpen || mainView !== "chat" || !session || session.transient) return;
    const cacheKey = `${session.id}:${branchActiveLeafId ?? ""}`;
    const cached = artifactScanCacheRef.current.get(cacheKey);
    if (cached) setHistoryArtifacts(cached);
    const controller = new AbortController();
    setArtifactsScanning(true);
    void loadSessionArtifacts({
      sessionId: session.id,
      leafId: branchActiveLeafId,
      cwd: session.cwd,
      signal: controller.signal,
    }).then((files) => {
      if (controller.signal.aborted) return;
      artifactScanCacheRef.current.set(cacheKey, files);
      setHistoryArtifacts(files);
    }).catch((error) => {
      if (!controller.signal.aborted) console.error("Failed to scan session artifacts:", error);
    }).finally(() => {
      if (!controller.signal.aborted) setArtifactsScanning(false);
    });
    return () => controller.abort();
  }, [artifactsOpen, mainView, selectedSession?.id, selectedSession?.cwd, branchActiveLeafId]);
  useLayoutEffect(() => {
    activeNewSessionDraftKeyRef.current = newSessionDraftKey;
  }, [newSessionDraftKey]);

  // The bar above a fresh composer. Only a fresh composer moves, and only
  // somewhere else: the folder in use would remount it for nothing. The
  // control the move came from takes focus in the bar of the new composer.
  const handlePickNewSessionContext = useCallback((target: NewSessionTarget, from: NewSessionContextControl) => {
    if (selectedSession !== null || !effectiveNewSessionCwd || target.cwd === effectiveNewSessionCwd) return;
    newSessionBarFocusRef.current = from;
    // A worktree just created already left its move, with the branch.
    if (target.projectKey && target.projectRoot && newSessionMoveRef.current?.cwd !== target.cwd) {
      newSessionMoveRef.current = { cwd: target.cwd, project: { key: target.projectKey, root: target.projectRoot }, branch: null };
    }
    sidebarControlRef.current?.startNewSessionIn({ ...target, carryComposer: true });
  }, [effectiveNewSessionCwd, selectedSession]);
  // The folder picker answers later: its pick runs the newest closure, guard included.
  const pickNewSessionContextRef = useRef(handlePickNewSessionContext);
  pickNewSessionContextRef.current = handlePickNewSessionContext;
  const handleOpenFolderForNewSession = useCallback((opener: HTMLElement | null) => {
    sidebarControlRef.current?.openFolderForNewSession((target) => pickNewSessionContextRef.current(target, "project"), opener);
  }, []);
  const handleDefaultDirectoryForNewSession = useCallback(() => {
    sidebarControlRef.current?.openDefaultDirectoryForNewSession((target) => pickNewSessionContextRef.current(target, "project"));
  }, []);
  const handleRefreshNewSessionWorktrees = useCallback(() => {
    sidebarControlRef.current?.refreshWorktrees();
  }, []);
  const handleCreateNewSessionWorktree = useCallback(async (project: ProjectChoice, branch: string) => {
    const control = sidebarControlRef.current;
    if (!control) throw new Error("The session sidebar is not mounted");
    const result = await control.createWorktree(project, branch);
    // The bar moves there next, before any report lists it.
    if ("path" in result) newSessionMoveRef.current = { cwd: result.path, project, branch };
    return result;
  }, []);
  const handleNewSessionBarFocusDone = useCallback(() => {
    newSessionBarFocusRef.current = null;
  }, []);
  const newSessionContextBar = selectedSession === null && effectiveNewSessionCwd ? (
    <NewSessionContextBar
      context={contextForCwd(sidebarNewSessionContext, effectiveNewSessionCwd, newSessionMoveRef.current)}
      mobile={isMobile}
      initialFocus={newSessionBarFocusRef.current}
      onInitialFocusDone={handleNewSessionBarFocusDone}
      onPick={handlePickNewSessionContext}
      onUseDefaultDirectory={handleDefaultDirectoryForNewSession}
      onOpenFolder={handleOpenFolderForNewSession}
      onRefreshWorktrees={handleRefreshNewSessionWorktrees}
      onCreateWorktree={handleCreateNewSessionWorktree}
    />
  ) : null;
  const showChat = selectedSession !== null || effectiveNewSessionCwd !== null;
  const projectTrustCwd = selectedSession?.cwd ?? effectiveNewSessionCwd;
  // While restoring initial session from URL, don't show the placeholder
  const showPlaceholder = initialSessionRestored && !showChat;

  // The Debug button shows with a chat only: without one, its panel would have nothing to close it.
  useEffect(() => {
    if (!showChat) setActiveTopPanel((panel) => panel === "debug" ? null : panel);
  }, [showChat]);

  useEffect(() => {
    setProjectTrust(null);
    setProjectTrustDialogOpen(false);
    setProjectTrustError(null);
    if (!projectTrustCwd) return;

    const controller = new AbortController();
    // The answer also lists the project's MCP servers (`mcpFile`, `mcpServers`), unused here: the
    // trust dialog fetches the listing again when it opens, since the file can change in between.
    fetch(`/api/project-trust?cwd=${encodeURIComponent(projectTrustCwd)}`, {
      signal: controller.signal,
    })
      .then(async (response) => {
        const data = await response.json() as ProjectTrustStatus & { error?: string };
        if (!response.ok || data.error) throw new Error(data.error ?? `HTTP ${response.status}`);
        setProjectTrust(data);
      })
      .catch((error) => {
        if (error instanceof DOMException && error.name === "AbortError") return;
        console.error("Failed to load project trust:", error);
      });
    return () => controller.abort();
  }, [projectTrustCwd]);

  const handleTrustProject = useCallback(async () => {
    if (!projectTrustCwd || projectTrustBusy) return;
    setProjectTrustBusy(true);
    setProjectTrustError(null);
    try {
      const response = await fetch("/api/project-trust", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ cwd: projectTrustCwd }),
      });
      const data = await response.json() as ProjectTrustStatus & Partial<McpErrorResponse>;
      if (!response.ok || data.error) {
        // The dialog translates the reason; the English error is only its diagnostic.
        setProjectTrustError({ error: data.error ?? `HTTP ${response.status}`, ...(data.reason ? { reason: data.reason } : {}) });
        return;
      }
      setProjectTrust(data);
      setProjectTrustDialogOpen(false);
      setModelsRefreshKey((key) => key + 1);
      setSessionKey((key) => key + 1);
    } catch (error) {
      setProjectTrustError({ error: error instanceof Error ? error.message : String(error) });
    } finally {
      setProjectTrustBusy(false);
    }
  }, [projectTrustBusy, projectTrustCwd]);

  // The restricted-mode banner and Settings › MCP's trust notice open the same dialog.
  const openProjectTrustDialog = useCallback(() => {
    setProjectTrustError(null);
    setProjectTrustDialogOpen(true);
  }, []);

  // Settings › MCP added a project server: `.pi/mcp.json` alone makes a folder require trust, and a
  // fresh folder was trusted in the same step. Every mounted section reloads in place on the new
  // status (projectTrustReloadKey); nothing was rebuilt, so the chat needs no new session key.
  const handleProjectTrustChanged = useCallback((cwd: string, status: ProjectTrustStatus) => {
    if (cwd === projectTrustCwd) setProjectTrust(status);
  }, [projectTrustCwd]);

  const activeCwdName = activeCwd ? getFileName(activeCwd) || activeCwd : null;
  const windowTitle = activeCwdName ? `${activeCwdName} - Pi Star` : "Pi Star";

  useEffect(() => {
    const syncWindowTitle = () => {
      if (document.title !== windowTitle) document.title = windowTitle;
    };

    syncWindowTitle();
    const observer = new MutationObserver(syncWindowTitle);
    observer.observe(document.head, { childList: true, subtree: true, characterData: true });
    return () => observer.disconnect();
  }, [windowTitle]);

  const sidebarContent = (
    <>
      <SessionSidebar
        selectedSessionId={selectedSession?.id ?? null}
        onSelectSession={handleSelectSession}
        onNewSession={handleNewSession}
        controlRef={sidebarControlRef}
        onNewSessionContextChange={handleSidebarNewSessionContext}
        initialSessionId={initialSessionId}
        initialSessionLinked={initialLinkedSessionId !== null}
        skipInitialProjectSelection={initialNavigation.requestedCwd !== null}
        onInitialRestoreDone={handleInitialRestoreDone}
        refreshKey={refreshKey}
        onSessionDeleted={handleSessionDeleted}
        selectedCwd={selectedSession?.cwd ?? newSessionCwd ?? null}
        onCwdChange={handleCwdChange}
        onBackgroundTaskDone={handleBackgroundTaskDone}
        hiddenFinishedSessionIds={hiddenFinishedSessionIds}
        onRunningSessionIdsChange={handleRunningSessionIdsChange}
        onSessionsChange={handleSessionsChange}
      />
      <div style={{ padding: "8px", flexShrink: 0, display: "flex", justifyContent: "space-between", gap: 4 }}>
        {([
          ["models", translate("common.models")],
          ["skills", translate("common.skills")],
        ] as const).map(([section, label]) => {
          const disabled = settingsSectionRequiresProject(section) && !projectTrustCwd;
          return (
            <button
              key={section}
              type="button"
              onClick={() => openSettingsSection(section)}
              disabled={disabled}
              title={disabled ? translate("settings.projectRequired") : label}
              aria-label={label}
              style={{
                flex: 1, display: "flex", alignItems: "center", justifyContent: "center", gap: 6,
                height: 32, padding: 0, background: "none", border: "none",
                borderRadius: 9, color: "var(--text-muted)", cursor: disabled ? "default" : "pointer",
                fontSize: 12, opacity: disabled ? 0.35 : 1,
                transition: "background 0.12s, color 0.12s",
              }}
              onMouseEnter={(event) => { if (!disabled) { event.currentTarget.style.background = "var(--bg-hover)"; event.currentTarget.style.color = "var(--text)"; } }}
              onMouseLeave={(event) => { event.currentTarget.style.background = "none"; event.currentTarget.style.color = "var(--text-muted)"; }}
            >
              <SettingsSectionIcon section={section} size={14} strokeWidth={2} />
              <span>{label}</span>
            </button>
          );
        })}
        <button
          type="button"
          onClick={() => {
            setSettingsOpenAtHome(true);
            setSettingsSection(getLastSettingsSection(projectTrustCwd));
          }}
          title={translate("common.settings")}
          aria-label={translate("common.settings")}
          style={{
            flex: 1, display: "flex", alignItems: "center", justifyContent: "center", gap: 6,
            height: 32, padding: 0, background: "none", border: "none",
            borderRadius: 9, color: "var(--text-muted)", cursor: "pointer",
            fontSize: 12, transition: "background 0.12s, color 0.12s",
          }}
          onMouseEnter={(event) => { event.currentTarget.style.background = "var(--bg-hover)"; event.currentTarget.style.color = "var(--text)"; }}
          onMouseLeave={(event) => { event.currentTarget.style.background = "none"; event.currentTarget.style.color = "var(--text-muted)"; }}
        >
          <SettingsSectionIcon section="general" size={14} strokeWidth={2} />
          <span>{translate("common.settings")}</span>
        </button>
      </div>
    </>
  );

  const renderProjectTrustWarning = (mobileBanner: boolean) => {
    if (!showChat || !projectTrust?.requiresTrust || projectTrust.trusted) return null;
    return (
      <button
        type="button"
        onClick={openProjectTrustDialog}
        title={translate("trust.resourcesNotLoaded")}
        aria-label={translate("trust.resourcesNotLoaded")}
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: mobileBanner ? "flex-start" : "center",
          gap: 6,
          width: mobileBanner ? "100%" : undefined,
          minHeight: mobileBanner ? 32 : undefined,
          height: mobileBanner ? undefined : "100%",
          padding: mobileBanner ? "6px 12px" : "0 12px",
          background: mobileBanner ? "color-mix(in srgb, #d97706 8%, var(--bg-panel))" : "none",
          border: "none",
          borderRight: mobileBanner ? "none" : "1px solid var(--border)",
          borderBottom: mobileBanner ? "1px solid var(--border)" : "none",
          color: "#d97706",
          cursor: "pointer",
          flexShrink: 0,
          fontSize: 11,
          lineHeight: 1.35,
          textAlign: "left",
        }}
        data-mobile-trust-banner={mobileBanner ? "true" : undefined}
      >
        <svg
          width="13"
          height="13"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
          style={{ flexShrink: 0 }}
        >
          <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z" />
          <path d="M12 8v4" />
          <path d="M12 16h.01" />
        </svg>
        <span>{translate("trust.resourcesNotLoaded")}</span>
      </button>
    );
  };

  // While subagents run: their count, ringed by a spinner; otherwise all of them.
  const renderAgentsBadge = (badge: { count: number; running: boolean }, mobile: boolean) => (
    <span
      aria-hidden="true"
      style={{
        position: "relative",
        minWidth: 15, height: 15, padding: "0 4px", display: "grid", placeItems: "center",
        borderRadius: 7,
        background: badge.running ? "color-mix(in srgb, var(--accent) 22%, var(--bg-panel))" : "var(--bg-selected)",
        color: "var(--accent)",
        fontSize: 10, lineHeight: 1, fontVariantNumeric: "tabular-nums",
        fontWeight: badge.running ? 600 : undefined,
        ...(mobile ? { position: "absolute", top: 3, right: 3, minWidth: 13, height: 13, padding: "0 3px", fontSize: 9 } : {}),
      }}
    >
      {badge.running && (
        <svg
          className="animate-spin motion-reduce:animate-none"
          viewBox="0 0 24 24" fill="none" aria-hidden="true"
          style={{ position: "absolute", top: -3, left: -3, width: "calc(100% + 6px)", height: "calc(100% + 6px)", pointerEvents: "none" }}
        >
          <circle cx="12" cy="12" r="11" stroke="currentColor" strokeWidth="2" opacity="0.2" />
          <path d="M23 12a11 11 0 0 0-11-11" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
        </svg>
      )}
      {badge.count}
    </span>
  );

  // The Agents switcher's button. A narrow phone shows it left of the More button instead of
  // folding it into that menu: the bar has the room, and the badge stays in sight.
  const renderAgentsButton = (mobile: boolean) => (
    <button
      type="button"
      className="main-toolbar-action"
      // On a narrow phone it sits outside the More menu, so it leaves that menu as it is.
      onClick={() => toggleTopPanel("agents", mobile && !isNarrowMobile)}
      disabled={!activeSessionFamily}
      title={agentsButtonLabel}
      aria-label={agentsButtonLabel}
      aria-pressed={activeTopPanel === "agents"}
      style={{
        position: "relative",
        display: "flex", alignItems: "center", justifyContent: "center", gap: 6,
        width: mobile ? TOP_BAR_ICON_BUTTON_SIZE : undefined,
        height: "100%", padding: mobile ? 0 : "0 12px",
        background: activeTopPanel === "agents" ? "var(--bg-selected)" : "none",
        border: "none",
        borderTop: activeTopPanel === "agents" ? "2px solid var(--accent)" : "2px solid transparent",
        color: activeTopPanel === "agents" ? "var(--text)" : activeSessionFamily ? "var(--text-muted)" : "var(--text-dim)",
        cursor: activeSessionFamily ? "pointer" : "not-allowed", opacity: activeSessionFamily ? 1 : 0.45,
        flexShrink: 0, fontSize: 11, whiteSpace: "nowrap",
        transition: "color 0.1s, background 0.1s",
      }}
      data-mobile-toolbar-action={mobile ? "agents" : undefined}
    >
      <svg width={mobile ? 16 : 13} height={mobile ? 16 : 13} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <rect x="5" y="7" width="14" height="11" rx="2" /><path d="M9 11h.01M15 11h.01M9 15h6M12 7V4M10 4h4" />
      </svg>
      {!mobile && <span>{translate("agentSwitcher.title")}</span>}
      {agentsBadge && agentsBadge.count > 0 && renderAgentsBadge(agentsBadge, mobile)}
    </button>
  );

  const renderChatToolbarActions = (mobile: boolean) => {
    if (!mobile && !showChat) return null;
    return (
      <div style={{ display: "flex", alignItems: "stretch", height: "100%" }}>
        {(() => {
          // 上下文压缩后当前消息可能不再包含 user 消息，需同时参考会话文件的消息总数。
          const hasMessages = Boolean(
            selectedSession
            && ((sessionStats?.userMessages ?? 0) > 0 || selectedSession.messageCount > 0),
          );
          const disabled = !selectedSession || selectedSession.transient || !hasMessages || autoNameStatus.kind === "naming";
          const isSuccess = autoNameStatus.kind === "success";
          const isError = autoNameStatus.kind === "error";
          const label = autoNameStatus.kind === "naming"
            ? translate("title.generating")
            : isSuccess
              ? translate("title.updated")
              : isError
                ? translate("title.failed")
                : translate("title.generate");
          const title = !selectedSession || selectedSession.transient
            ? translate("title.unsaved")
            : !hasMessages
              ? translate("title.noMessages")
              : isError
                ? autoNameStatus.message
                : translate("title.generateSession");

          return (
            <button
              type="button"
              className="main-toolbar-action"
              onClick={() => {
                void handleAutoName();
                if (mobile && isNarrowMobile) setMobileToolbarMoreOpen(true);
              }}
              disabled={disabled}
              title={title}
              aria-label={label}
              style={{
                display: "flex", alignItems: "center", justifyContent: "center", gap: 6,
                width: mobile ? TOP_BAR_ICON_BUTTON_SIZE : undefined,
                height: "100%", padding: mobile ? 0 : "0 12px",
                background: "none", border: "none",
                borderTop: "2px solid transparent",
                color: isError ? "#dc2626" : isSuccess ? "var(--accent)" : disabled ? "var(--text-dim)" : "var(--text-muted)",
                cursor: disabled ? "not-allowed" : "pointer",
                opacity: disabled && autoNameStatus.kind !== "naming" ? 0.45 : 1,
                flexShrink: 0, fontSize: 11, whiteSpace: "nowrap",
                transition: "color 0.1s, background 0.1s, opacity 0.1s",
              }}
              onMouseEnter={(event) => {
                if (disabled) return;
                event.currentTarget.style.color = isError ? "#dc2626" : "var(--text)";
                event.currentTarget.style.background = "var(--bg-hover)";
              }}
              onMouseLeave={(event) => {
                event.currentTarget.style.color = isError ? "#dc2626" : isSuccess ? "var(--accent)" : disabled ? "var(--text-dim)" : "var(--text-muted)";
                event.currentTarget.style.background = "none";
              }}
              data-mobile-toolbar-action={mobile ? "name" : undefined}
            >
              {autoNameStatus.kind === "naming" ? (
                <svg className="animate-spin" width={mobile ? 16 : 13} height={mobile ? 16 : 13} viewBox="0 0 24 24" fill="none" aria-hidden="true">
                  <circle cx="12" cy="12" r="9" stroke="currentColor" strokeWidth="2" opacity="0.25" />
                  <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
                </svg>
              ) : isSuccess ? (
                <svg width={mobile ? 16 : 13} height={mobile ? 16 : 13} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <polyline points="20 6 9 17 4 12" />
                </svg>
              ) : (
                <svg width={mobile ? 16 : 13} height={mobile ? 16 : 13} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <path d="m15 4 5 5L7 22l-5-5Z" />
                  <path d="m14 5 5 5" />
                  <path d="M6 4V2M5 3H3M19 19v3M17.5 20.5h3" />
                </svg>
              )}
              {!mobile && <span>{label}</span>}
            </button>
          );
        })()}
        {!(mobile && isNarrowMobile) && renderAgentsButton(mobile)}
        {sessionHasBranches && (mobile ? (
          <button
            type="button"
            onClick={() => toggleTopPanel("branches", true)}
            title={translate("i18n.branches")}
            aria-label={translate("i18n.branches")}
            aria-pressed={activeTopPanel === "branches"}
            style={{
              display: "flex", alignItems: "center", justifyContent: "center",
              width: TOP_BAR_ICON_BUTTON_SIZE, height: "100%", padding: 0,
              background: activeTopPanel === "branches" ? "var(--bg-selected)" : "none",
              border: "none",
              borderTop: activeTopPanel === "branches" ? "2px solid var(--accent)" : "2px solid transparent",
              borderRight: "1px solid var(--border)",
              color: activeTopPanel === "branches" ? "var(--text)" : "var(--text-muted)",
              cursor: "pointer", flexShrink: 0,
            }}
            data-mobile-toolbar-action="branches"
          >
            <svg width={mobile ? 16 : 12} height={mobile ? 16 : 12} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ color: branchTree.length > 0 ? "var(--accent)" : "var(--text-dim)" }} aria-hidden="true">
              <line x1="6" y1="3" x2="6" y2="15" />
              <circle cx="18" cy="6" r="3" />
              <circle cx="6" cy="18" r="3" />
              <path d="M18 9a9 9 0 0 1-9 9" />
            </svg>
          </button>
        ) : (
          <BranchNavigator
            tree={branchTree}
            activeLeafId={branchActiveLeafId}
            onLeafChange={handleBranchLeafChange}
            locked={branchSwitchLocked}
            inline
            containerRef={topBarRef}
            open={activeTopPanel === "branches"}
            onToggle={() => toggleTopPanel("branches")}
            hasSession
          />
        ))}
      </div>
    );
  };

  const renderSessionStatsButton = (mobile: boolean) => {
    if (!mobile && (!showChat || (!sessionStats && !contextUsage))) return null;

    const tokens = sessionStats?.tokens;
    const cost = sessionStats?.cost ?? 0;
    const formatCompact = formatCompactCount;
    const costText = formatCostShort(cost);

    let contextColor = "var(--text-muted)";
    let desktopContextText: string | null = null;
    let mobileContextText: string | null = null;
    if (contextUsage?.contextWindow) {
      const percent = contextUsage.percent;
      if (percent !== null && percent > 90) contextColor = "#ef4444";
      else if (percent !== null && percent > 70) contextColor = "rgba(234,179,8,0.95)";
      desktopContextText = percent !== null
        ? `${percent.toFixed(0)}% / ${formatCompact(contextUsage.contextWindow)}`
        : `? / ${formatCompact(contextUsage.contextWindow)}`;
      mobileContextText = percent !== null ? `${percent.toFixed(0)}%` : null;
    }

    const tooltipParts: string[] = [];
    if (tokens) {
      tooltipParts.push(`${translate("session.input")}: ${tokens.input.toLocaleString(locale)}`);
      tooltipParts.push(`${translate("session.output")}: ${tokens.output.toLocaleString(locale)}`);
      tooltipParts.push(`${translate("session.cacheRead")}: ${tokens.cacheRead.toLocaleString(locale)}`);
      tooltipParts.push(`${translate("session.cacheWrite")}: ${tokens.cacheWrite.toLocaleString(locale)}`);
      if (cost > 0) tooltipParts.push(`${translate("session.cost")}: $${cost.toFixed(4)}`);
    }
    if (contextUsage?.contextWindow) {
      const percent = contextUsage.percent;
      tooltipParts.push(`${translate("session.context")}: ${percent !== null ? `${percent.toFixed(1)}%` : "?"} / ${contextUsage.contextWindow.toLocaleString(locale)}`);
    }
    const tooltip = tooltipParts.join("  |  ");
    const covered = mobile && isNarrowMobile && mobileToolbarMoreOpen;
    const hasMobileValues = Boolean(
      (tokens && (tokens.input > 0 || tokens.output > 0))
      || costText
      || mobileContextText,
    );

    return (
      <button
        type="button"
        onClick={() => toggleTopPanel("session")}
        disabled={!showChat || covered}
        tabIndex={covered ? -1 : undefined}
        title={tooltip || translate("session.title")}
        aria-label={translate("session.title")}
        aria-pressed={activeTopPanel === "session"}
        aria-hidden={covered ? true : undefined}
        className={mobile ? "mobile-session-stats" : undefined}
        data-mobile-toolbar-stats={mobile ? "true" : undefined}
        style={{
          // The Debug button before it takes the free space (renderDebugButton).
          marginLeft: 0,
          display: "flex", alignItems: "center", justifyContent: "flex-end",
          flex: mobile ? 1 : undefined,
          minWidth: 0,
          gap: mobile ? 7 : 10,
          paddingLeft: mobile ? 6 : 12,
          paddingRight: mobile ? 6 : 12,
          height: "100%",
          overflow: "hidden",
          visibility: covered ? "hidden" : "visible",
          pointerEvents: covered ? "none" : "auto",
          background: activeTopPanel === "session" ? "var(--bg-selected)" : "none",
          border: "none",
          borderTop: activeTopPanel === "session" ? "2px solid var(--accent)" : "2px solid transparent",
          fontSize: 11, color: "var(--text-muted)",
          whiteSpace: "nowrap", cursor: showChat ? "pointer" : "default",
          fontVariantNumeric: "tabular-nums",
          transition: "color 0.1s, background 0.1s",
        }}
        onMouseEnter={(event) => {
          if (showChat && !covered) event.currentTarget.style.color = "var(--text)";
        }}
        onMouseLeave={(event) => {
          event.currentTarget.style.color = activeTopPanel === "session" ? "var(--text)" : "var(--text-muted)";
        }}
      >
        {mobile ? (
          <>
            {tokens && tokens.input > 0 && (
              <span className="mobile-session-stat-io" style={{ display: "flex", alignItems: "center", gap: 2, flexShrink: 0 }}>
                <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <line x1="5" y1="8.5" x2="5" y2="1.5" /><polyline points="2 4 5 1.5 8 4" />
                </svg>
                {formatCompact(tokens.input)}
              </span>
            )}
            {tokens && tokens.output > 0 && (
              <span className="mobile-session-stat-io" style={{ display: "flex", alignItems: "center", gap: 2, flexShrink: 0 }}>
                <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <line x1="5" y1="1.5" x2="5" y2="8.5" /><polyline points="2 6 5 8.5 8 6" />
                </svg>
                {formatCompact(tokens.output)}
              </span>
            )}
            {costText && (
              <span className="mobile-session-stat-cost" style={{ color: "var(--text)", fontWeight: 500, flexShrink: 0 }}>
                {costText}
              </span>
            )}
            {mobileContextText && (
              <span style={{ color: contextColor, flexShrink: 0 }}>
                {mobileContextText}
              </span>
            )}
            {!hasMobileValues && showChat && (
              <span style={{ overflow: "hidden", textOverflow: "ellipsis", color: "var(--text-dim)" }}>
                {translate("session.title")}
              </span>
            )}
          </>
        ) : (
          <>
            {tokens && tokens.input > 0 && (
              <span style={{ display: "flex", alignItems: "center", gap: 4 }}>
                <svg width="12" height="12" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <line x1="5" y1="8.5" x2="5" y2="1.5" /><polyline points="2 4 5 1.5 8 4" />
                </svg>
                {formatCompact(tokens.input)}
              </span>
            )}
            {tokens && tokens.output > 0 && (
              <span style={{ display: "flex", alignItems: "center", gap: 4 }}>
                <svg width="12" height="12" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <line x1="5" y1="1.5" x2="5" y2="8.5" /><polyline points="2 6 5 8.5 8 6" />
                </svg>
                {formatCompact(tokens.output)}
              </span>
            )}
            {tokens && tokens.cacheRead > 0 && (
              <span style={{ display: "flex", alignItems: "center", gap: 4 }}>
                <svg width="12" height="12" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <path d="M8.5 5a3.5 3.5 0 1 1-1-2.45" /><polyline points="6.5 1.5 8.5 2.5 7.5 4.5" />
                </svg>
                {formatCompact(tokens.cacheRead)}
              </span>
            )}
            {costText && (
              <span style={{ display: "flex", alignItems: "center", color: "var(--text)", fontWeight: 500 }}>
                {costText}
              </span>
            )}
            {desktopContextText && (
              <span style={{ display: "flex", alignItems: "center", gap: 4, color: contextColor }}>
                <svg width="12" height="12" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <path d="M1 9 L1 5 Q1 1 5 1 Q9 1 9 5 L9 9" /><line x1="1" y1="9" x2="9" y2="9" />
                </svg>
                {desktopContextText}
              </span>
            )}
          </>
        )}
      </button>
    );
  };

  // The session's artifacts pane, left of Debug: its own panel and state, not
  // the global file panel's. The count says what the session has written.
  const renderArtifactsButton = (mobile: boolean) => {
    if (!showChat || mainView !== "chat") return null;
    const open = artifactsOpen;
    const count = sessionArtifacts.length;
    return (
      <button
        ref={artifactsButtonRef}
        type="button"
        onClick={handleArtifactsToggle}
        title={translate(open ? "artifacts.hide" : "artifacts.show")}
        aria-label={translate(open ? "artifacts.hide" : "artifacts.show")}
        aria-controls="session-artifacts"
        aria-expanded={open}
        data-mobile-toolbar-action={mobile ? "artifacts" : undefined}
        style={{
          position: "relative",
          marginLeft: mobile ? 0 : "auto",
          display: "flex", alignItems: "center", justifyContent: "center",
          width: TOP_BAR_ICON_BUTTON_SIZE, height: "100%", padding: 0,
          background: open ? "var(--bg-selected)" : "none",
          border: "none",
          borderTop: open ? "2px solid var(--accent)" : "2px solid transparent",
          color: open ? "var(--text)" : "var(--text-muted)",
          cursor: "pointer", flexShrink: 0, transition: "color 0.1s, background 0.1s",
        }}
        onMouseEnter={(event) => { event.currentTarget.style.color = "var(--text)"; }}
        onMouseLeave={(event) => { event.currentTarget.style.color = open ? "var(--text)" : "var(--text-muted)"; }}
      >
        <span style={{ display: "flex", color: open ? "var(--accent)" : "var(--text-dim)" }}>
          <ArtifactsIcon size={mobile ? 16 : 13} open={open} />
        </span>
        {count > 0 && (
          <span className="artifacts-button-count" aria-hidden="true">{count > 99 ? "99+" : count}</span>
        )}
      </button>
    );
  };

  // Icon only, right of the chat's actions; on a desktop it takes the bar's free space, so it
  // sits left of the session stats. It only opens or closes its panel: nothing is loaded here.
  const renderDebugButton = (mobile: boolean) => {
    if (!showChat) return null;
    const open = activeTopPanel === "debug";
    return (
      <button
        ref={debugButtonRef}
        type="button"
        onClick={() => toggleTopPanel("debug", mobile)}
        title={translate("debug.title")}
        aria-label={translate("debug.title")}
        aria-pressed={open}
        style={{
          marginLeft: mobile || (showChat && mainView === "chat") ? 0 : "auto",
          display: "flex", alignItems: "center", justifyContent: "center",
          width: TOP_BAR_ICON_BUTTON_SIZE, height: "100%", padding: 0,
          background: open ? "var(--bg-selected)" : "none",
          border: "none",
          borderTop: open ? "2px solid var(--accent)" : "2px solid transparent",
          ...(mobile ? { borderRight: "1px solid var(--border)" } : {}),
          color: open ? "var(--text)" : "var(--text-muted)",
          cursor: "pointer", flexShrink: 0, transition: "color 0.1s, background 0.1s",
        }}
        onMouseEnter={(event) => {
          event.currentTarget.style.color = "var(--text)";
        }}
        onMouseLeave={(event) => {
          event.currentTarget.style.color = open ? "var(--text)" : "var(--text-muted)";
        }}
        data-mobile-toolbar-action={mobile ? "debug" : undefined}
      >
        <svg width={mobile ? 16 : 13} height={mobile ? 16 : 13} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ color: open ? "var(--accent)" : "var(--text-dim)", flexShrink: 0 }} aria-hidden="true">
          <path d="M8 2l1.9 1.9M16 2l-1.9 1.9M9 7.1v-1a3 3 0 0 1 6 0v1" />
          <path d="M12 20c-3.3 0-6-2.7-6-6v-3a4 4 0 0 1 4-4h4a4 4 0 0 1 4 4v3c0 3.3-2.7 6-6 6zM12 20v-9" />
          <path d="M6.5 9C4.6 8.8 3 7.1 3 5M6 13H2M3 21c0-2.1 1.7-3.9 3.8-4M21 5c0 2.1-1.6 3.8-3.5 4M22 13h-4M17.2 17c2.1.1 3.8 1.9 3.8 4" />
        </svg>
      </button>
    );
  };

  // The dock's toggle, left of the session stats on a desktop toolbar. It only
  // opens or closes the pane: every leaf loads its own view.
  const renderDockButton = () => {
    if (!canDock) return null;
    return (
      <button
        type="button"
        onClick={handleDockToggle}
        aria-controls="right-dock"
        aria-expanded={dockVisible}
        aria-label={translate(dockVisible ? "dock.hide" : "dock.show")}
        title={translate(dockVisible ? "dock.hide" : "dock.show")}
        data-dock-toggle="true"
        style={{
          marginLeft: !showChat ? "auto" : 0,
          display: "flex", alignItems: "center", justifyContent: "center",
          width: TOP_BAR_ICON_BUTTON_SIZE, height: "100%", padding: 0,
          background: dockVisible ? "var(--bg-selected)" : "none",
          border: "none", borderLeft: showChat ? "1px solid var(--border)" : "none",
          color: dockVisible ? "var(--text)" : "var(--text-muted)",
          cursor: "pointer", flexShrink: 0, transition: "color 0.12s, background 0.12s",
        }}
        onMouseEnter={(event) => { event.currentTarget.style.color = "var(--text)"; }}
        onMouseLeave={(event) => { event.currentTarget.style.color = dockVisible ? "var(--text)" : "var(--text-muted)"; }}
      >
        <DockPanelIcon size={16} />
      </button>
    );
  };

  return (
    <>
    <style>{`
      @keyframes session-info-pop {
        0% {
          opacity: 0;
          transform: translateY(-24px);
          filter: blur(6px);
          box-shadow: 0 2px 8px rgba(0,0,0,0);
        }
        55% {
          opacity: 1;
          transform: translateY(0);
          filter: blur(0);
          background: color-mix(in srgb, var(--accent) 8%, var(--bg-panel));
          box-shadow: 0 18px 44px rgba(37,99,235,0.16);
        }
        100% {
          opacity: 1;
          transform: translateY(0);
          filter: blur(0);
          background: var(--bg-panel);
          box-shadow: 0 10px 28px rgba(0,0,0,0.10);
        }
      }
      @keyframes session-info-light-wash {
        0% {
          opacity: 0;
          transform: translateX(-110%) skewX(-16deg);
        }
        24% {
          opacity: 0.42;
        }
        100% {
          opacity: 0;
          transform: translateX(115%) skewX(-16deg);
        }
      }
      .session-info-popover {
        position: relative;
        overflow: hidden;
        transform-origin: top right;
        animation: session-info-pop 360ms ease-out both;
        will-change: transform, opacity, filter, background, box-shadow;
      }
      .session-info-popover::after {
        content: "";
        position: absolute;
        top: 0;
        bottom: 0;
        left: 0;
        width: 44%;
        pointer-events: none;
        background: linear-gradient(90deg, transparent, color-mix(in srgb, var(--accent) 24%, transparent), transparent);
        animation: session-info-light-wash 620ms ease-out both;
      }
      @media (prefers-reduced-motion: reduce) {
        .session-info-popover,
        .session-info-popover::after {
          animation: none;
        }
      }
      .mobile-session-stats {
        container-type: inline-size;
      }
      @container (max-width: 158px) {
        .mobile-session-stat-io {
          display: none !important;
        }
      }
      @container (max-width: 88px) {
        .mobile-session-stat-cost {
          display: none !important;
        }
      }
      @media (max-width: 640px) {
        .sidebar-overlay-backdrop.sidebar-mobile-pending {
          opacity: 0 !important;
          pointer-events: none !important;
        }
        .sidebar-container.sidebar-mobile-pending.sidebar-open {
          transform: translateX(calc(-100% - env(safe-area-inset-left)));
          box-shadow: none;
        }
      }
    `}</style>
    <div style={{
      display: "flex",
      width: "100%",
      height: "var(--app-viewport-height, 100dvh)",
      paddingLeft: "env(safe-area-inset-left)",
      paddingRight: "env(safe-area-inset-right)",
      overflow: "hidden",
      background: "var(--bg)",
    }}>
      {/* Mobile overlay backdrop */}
      <div
        className={`sidebar-overlay-backdrop${mobileSidebarReady ? "" : " sidebar-mobile-pending"}`}
        onClick={() => setSidebarOpen(false)}
        style={{
          position: "fixed",
          inset: 0,
          zIndex: 199,
          background: "rgba(0,0,0,0.4)",
          opacity: sidebarOpen ? 1 : 0,
          pointerEvents: sidebarOpen ? "auto" : "none",
          transition: "opacity 0.25s ease",
        }}
      />

      {/* Left sidebar */}
      <div
        ref={sidebarResizer.panelRef}
        id="session-sidebar"
        inert={dockFullWidth}
        className={`sidebar-container${sidebarOpen ? " sidebar-open" : " sidebar-closed"}${mobileSidebarReady ? "" : " sidebar-mobile-pending"}${sidebarResizer.isResizing ? " sidebar-resizing" : ""}`}
        style={{
          "--sidebar-width": `${sidebarResizer.width}px`,
          background: "var(--bg-panel)",
          borderRight: "1px solid var(--border)",
          display: "flex",
          flexDirection: "column",
          flexShrink: 0,
          paddingTop: "env(safe-area-inset-top)",
          paddingBottom: "env(safe-area-inset-bottom)",
          zIndex: 200,
        } as React.CSSProperties}
      >
        {sidebarContent}
      </div>
      {sidebarOpen && (
        <div
          {...sidebarResizer.separatorProps}
          inert={dockFullWidth}
          aria-controls="session-sidebar"
          className={`panel-resize-handle sidebar-resize-handle${sidebarResizer.isResizing ? " is-resizing" : ""}`}
          data-resize-handle="sidebar"
          title={`${translate("layout.resizeSidebar")}: ${translate("layout.resizeHint")}`}
        />
      )}

      {/* Center: chat */}
      <div inert={dockFullWidth} style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", minWidth: 0 }}>
        {/* Top bar with sidebar toggle */}
        <div ref={topBarRef} style={{ flexShrink: 0, background: "var(--bg-panel)" }}>
        <div
          data-toolbar-tight={tabSlot.compact ? "true" : undefined}
          style={{ display: "flex", alignItems: "center", position: "relative", borderBottom: "1px solid var(--border)", height: "calc(36px + env(safe-area-inset-top))", paddingTop: "env(safe-area-inset-top)" }}
        >
          {/* The two groups the centred view switch keeps clear of (see tabSlot). */}
          <div
            ref={topBarLeftRef}
            data-top-bar-group="left"
            style={{
              display: "flex",
              alignItems: "stretch",
              height: "100%",
              flexShrink: 0,
              borderRight: isMobile ? undefined : "1px solid var(--border)",
            }}
          >
          <button
            onClick={handleSidebarToggle}
             title={sidebarOpen ? translate("sidebar.hide") : translate("sidebar.show")}
             aria-label={sidebarOpen ? translate("sidebar.hide") : translate("sidebar.show")}
            style={{
              display: "flex", alignItems: "center", justifyContent: "center",
              width: TOP_BAR_ICON_BUTTON_SIZE, height: TOP_BAR_ICON_BUTTON_SIZE, padding: 0,
              background: "none", border: "none", borderRight: "1px solid var(--border)",
              color: "var(--text-muted)", cursor: "pointer", flexShrink: 0, transition: "color 0.12s",
            }}
            onMouseEnter={(e) => { e.currentTarget.style.color = "var(--text)"; }}
            onMouseLeave={(e) => { e.currentTarget.style.color = "var(--text-muted)"; }}
          >
            {sidebarOpen ? (
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <rect x="3" y="3" width="18" height="18" rx="2" /><line x1="9" y1="3" x2="9" y2="21" />
              </svg>
            ) : (
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                <line x1="3" y1="6" x2="21" y2="6" /><line x1="3" y1="12" x2="21" y2="12" /><line x1="3" y1="18" x2="21" y2="18" />
              </svg>
            )}
          </button>
          {!isMobile && (
            <>
              {renderProjectTrustWarning(false)}
              {renderChatToolbarActions(false)}
            </>
          )}
          </div>
          {!isMobile && (
            <div
              ref={tabSlotRef}
              className="main-view-tabs-slot"
              style={{
                position: "absolute",
                // Centred in the bar, and kept clear of both button groups so a
                // narrow chat (an open dock) never slides it under them.
                left: tabSlot.left,
                top: "env(safe-area-inset-top)",
                bottom: 0,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                overflow: "hidden",
                zIndex: 6,
                pointerEvents: "none",
              }}
            >
              <div style={{ pointerEvents: "auto", minWidth: 0 }}>
                <MainViewTabs value={mainView} onChange={handleMainViewChange} compact={tabSlot.compact} />
              </div>
            </div>
          )}
          {isMobile && (
            <div
              ref={mobileToolbarRef}
              data-mobile-toolbar="true"
              style={{
                position: "relative",
                display: "flex",
                alignItems: "stretch",
                flex: 1,
                minWidth: 0,
                height: "100%",
              }}
            >
              {isNarrowMobile && renderAgentsButton(true)}
              {isNarrowMobile && (
                <button
                  type="button"
                  onClick={handleMobileToolbarMoreToggle}
                  title={moreControlsLabel}
                  aria-label={moreControlsLabel}
                  aria-controls="mobile-toolbar-actions"
                  aria-expanded={mobileToolbarMoreOpen}
                  data-mobile-toolbar-more="true"
                  style={{
                    position: "relative",
                    zIndex: mobileToolbarMoreOpen ? 21 : undefined,
                    display: "flex", alignItems: "center", justifyContent: "center",
                    width: TOP_BAR_ICON_BUTTON_SIZE, height: TOP_BAR_ICON_BUTTON_SIZE, padding: 0,
                    background: mobileToolbarMoreOpen ? "var(--bg-selected)" : "none",
                    border: "none", borderRight: "1px solid var(--border)",
                    color: mobileToolbarMoreOpen ? "var(--text)" : "var(--text-muted)",
                    cursor: "pointer", flexShrink: 0, transition: "color 0.12s, background 0.12s",
                  }}
                >
                  {mobileToolbarMoreOpen ? (
                    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
                      <line x1="5" y1="5" x2="19" y2="19" /><line x1="19" y1="5" x2="5" y2="19" />
                    </svg>
                  ) : (
                    <svg width="17" height="17" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                      <circle cx="5" cy="12" r="1.5" /><circle cx="12" cy="12" r="1.5" /><circle cx="19" cy="12" r="1.5" />
                    </svg>
                  )}
                </button>
              )}
              <div
                className="main-view-tabs-mobile"
                style={{ display: "flex", alignItems: "center", flexShrink: 0, padding: "0 4px" }}
              >
                <MainViewTabs compact value={mainView} onChange={handleMainViewChange} />
              </div>
              {!isNarrowMobile && renderChatToolbarActions(true)}
              {!isNarrowMobile && renderArtifactsButton(true)}
              {!isNarrowMobile && renderDebugButton(true)}
              {renderSessionStatsButton(true)}
                  {isNarrowMobile && mobileToolbarMoreOpen && (
                <div
                  id="mobile-toolbar-actions"
                  role="toolbar"
                  aria-label={translate("chat.moreControls")}
                  data-mobile-toolbar-actions="true"
                  style={{
                    position: "absolute",
                    top: 0,
                    right: 0,
                    bottom: 0,
                    // Right of the Agents and More buttons, which stay in place.
                    left: `calc(2 * ${TOP_BAR_ICON_BUTTON_SIZE})`,
                    zIndex: 20,
                    display: "flex",
                    alignItems: "stretch",
                    // More buttons than a 320px phone fits (the Debug button made seven): scroll, never clip.
                    overflowX: "auto",
                    overflowY: "hidden",
                    scrollbarWidth: "none",
                    background: "color-mix(in srgb, var(--bg-panel) 94%, var(--bg))",
                    boxShadow: "4px 0 18px rgba(0,0,0,0.12)",
                    backdropFilter: "blur(10px)",
                  }}
                >
                  {renderChatToolbarActions(true)}
                  {renderArtifactsButton(true)}
                  {renderDebugButton(true)}
                </div>
              )}
            </div>
          )}
          <div
            ref={topBarRightRef}
            data-top-bar-group="right"
            style={{
              display: "flex",
              alignItems: "stretch",
              height: "100%",
              marginLeft: "auto",
              flexShrink: 0,
              borderLeft: !isMobile && (showChat || canDock) ? "1px solid var(--border)" : undefined,
            }}
          >
          {!isMobile && (
            <>
              {renderArtifactsButton(false)}
              {renderDebugButton(false)}
              {renderSessionStatsButton(false)}
            </>
          )}
          {!isMobile && renderDockButton()}
          </div>
          {isMobile && sessionHasBranches && (
            <BranchNavigator
              tree={branchTree}
              activeLeafId={branchActiveLeafId}
              onLeafChange={handleBranchLeafChange}
              locked={branchSwitchLocked}
              inline
              compact
              containerRef={topBarRef}
              open={activeTopPanel === "branches"}
              onToggle={() => toggleTopPanel("branches")}
              hasSession={showChat}
              hideInlineButton
            />
          )}
          {/* Top panel dropdown — shared, only one active at a time */}
          {activeTopPanel && topPanelPos && (
            <div style={{
              position: "fixed",
              top: topPanelPos.top,
              left: topPanelPos.left,
              width: topPanelPos.width,
              maxHeight: `calc(100dvh - ${topPanelPos.top}px)`,
              overflowY: "auto",
              zIndex: 500,
            }}>
              {activeTopPanel === "agents" && activeSessionFamily && selectedSession && (
                <AgentSessionPanel
                  rootSession={activeSessionFamily.root}
                  subagents={activeSessionFamily.subagents}
                  selectedSessionId={selectedSession.id}
                  runningSessionIds={runningSessionIds}
                  onSelectSession={handleSelectSession}
                  openSubagentTabIds={openSubagentTabIds}
                  onToggleSubagentTab={handleToggleSubagentTab}
                />
              )}
              {activeTopPanel === "session" && (
                <div className="session-info-popover" style={{
                  background: "var(--bg-panel)",
                  borderBottom: "1px solid var(--border)",
                  boxShadow: "0 10px 28px rgba(0,0,0,0.10)",
                  padding: "12px 16px",
                }}>
                  {sessionStats ? (() => {
                    const totalActiveMs = sessionStats.totalActiveMs ?? 0;
                    const ws = selectedSession;
                    const sessionRows = [
                       ...(sessionStats.sessionName ? [{ label: translate("session.name"), value: sessionStats.sessionName, copyField: null }] : []),
                       { label: translate("session.file"), value: sessionStats.sessionFile ?? translate("session.inMemory"), copyField: "file" as const },
                       { label: translate("session.id"), value: sessionStats.sessionId, copyField: "id" as const },
                       ...(totalActiveMs > 0 ? [{ label: translate("session.totalActive"), value: formatActiveDuration(totalActiveMs), copyField: null }] : []),
                    ];
                    const projectRows = [
                      ...(ws ? [{ label: translate("session.projectDir"), value: ws.projectRoot ?? ws.cwd, copyField: "projectDir" as const }] : []),
                      ...(ws?.branch ? [{ label: translate("session.gitBranch"), value: ws.branch, copyField: "gitBranch" as const }] : []),
                      ...(ws?.isWorktree ? [{ label: translate("session.gitWorktree"), value: ws.cwd, copyField: "gitWorktree" as const }] : []),
                    ];
                    const messageRows = [
                       [translate("session.user"), sessionStats.userMessages.toLocaleString(locale)],
                       [translate("session.assistant"), sessionStats.assistantMessages.toLocaleString(locale)],
                       [translate("session.toolCalls"), sessionStats.toolCalls.toLocaleString(locale)],
                       [translate("session.toolResults"), sessionStats.toolResults.toLocaleString(locale)],
                       [translate("session.total"), sessionStats.totalMessages.toLocaleString(locale)],
                    ];
                    const tokenRows = [
                       [translate("session.input"), sessionStats.tokens.input.toLocaleString(locale)],
                       [translate("session.output"), sessionStats.tokens.output.toLocaleString(locale)],
                       ...(sessionStats.tokens.cacheRead > 0 ? [[translate("session.cacheRead"), sessionStats.tokens.cacheRead.toLocaleString(locale)]] : []),
                       ...(sessionStats.tokens.cacheWrite > 0 ? [[translate("session.cacheWrite"), sessionStats.tokens.cacheWrite.toLocaleString(locale)]] : []),
                       [translate("session.total"), sessionStats.tokens.total.toLocaleString(locale)],
                    ];
                    const ctx = contextUsage ?? sessionStats.contextUsage;
                    const extraTokenRows = [
                       ...(sessionStats.cost > 0 ? [[translate("session.cost"), `$${sessionStats.cost.toFixed(4)}`]] : []),
                       ...(ctx?.contextWindow ? [[translate("session.context"), `${ctx.percent !== null ? `${ctx.percent.toFixed(1)}%` : "?"} / ${formatCompactCount(ctx.contextWindow)}`]] : []),
                       // Cache hit rate = cache reads / (input + cache writes + cache reads) — the denominator covers all input-class tokens.
                       ...(sessionStats.tokens.cacheRead + sessionStats.tokens.cacheWrite > 0 && sessionStats.tokens.cacheRead + sessionStats.tokens.cacheWrite + sessionStats.tokens.input > 0
                         ? [[translate("session.cacheHitRate"), `${(sessionStats.tokens.cacheRead / (sessionStats.tokens.cacheRead + sessionStats.tokens.cacheWrite + sessionStats.tokens.input) * 100).toFixed(1)}%`]]
                         : []),
                    ];
                    const section = (
                      title: string,
                      sectionRows: string[][],
                      valueAlign: "left" | "right" = "left",
                      compact = false,
                    ) => (
                        <div style={{ minWidth: 0 }}>
                          <div style={{ fontSize: 11, fontWeight: 700, color: "var(--text)", marginBottom: 6 }}>{title}</div>
                          <div style={{
                            display: "grid",
                            gridTemplateColumns: compact ? "max-content max-content" : "auto minmax(0, 1fr)",
                            columnGap: compact ? 14 : 12,
                            rowGap: 4,
                            justifyContent: compact ? "start" : undefined,
                          }}>
                            {sectionRows.map(([label, value]) => (
                              <div key={`${title}:${label}`} style={{ display: "contents" }}>
                                <div style={{ color: "var(--text-dim)", whiteSpace: "nowrap" }}>{label}</div>
                                <div style={{
                                  color: "var(--text-muted)",
                                  minWidth: 0,
                                  overflowWrap: compact ? "normal" : "anywhere",
                                  textAlign: valueAlign,
                                  whiteSpace: valueAlign === "right" ? "nowrap" : "normal",
                                }}>{value}</div>
                              </div>
                            ))}
                          </div>
                        </div>
                      );
                    const copyTitleKey: Record<SessionCopyField, string> = {
                      file: "session.copyFile",
                      id: "session.copyId",
                      projectDir: "session.copyProjectDir",
                      gitBranch: "session.copyGitBranch",
                      gitWorktree: "session.copyGitWorktree",
                    };
                    const copyButton = (field: SessionCopyField, value: string) => {
                      const copied = copiedSessionField === field;
                      return (
                        <button
                          type="button"
                          title={copied ? translate("session.copied") : translate(copyTitleKey[field])}
                          onClick={() => handleCopySessionField(field, value)}
                          style={{
                            alignSelf: "start",
                            display: "inline-flex",
                            alignItems: "center",
                            justifyContent: "center",
                            width: 22,
                            height: 22,
                            marginTop: -2,
                            color: copied ? "var(--accent)" : "var(--text-dim)",
                            background: "transparent",
                            border: "1px solid var(--border)",
                            borderRadius: 4,
                            cursor: "pointer",
                            flex: "0 0 auto",
                            transition: "color 0.12s, border-color 0.12s, background 0.12s",
                          }}
                          onMouseEnter={(e) => {
                            e.currentTarget.style.color = "var(--accent)";
                            e.currentTarget.style.borderColor = "var(--accent)";
                            e.currentTarget.style.background = "var(--bg-hover)";
                          }}
                          onMouseLeave={(e) => {
                            e.currentTarget.style.color = copied ? "var(--accent)" : "var(--text-dim)";
                            e.currentTarget.style.borderColor = "var(--border)";
                            e.currentTarget.style.background = "transparent";
                          }}
                        >
                          {copied ? (
                            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                              <polyline points="20 6 9 17 4 12" />
                            </svg>
                          ) : (
                            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                              <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
                              <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                            </svg>
                          )}
                        </button>
                      );
                    };
                    const sessionInfoSection = (
                      <div style={{ minWidth: 0 }}>
                         <div style={{ fontSize: 11, fontWeight: 700, color: "var(--text)", marginBottom: 6 }}>{translate("session.infoSection")}</div>
                        <div style={{ display: "grid", gridTemplateColumns: "auto minmax(0, 1fr) auto", columnGap: 12, rowGap: 8, alignItems: "start" }}>
                          {sessionRows.map((row) => (
                            <div key={`session-info:${row.label}`} style={{ display: "contents" }}>
                              <div style={{ color: "var(--text-dim)", whiteSpace: "nowrap" }}>{row.label}</div>
                              <div style={{
                                color: "var(--text-muted)",
                                minWidth: 0,
                                overflowWrap: "anywhere",
                                wordBreak: "break-word",
                                whiteSpace: "normal",
                              }}>{row.value}</div>
                              <div>{row.copyField ? copyButton(row.copyField, row.value) : null}</div>
                            </div>
                          ))}
                        </div>
                      </div>
                    );
                    const projectInfoSection = projectRows.length > 0 ? (
                      <div style={{ minWidth: 0 }}>
                        <div style={{ fontSize: 11, fontWeight: 700, color: "var(--text)", marginBottom: 6 }}>{translate("session.projectSection")}</div>
                        <div style={{ display: "grid", gridTemplateColumns: "auto minmax(0, 1fr) auto", columnGap: 12, rowGap: 8, alignItems: "start" }}>
                          {projectRows.map((row) => (
                            <div key={`project-info:${row.label}`} style={{ display: "contents" }}>
                              <div style={{ color: "var(--text-dim)", whiteSpace: "nowrap" }}>{row.label}</div>
                              <div style={{
                                color: "var(--text-muted)",
                                minWidth: 0,
                                overflowWrap: "anywhere",
                                wordBreak: "break-word",
                                whiteSpace: "normal",
                              }}>{row.value}</div>
                              <div>{row.copyField ? copyButton(row.copyField, row.value) : null}</div>
                            </div>
                          ))}
                        </div>
                      </div>
                    ) : null;

                    return (
                      <div style={{
                        display: "grid",
                        gridTemplateColumns: isMobile
                          ? "1fr"
                          : "minmax(360px, 1.7fr) minmax(140px, 0.55fr) minmax(190px, 0.75fr)",
                        gap: isMobile ? 16 : 24,
                        fontSize: 12,
                        lineHeight: 1.5,
                        fontFamily: "var(--font-mono)",
                      }}>
                        <div style={{ display: "flex", flexDirection: "column", gap: isMobile ? 16 : 20 }}>
                          {sessionInfoSection}
                          {projectInfoSection}
                        </div>
                         {section(translate("session.messages"), messageRows)}
                         {section(translate("session.tokens"), [...tokenRows, ...extraTokenRows], "right", true)}
                      </div>
                    );
                  })() : (
                    <div style={{ fontSize: 12, color: "var(--text-muted)", fontStyle: "italic" }}>
                       {translate("session.load")}
                    </div>
                  )}
                </div>
              )}
              {activeTopPanel === "debug" && (
                <DebugPanelBoundary fallback={translate("debug.failed")}>
                  <DebugPanel
                    readerRef={debugSnapshotReaderRef}
                    selectedSessionId={selectedSession?.id ?? null}
                    sessions={sessionsWithSelection}
                    onClose={closeDebugPanel}
                  />
                </DebugPanelBoundary>
              )}
            </div>
          )}

        </div>
        {isMobile && renderProjectTrustWarning(true)}
        </div>

        {/* Chat content */}
        <div style={{ flex: 1, overflow: "hidden", position: "relative" }}>
          <div style={{ display: mainView === "chat" ? "flex" : "none", flexDirection: "column", height: "100%", minHeight: 0 }}>
          {showChat ? (
            <div style={{ display: "flex", flex: 1, minHeight: 0, minWidth: 0 }}>
              {artifactsOpen && mainView === "chat" && artifactsOverlay && (
                <div className="artifacts-overlay-backdrop" aria-hidden="true" onClick={closeArtifacts} />
              )}
              <div style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column" }}>
                <ChatWindow
                  key={sessionKey}
              session={selectedSession}
              searchTarget={searchTarget?.sessionId === selectedSession?.id ? searchTarget : null}
              onSearchTargetHandled={handleSearchTargetHandled}
              initialScrollPosition={selectedSession ? sessionScrollPositionsRef.current.get(selectedSession.id) ?? null : null}
              onScrollPositionChange={handleSessionScrollPositionChange}
              sessionRunning={Boolean(selectedSession && runningSessionIds.has(selectedSession.id))}
              newSessionCwd={effectiveNewSessionCwd}
              newSessionDraftKey={newSessionDraftKey}
              newSessionContextBar={newSessionContextBar}
              initialNewSessionChoices={selectedSession === null ? carriedNewSessionChoices : null}
              onNewSessionChoicesChange={handleNewSessionChoicesChange}
              onAgentEnd={handleAgentEnd}
              onToolEnd={handleToolEnd}
              onAttentionNeeded={handleAttentionNeeded}
              onSessionCreated={handleSessionCreated}
              onSessionForked={handleSessionForked}
              modelsRefreshKey={modelsRefreshKey}
              chatInputRef={chatInputRef}
              onBranchDataChange={handleBranchDataChange}
              onSystemPromptChange={handleSystemPromptChange}
              onSystemToolsChange={handleSystemToolsChange}
              onSystemInfoLoaderChange={handleSystemInfoLoaderChange}
              onDebugSnapshotReaderChange={handleDebugSnapshotReaderChange}
              onSessionStatsChange={handleSessionStatsChange}
              onSessionStatsPanelOpen={openSessionStatsPanel}
              onOpenSettings={openSettingsSection}
              onContextUsageChange={handleContextUsageChange}
                  onOpenFile={handleOpenArtifactFile}
                  onArtifactsChange={handleArtifactsChange}
                  onFilesUploaded={handleExplorerRefresh}
              onOpenSubagent={handleOpenSubagentTab}
              onAskInNewChat={handleAskInNewChat}
              quoteSelectionEnabled={quoteSelectionEnabled}
              initialPrompt={pendingQuotePrompt?.sessionId === selectedSession?.id ? pendingQuotePrompt?.text : undefined}
              onInitialPromptConsumed={() => setPendingQuotePrompt(null)}
              soundEnabled={soundEnabled}
              onSoundToggle={onSoundToggle}
              playDoneSound={playDoneSound}
              unlockAudio={unlockAudio}
                  holdAudio={holdAudio}
                />
              </div>
              {artifactsOpen && mainView === "chat" && (
                <>
                  {!artifactsOverlay && (
                    <div
                      {...artifactsResizer.separatorProps}
                      aria-controls="session-artifacts"
                      className={`panel-resize-handle artifacts-resize-handle${artifactsResizer.isResizing ? " is-resizing" : ""}`}
                      data-resize-handle="artifacts"
                      title={`${translate("artifacts.resize")}: ${translate("layout.resizeHint")}`}
                    />
                  )}
                  <SessionArtifactsPane
                    ref={artifactsResizer.panelRef}
                    sessionId={selectedSession?.id ?? null}
                    cwd={artifactsCwd}
                    artifacts={sessionArtifacts}
                    scanning={artifactsScanning}
                    pane={artifactPane}
                    overlay={artifactsOverlay}
                    runningSessionIds={runningSessionIds}
                    onChange={(next) => updateArtifactPane(() => next)}
                    onOpenFile={handleOpenArtifactFile}
                    onOpenSubagent={handleOpenSubagentTab}
                    onOpenInCode={handleOpenArtifactInCode}
                    onOpenCodeView={handleOpenArtifactsCodeView}
                    onViewerStateChange={handleArtifactViewerStateChange}
                    onClose={closeArtifacts}
                    onAtMention={handleAtMention}
                    onMentionLines={handleFileLineMention}
                    style={{ "--artifacts-width": `${artifactsResizer.width}px` } as React.CSSProperties}
                  />
                </>
              )}
            </div>
          ) : initialCwdStatus === "validating" ? (
            <div
              role="status"
              style={{ height: "100%", display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 8, padding: 24, color: "var(--text-muted)", textAlign: "center" }}
            >
               <div style={{ fontSize: 14, color: "var(--text)" }}>{translate("workspace.opening")}</div>
              <div style={{ maxWidth: "min(720px, 100%)", overflowWrap: "anywhere", fontFamily: "var(--font-mono)", fontSize: 12 }}>
                {initialNavigation.requestedCwd}
              </div>
            </div>
          ) : initialCwdStatus === "error" ? (
            <div
              role="alert"
              style={{ height: "100%", display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 8, padding: 24, color: "var(--text-muted)", textAlign: "center" }}
            >
               <div style={{ fontSize: 14, color: "#dc2626" }}>{translate("workspace.unable")}</div>
              <div style={{ maxWidth: "min(720px, 100%)", overflowWrap: "anywhere", fontFamily: "var(--font-mono)", fontSize: 12 }}>
                {initialNavigation.requestedCwd}
              </div>
              <div style={{ maxWidth: 720, fontSize: 12 }}>{initialCwdError}</div>
              <div style={{ display: "flex", gap: 8, marginTop: 6 }}>
                <button type="button" onClick={retryInitialWorkspace} style={{ padding: "6px 12px", borderRadius: 7, border: "1px solid var(--border)", background: "var(--bg-panel)", color: "var(--text)", cursor: "pointer" }}>
                  {translate("common.retry")}
                </button>
                <button type="button" onClick={dismissInitialWorkspace} style={{ padding: "6px 12px", borderRadius: 7, border: "1px solid var(--border)", background: "transparent", color: "var(--text-muted)", cursor: "pointer" }}>
                  {translate("workspace.selectProject")}
                </button>
              </div>
            </div>
          ) : showPlaceholder ? (
            activeCwd ? (
              <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-muted)", fontSize: 15 }}>
                 {translate("workspace.selectSession")}
              </div>
            ) : (
              <div style={{ position: "absolute", top: 12, left: 12, display: "flex", alignItems: "flex-start", gap: 8, userSelect: "none", pointerEvents: "none" }}>
                <svg width="44" height="44" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.7, flexShrink: 0 }}>
                  <line x1="20" y1="12" x2="4" y2="12" /><polyline points="10 6 4 12 10 18" />
                </svg>
                <div>
                   <div style={{ fontSize: 18, fontWeight: 600, color: "var(--text)", marginBottom: 8 }}>{translate("workspace.getStarted")}</div>
                  <div style={{ fontSize: 12, color: "var(--text-muted)", lineHeight: 1.8 }}>
                     <span style={{ color: "var(--text-dim)", marginRight: 6 }}>1.</span>{translate("workspace.selectProject")}<br />
                     <span style={{ color: "var(--text-dim)", marginRight: 6 }}>2.</span>{translate("workspace.addModels")}
                  </div>
                </div>
              </div>
            )
          ) : null}
          </div>
          {traceMounted && (
            <div style={{ display: mainView === "trace" ? "flex" : "none", height: "100%", minHeight: 0, minWidth: 0 }}>
              <TraceView
                sessionId={selectedSession?.id ?? null}
                running={Boolean(selectedSession && runningSessionIds.has(selectedSession.id))}
                tools={systemTools}
                isMobile={isMobile}
                active={mainView === "trace"}
                // The session's own fingerprint, refreshed with the catalog row
                // on every list reload: the trace reloads when the log grows
                // rather than only while a run is in flight.
                revision={
                  selectedSession
                    ? `${selectedSession.modified}:${selectedSession.messageCount}`
                    : ""
                }
              />
            </div>
          )}
          {codeMounted && (
            <div style={{ display: mainView === "code" ? "flex" : "none", height: "100%", minHeight: 0 }}>
              <CodeWorkspace
                root={codeRoot}
                projectKey={codeProjectIdentity ?? null}
                projectName={codeRoot ? getFileName(codeRoot) || codeRoot : null}
                state={codeWorkspace}
                active={mainView === "code"}
                isMobile={isMobile}
                refreshKey={explorerRefreshKey}
                onOpenFile={openCodeFile}
                onChange={updateCodeWorkspace}
                onAtMention={handleAtMention}
                onMentionLines={handleFileLineMention}
              />
            </div>
          )}
          {gitMounted && (
            <div style={{ display: mainView === "git" ? "flex" : "none", width: "100%", height: "100%", minWidth: 0, minHeight: 0, flex: 1 }}>
              <GitWorkspace
                projectRoot={gitProjectRoot ?? null}
                state={gitWorkspace}
                active={mainView === "git"}
                isMobile={isMobile}
                onChange={updateGitWorkspace}
              />
            </div>
          )}
          {terminalMounted && (
            <div style={{ display: mainView === "terminal" ? "flex" : "none", width: "100%", height: "100%", minWidth: 0, minHeight: 0, flex: 1 }}>
              <TerminalWorkspace
                projectRoot={gitProjectRoot ?? null}
                defaultCwd={terminalDefaultCwd ?? null}
                state={terminalWorkspace}
                active={mainView === "terminal"}
                isMobile={isMobile}
                onChange={updateTerminalWorkspace}
              />
            </div>
          )}
        </div>
      </div>

      {dockVisible && dockContext && dockState && (
        <div
          {...dockResizer.separatorProps}
          inert={dockFullWidth}
          aria-controls="right-dock"
          className={`panel-resize-handle dock-resize-handle${dockResizer.isResizing ? " is-resizing" : ""}`}
          data-resize-handle="right-dock"
          title={`${translate("dock.resize")}: ${translate("layout.resizeHint")}`}
        />
      )}

      {/* Right dock: a project's own Code/Diff/Terminal views, side by side. */}
      {dockVisible && dockContext && dockState && (
        <RightDock
          ref={dockResizer.panelRef}
          context={dockContext}
          state={dockState}
          // Rendered only when visible, so its leaves are live whenever they exist.
          active
          isMobile={isMobile}
          codeRoot={codeRoot}
          gitRoot={gitProjectRoot}
          defaultCwd={terminalDefaultCwd}
          projectName={codeRoot ? getFileName(codeRoot) || codeRoot : null}
          refreshKey={explorerRefreshKey}
          expanded={dockExpanded}
          onChange={updateDockState}
          onToggleExpanded={handleDockExpandToggle}
          onClose={() => setDockOpen(false)}
          onAtMention={handleAtMention}
          onMentionLines={handleFileLineMention}
          style={{ "--dock-width": `${dockResizer.width}px` } as React.CSSProperties}
        />
      )}
    </div>
    {settingsSection && (
      <SettingsPanel
        cwd={projectTrustCwd}
        sessionId={selectedSession?.id ?? null}
        initialSection={settingsSection}
        openAtHome={settingsOpenAtHome}
        quoteSelectionEnabled={quoteSelectionEnabled}
        onQuoteSelectionChange={handleQuoteSelectionChange}
        onClose={() => {
          setSettingsSection(null);
          setModelsRefreshKey((key) => key + 1);
        }}
        onSessionReloaded={() => setSessionKey((key) => key + 1)}
        projectTrust={projectTrust}
        onOpenTrustDialog={openProjectTrustDialog}
        onProjectTrustChanged={handleProjectTrustChanged}
        systemPrompt={systemPrompt}
        systemTools={systemTools}
        systemInfoLoading={systemInfoLoading}
        onLoadSystemInfo={loadSystemInfo}
      />
    )}
    {/* After Settings, so it opens above it (z-index 1100 over 1000) when Settings › MCP asks for it. */}
    {projectTrustDialogOpen && projectTrustCwd && (
      <ProjectTrustDialog
        cwd={projectTrustCwd}
        busy={projectTrustBusy}
        error={projectTrustError}
        onCancel={() => {
          if (!projectTrustBusy) setProjectTrustDialogOpen(false);
        }}
        onConfirm={() => void handleTrustProject()}
        onStatus={setProjectTrust}
      />
    )}
    </>
  );
}
