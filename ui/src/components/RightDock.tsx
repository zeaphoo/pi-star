"use client";

import { forwardRef, useCallback, type CSSProperties } from "react";
import { useI18n } from "@/hooks/useI18n";
import { DockNodeView } from "./DockNodeView";
import {
  DockCloseIcon,
  DockExpandIcon,
  DockPanelIcon,
  dockViewIcon,
} from "./dock-icons";
import {
  addDockView,
  canAddDockView,
  closeDockLeaf,
  dockLeafCount,
  focusDockLeaf,
  type DockContext,
  type DockState,
  type DockView,
} from "@/lib/dock-state";

interface Props {
  context: DockContext;
  state: DockState;
  /** False when the dock is hidden: leaves stop fetching and watching. */
  active: boolean;
  isMobile: boolean;
  codeRoot: string | null;
  gitRoot: string | null;
  defaultCwd: string | null;
  projectName: string | null;
  refreshKey: number;
  expanded: boolean;
  onChange: (next: DockState) => void;
  onToggleExpanded: () => void;
  onClose: () => void;
  onAtMention?: (relativePath: string, isDir: boolean) => void;
  onMentionLines?: (relativePath: string, startLine: number, endLine: number) => void;
  style?: CSSProperties;
}

const VIEWS: DockView[] = ["code", "diff", "terminal"];

/**
 * The right dock: a project's own workbench beside the chat. It holds
 * independent Code, Diff and Terminal instances arranged in a split tree, so a
 * wide screen can show two of them at once without the main view losing its
 * place. Nothing here shares state with the main view.
 */
export const RightDock = forwardRef<HTMLDivElement, Props>(function RightDock({
  context,
  state,
  active,
  isMobile,
  codeRoot,
  gitRoot,
  defaultCwd,
  projectName,
  refreshKey,
  expanded,
  onChange,
  onToggleExpanded,
  onClose,
  onAtMention,
  onMentionLines,
  style,
}, ref) {
  const { t } = useI18n();
  const leafCount = dockLeafCount(state.layout);

  const handleFocusLeaf = useCallback((leafId: string) => {
    onChange(focusDockLeaf(state, leafId));
  }, [onChange, state]);

  const handleCloseLeaf = useCallback((leafId: string) => {
    const result = closeDockLeaf(state, leafId);
    for (const terminalId of result.killedTerminalIds) {
      void fetch(`/api/terminal/${encodeURIComponent(terminalId)}`, { method: "DELETE", keepalive: true })
        .catch(() => { /* the shell may already be gone */ });
    }
    onChange(result.state);
  }, [onChange, state]);

  const handleAdd = useCallback((view: DockView) => {
    onChange(addDockView(state, view, "row", context));
  }, [context, onChange, state]);

  return (
    <div
      ref={ref}
      id="right-dock"
      className={`dock-panel${expanded ? " is-expanded" : ""}`}
      style={style}
      role="complementary"
      aria-label={t("dock.title")}
    >
      <div className="dock-header">
        <span className="dock-header-icon" aria-hidden="true"><DockPanelIcon size={14} /></span>
        <span className="dock-header-title">{t("dock.title")}</span>
        <span className="dock-header-spacer" />
        {VIEWS.map((view) => {
          const disabled = !canAddDockView(state, view);
          return (
            <button
              key={view}
              type="button"
              className="dock-header-button"
              data-dock-add={view}
              disabled={disabled}
              onClick={() => handleAdd(view)}
              title={disabled ? t("dock.limitReached") : t(`dock.add${view === "code" ? "Code" : view === "diff" ? "Diff" : "Terminal"}`)}
              aria-label={t(`dock.add${view === "code" ? "Code" : view === "diff" ? "Diff" : "Terminal"}`)}
            >
              {dockViewIcon(view, 14)}
            </button>
          );
        })}
        <span className="dock-header-divider" aria-hidden="true" />
        <button
          type="button"
          className="dock-header-button"
          onClick={onToggleExpanded}
          aria-pressed={expanded}
          title={t(expanded ? "dock.restoreWidth" : "dock.expand")}
          aria-label={t(expanded ? "dock.restoreWidth" : "dock.expand")}
        >
          <DockExpandIcon size={15} expanded={expanded} />
        </button>
        <button
          type="button"
          className="dock-header-button"
          onClick={onClose}
          title={t("dock.hide")}
          aria-label={t("dock.hide")}
        >
          <DockCloseIcon size={14} />
        </button>
      </div>

      <div className="dock-body">
        {state.layout.root ? (
          <DockNodeView
            node={state.layout.root}
            dock={state}
            context={context}
            active={active}
            isMobile={isMobile}
            refreshKey={refreshKey}
            focusedLeafId={state.layout.focusedLeafId}
            leafCount={leafCount}
            codeRoot={codeRoot}
            gitRoot={gitRoot}
            defaultCwd={defaultCwd}
            projectName={projectName}
            onChange={onChange}
            onFocusLeaf={handleFocusLeaf}
            onCloseLeaf={handleCloseLeaf}
            onAtMention={onAtMention}
            onMentionLines={onMentionLines}
          />
        ) : (
          <div className="dock-empty" role="group" aria-label={t("dock.pickView")}>
            <div className="dock-empty-title">{t("dock.emptyTitle")}</div>
            <div className="dock-empty-hint">{t("dock.emptyHint")}</div>
            <div className="dock-empty-choices">
              {VIEWS.map((view) => (
                <button
                  key={view}
                  type="button"
                  className="dock-empty-choice"
                  data-dock-add={view}
                  onClick={() => handleAdd(view)}
                >
                  <span className="dock-empty-choice-icon">{dockViewIcon(view, 22)}</span>
                  <span>{t(`dock.add${view === "code" ? "Code" : view === "diff" ? "Diff" : "Terminal"}`)}</span>
                </button>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
});