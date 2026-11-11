"use client";

import { forwardRef, useRef, useState, type CSSProperties } from "react";
import { useI18n } from "@/hooks/useI18n";
import { copyText } from "@/lib/clipboard";
import { getFileName } from "@/lib/file-paths";
import { formatShortRelativeTime } from "@/lib/i18n/format";
import type { FileViewerState } from "@/lib/file-viewer-state";
import type { SessionArtifact } from "@/lib/session-artifacts";
import { FileViewer } from "./FileViewer";
import { getFileIcon } from "./FileIcons";
import { SubagentViewer } from "./SubagentViewer";
import { TabBar, type Tab } from "./TabBar";
import { agentTabSessionId } from "./file-tab-state";

export interface ArtifactPaneState {
  tabs: Tab[];
  activeTabId: string | null;
}

export const EMPTY_ARTIFACT_PANE: ArtifactPaneState = { tabs: [], activeTabId: null };

interface Props {
  sessionId: string | null;
  cwd: string | null;
  artifacts: readonly SessionArtifact[];
  /** A full-history scan is running; the live list may still be short. */
  scanning: boolean;
  pane: ArtifactPaneState;
  onChange: (pane: ArtifactPaneState) => void;
  /** Sub-agent tabs report whether their run is live. */
  runningSessionIds: ReadonlySet<string>;
  /**
   * Opens a file in this pane. `sourceSessionId` is the session whose cwd the
   * path belongs to: the pane's own, or a sub-agent's when its tab linked it.
   */
  onOpenFile: (filePath: string, page?: number, sourceSessionId?: string | null) => void;
  /** Opens another sub-agent's transcript beside this one. */
  onOpenSubagent?: (sessionId: string, label: string) => void;
  onOpenInCode: (filePath: string) => void;
  onOpenCodeView: () => void;
  onViewerStateChange: (tabId: string, revision: number, state: FileViewerState) => void;
  onClose: () => void;
  onAtMention?: (relativePath: string, isDir: boolean) => void;
  onMentionLines?: (relativePath: string, startLine: number, endLine: number) => void;
  /** The chat column is too narrow to split: float over it instead. */
  overlay?: boolean;
  style?: CSSProperties;
}

export function ArtifactsIcon({ size = 16, open = false }: { size?: number; open?: boolean }) {
  // Lucide's package / package-open (ISC), inline like the other toolbar icons.
  return open ? (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 22v-9" />
      <path d="M15.17 2.21a1.67 1.67 0 0 1 1.63 0L21 4.57a1.93 1.93 0 0 1 0 3.36L8.82 14.79a1.655 1.655 0 0 1-1.64 0L3 12.43a1.93 1.93 0 0 1 0-3.36z" />
      <path d="M20 13v3.87a2.06 2.06 0 0 1-1.11 1.83l-6 3.08a1.93 1.93 0 0 1-1.78 0l-6-3.08A2.06 2.06 0 0 1 4 16.87V13" />
      <path d="M21 12.43a1.93 1.93 0 0 0 0-3.36L8.83 2.2a1.64 1.64 0 0 0-1.63 0L3 4.57a1.93 1.93 0 0 0 0 3.36l12.18 6.86a1.636 1.636 0 0 0 1.63 0z" />
    </svg>
  ) : (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M11 21.73a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73z" />
      <path d="M12 22V12" />
      <polyline points="3.29 7 12 12 20.71 7" />
      <path d="m7.5 4.27 9 5.15" />
    </svg>
  );
}

function CodeIcon({ size = 13 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="m8 6-6 6 6 6" />
      <path d="m16 6 6 6-6 6" />
    </svg>
  );
}

function CopyIcon({ size = 13 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </svg>
  );
}

function CheckIcon({ size = 13 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="m5 12 5 5L20 7" />
    </svg>
  );
}

function CloseIcon({ size = 15 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M18 6 6 18M6 6l12 12" />
    </svg>
  );
}

/**
 * The session's own right pane: the files this session wrote, and the files
 * opened from its chat. It is not the global file panel: its tabs are parked
 * per session (AppShell), so switching sessions never shows another one's
 * files. See docs (artifacts design) and lib/session-artifacts.ts.
 */
export const SessionArtifactsPane = forwardRef<HTMLDivElement, Props>(function SessionArtifactsPane({
  sessionId,
  cwd,
  artifacts,
  scanning,
  pane,
  onChange,
  runningSessionIds,
  onOpenFile,
  onOpenSubagent,
  onOpenInCode,
  onOpenCodeView,
  onViewerStateChange,
  onClose,
  onAtMention,
  onMentionLines,
  overlay = false,
  style,
}, ref) {
  const { t, locale } = useI18n();
  const activeTab = pane.tabs.find((tab) => tab.id === pane.activeTabId) ?? null;
  const showList = activeTab === null;
  const activeAgentSessionId = activeTab ? agentTabSessionId(activeTab.id) : null;
  const [copiedPath, setCopiedPath] = useState<string | null>(null);
  const copiedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const copyPath = (filePath: string) => {
    void copyText(filePath).then(() => {
      setCopiedPath(filePath);
      if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
      copiedTimerRef.current = setTimeout(() => setCopiedPath(null), 1500);
    }).catch(() => {});
  };

  const now = new Date();
  return (
    <div
      ref={ref}
      id="session-artifacts"
      className={`artifacts-panel${overlay ? " is-overlay" : ""}`}
      style={style}
      role="complementary"
      aria-label={t("artifacts.title")}
    >
      <div className="artifacts-header">
        <button
          type="button"
          className={`artifacts-home${showList ? " is-active" : ""}`}
          onClick={() => onChange({ ...pane, activeTabId: null })}
          aria-pressed={showList}
          title={t("artifacts.title")}
        >
          <ArtifactsIcon size={14} open={showList} />
          <span className="artifacts-home-label">{t("artifacts.title")}</span>
          {artifacts.length > 0 && <span className="artifacts-count" aria-hidden="true">{artifacts.length}</span>}
        </button>
        {pane.tabs.length > 0 && (
          <div className="artifacts-tabs">
            <TabBar
              tabs={pane.tabs}
              activeTabId={pane.activeTabId ?? ""}
              onSelectTab={(id) => onChange({ ...pane, activeTabId: id })}
              onCloseTab={(id) => {
                const remaining = pane.tabs.filter((tab) => tab.id !== id);
                onChange({
                  tabs: remaining,
                  activeTabId: pane.activeTabId === id ? (remaining.at(-1)?.id ?? null) : pane.activeTabId,
                });
              }}
            />
          </div>
        )}
        <span className="artifacts-header-spacer" />
        {activeTab && (
          <button
            type="button"
            className="artifacts-header-button"
            onClick={() => onOpenInCode(activeTab.filePath)}
            title={t("artifacts.openInCode")}
            aria-label={t("artifacts.openInCode")}
          >
            <CodeIcon size={15} />
          </button>
        )}
        <button
          type="button"
          className="artifacts-header-button"
          onClick={onClose}
          title={t("artifacts.hide")}
          aria-label={t("artifacts.hide")}
        >
          <CloseIcon size={15} />
        </button>
      </div>

      <div className="artifacts-body">
        {showList ? (
          <div className="artifacts-list" role="list" aria-label={t("artifacts.listLabel")}>
            {scanning && (
              <div className="artifacts-scanning" role="status">{t("artifacts.scanning")}</div>
            )}
            {artifacts.length === 0 ? (
              <div className="artifacts-empty">
                <span className="artifacts-empty-icon"><ArtifactsIcon size={28} /></span>
                <div className="artifacts-empty-title">{t("artifacts.empty")}</div>
                <div className="artifacts-empty-hint">{t("artifacts.emptyHint")}</div>
                <button type="button" className="artifacts-empty-action" onClick={onOpenCodeView}>
                  {t("artifacts.openCodeView")}
                </button>
              </div>
            ) : artifacts.map((artifact) => (
              <div key={artifact.filePath} className="artifacts-row" role="listitem">
                <button
                  type="button"
                  className="artifacts-row-main"
                  onClick={() => onOpenFile(artifact.filePath)}
                  title={artifact.filePath}
                >
                  <span className="artifacts-row-icon">{getFileIcon(getFileName(artifact.filePath), 15)}</span>
                  <span className="artifacts-row-text">
                    <span className="artifacts-row-name">{getFileName(artifact.filePath)}</span>
                    <span className="artifacts-row-path">{artifact.relativePath}</span>
                  </span>
                  {artifact.lastWriteAt !== undefined && (
                    <span className="artifacts-row-time">
                      {formatShortRelativeTime(new Date(artifact.lastWriteAt), locale, now)}
                    </span>
                  )}
                </button>
                <button
                  type="button"
                  className="artifacts-row-action"
                  onClick={() => onOpenInCode(artifact.filePath)}
                  title={t("artifacts.openInCode")}
                  aria-label={`${t("artifacts.openInCode")}: ${artifact.relativePath}`}
                >
                  <CodeIcon />
                </button>
                <button
                  type="button"
                  className="artifacts-row-action"
                  onClick={() => copyPath(artifact.filePath)}
                  title={t(copiedPath === artifact.filePath ? "artifacts.copied" : "artifacts.copyPath")}
                  aria-label={t(copiedPath === artifact.filePath ? "artifacts.copied" : "artifacts.copyPath")}
                >
                  {copiedPath === artifact.filePath ? <CheckIcon /> : <CopyIcon />}
                </button>
              </div>
            ))}
          </div>
        ) : activeAgentSessionId ? (
          <SubagentViewer
            key={activeAgentSessionId}
            sessionId={activeAgentSessionId}
            fallbackLabel={activeTab.label}
            running={runningSessionIds.has(activeAgentSessionId)}
            onOpenFile={(filePath, page) => onOpenFile(filePath, page, activeAgentSessionId)}
            onOpenSubagent={onOpenSubagent}
          />
        ) : activeTab ? (
          <FileViewer
            key={`${sessionId ?? ""}:${activeTab.id}:${activeTab.viewerRevision ?? 0}`}
            filePath={activeTab.filePath}
            cwd={cwd ?? undefined}
            sourceSessionId={activeTab.sourceSessionId ?? sessionId}
            initialDisplayMode={activeTab.initialDisplayMode}
            initialPage={activeTab.page}
            initialState={activeTab.viewerState}
            watchEnabled
            onStateChange={(state) => onViewerStateChange(activeTab.id, activeTab.viewerRevision ?? 0, state)}
            onOpenFile={(filePath, page) => onOpenFile(filePath, page, activeTab.sourceSessionId ?? sessionId)}
            onAtMention={onAtMention}
            onMentionLines={onMentionLines}
          />
        ) : null}
      </div>
    </div>
  );
});
