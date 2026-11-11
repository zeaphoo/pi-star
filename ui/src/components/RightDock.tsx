"use client";

import { forwardRef, useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import { useI18n } from "@/hooks/useI18n";
import { DockNodeView } from "./DockNodeView";
import {
  DockChevronDownIcon,
  DockCloseIcon,
  DockExpandIcon,
  DockPanelIcon,
  DockPlusIcon,
  SplitColumnIcon,
  SplitRowIcon,
  dockViewIcon,
} from "./dock-icons";
import {
  DOCK_VIEWS,
  addDockView,
  canAddDockLeaf,
  clearDockLeafContent,
  closeDockLeaf,
  dockLeafCount,
  focusDockLeaf,
  focusedDockLeaf,
  splitDockLeaf,
  type DockContext,
  type DockState,
  type DockStateUpdater,
  type DockView,
} from "@/lib/dock-state";
import type { SplitDirection } from "@/lib/split-tree";

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
  onChange: (next: DockStateUpdater) => void;
  onToggleExpanded: () => void;
  onClose: () => void;
  onAtMention?: (relativePath: string, isDir: boolean) => void;
  onMentionLines?: (relativePath: string, startLine: number, endLine: number) => void;
  style?: CSSProperties;
}

/** The dock is tall and narrow; only a genuinely wide pane earns a side-by-side split. */
function autoSplitDirection(leafId: string | null): SplitDirection {
  if (!leafId || typeof document === "undefined") return "column";
  const element = document.querySelector<HTMLElement>(`[data-dock-leaf="${CSS.escape(leafId)}"]`);
  const rect = element?.getBoundingClientRect();
  if (!rect || rect.width <= 0 || rect.height <= 0) return "column";
  return rect.width > rect.height ? "row" : "column";
}

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
  const canSplit = canAddDockLeaf(state);
  const [splitMenuOpen, setSplitMenuOpen] = useState(false);
  const splitMenuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!splitMenuOpen) return;
    const closeOnOutsidePointer = (event: PointerEvent) => {
      if (splitMenuRef.current?.contains(event.target as Node)) return;
      setSplitMenuOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setSplitMenuOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutsidePointer, true);
    document.addEventListener("keydown", closeOnEscape, true);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePointer, true);
      document.removeEventListener("keydown", closeOnEscape, true);
    };
  }, [splitMenuOpen]);

  const handleFocusLeaf = useCallback((leafId: string) => {
    onChange((current) => focusDockLeaf(current, leafId));
  }, [onChange]);

  const handleCloseLeaf = useCallback((leafId: string) => {
    const result = closeDockLeaf(state, leafId);
    for (const terminalId of result.killedTerminalIds) {
      void fetch(`/api/terminal/${encodeURIComponent(terminalId)}`, { method: "DELETE", keepalive: true })
        .catch(() => { /* the shell may already be gone */ });
    }
    onChange(result.state);
  }, [onChange, state]);

  const handleAdd = useCallback((view: DockView) => {
    onChange((current) => addDockView(current, view, "row", context));
  }, [context, onChange]);

  const handleSplit = useCallback((direction?: SplitDirection) => {
    setSplitMenuOpen(false);
    onChange((current) => {
      const focused = focusedDockLeaf(current);
      const targetId = focused?.id ?? "";
      return splitDockLeaf(current, targetId, direction ?? autoSplitDirection(targetId || null), context);
    });
  }, [context, onChange]);

  const handleClearContent = useCallback((leafId: string) => {
    const result = clearDockLeafContent(state, leafId);
    for (const terminalId of result.killedTerminalIds) {
      void fetch(`/api/terminal/${encodeURIComponent(terminalId)}`, { method: "DELETE", keepalive: true })
        .catch(() => { /* the shell may already be gone */ });
    }
    onChange(result.state);
  }, [onChange, state]);

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
        <div ref={splitMenuRef} className="dock-split-control">
          <button
            type="button"
            className="dock-header-button dock-header-split"
            data-dock-split="true"
            disabled={!canSplit}
            onClick={() => handleSplit()}
            title={canSplit ? t("dock.splitPane") : t("dock.limitReached")}
            aria-label={t("dock.splitPane")}
          >
            <DockPlusIcon size={14} />
            <span>{t("dock.splitPane")}</span>
          </button>
          <button
            type="button"
            className="dock-header-button dock-split-toggle"
            data-dock-split-toggle="true"
            disabled={!canSplit}
            onClick={() => setSplitMenuOpen((open) => !open)}
            title={canSplit ? t("dock.splitDirection") : t("dock.limitReached")}
            aria-label={t("dock.splitDirection")}
            aria-haspopup="menu"
            aria-expanded={splitMenuOpen}
          >
            <DockChevronDownIcon size={12} />
          </button>
          {splitMenuOpen && (
            <div className="dock-split-menu" role="menu" aria-label={t("dock.splitDirection")}>
              <button
                type="button"
                role="menuitem"
                className="dock-split-menu-item"
                onClick={() => handleSplit("row")}
              >
                <SplitRowIcon size={13} />
                <span>{t("dock.splitRight")}</span>
              </button>
              <button
                type="button"
                role="menuitem"
                className="dock-split-menu-item"
                onClick={() => handleSplit("column")}
              >
                <SplitColumnIcon size={13} />
                <span>{t("dock.splitDown")}</span>
              </button>
            </div>
          )}
        </div>
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
            onClearContent={handleClearContent}
            onAtMention={onAtMention}
            onMentionLines={onMentionLines}
          />
        ) : (
          <div className="dock-empty" role="group" aria-label={t("dock.pickView")}>
            <div className="dock-empty-title">{t("dock.emptyTitle")}</div>
            <div className="dock-empty-hint">{t("dock.emptyHint")}</div>
            <div className="dock-empty-choices">
              {DOCK_VIEWS.map((view) => (
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