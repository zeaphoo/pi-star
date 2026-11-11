"use client";

import { useCallback, useMemo, useRef, type CSSProperties } from "react";
import { useI18n } from "@/hooks/useI18n";
import { CodeWorkspace } from "./CodeWorkspace";
import { GitWorkspace } from "./GitWorkspace";
import { TerminalWorkspace } from "./TerminalWorkspace";
import { SplitColumnIcon, SplitRowIcon, DockCloseIcon, dockViewIcon } from "./dock-icons";
import { openCodeFileTab } from "@/lib/code-workspace";
import { getFileName } from "@/lib/file-paths";
import {
  resizeDockSplit,
  setDockLeafCode,
  setDockLeafDiff,
  setDockLeafTerminal,
  splitDockLeaf,
  type DockContext,
  type DockLeafNode,
  type DockNode,
  type DockSplitNode,
  type DockState,
} from "@/lib/dock-state";
import type { SplitDirection } from "@/lib/split-tree";

const SASH_SIZE = 5;
const MIN_LEAF_RATIO = 0.15;

interface SharedProps {
  dock: DockState;
  context: DockContext;
  active: boolean;
  isMobile: boolean;
  refreshKey: number;
  focusedLeafId: string | null;
  leafCount: number;
  codeRoot: string | null;
  gitRoot: string | null;
  defaultCwd: string | null;
  projectName: string | null;
  onChange: (next: DockState) => void;
  onFocusLeaf: (leafId: string) => void;
  onCloseLeaf: (leafId: string) => void;
  onAtMention?: (relativePath: string, isDir: boolean) => void;
  onMentionLines?: (relativePath: string, startLine: number, endLine: number) => void;
}

function Sash({
  direction,
  containerRef,
  index,
  sizes,
  onResize,
}: {
  direction: SplitDirection;
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
    if (first < MIN_LEAF_RATIO || second < MIN_LEAF_RATIO) return;
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
      className="dock-sash"
      data-dock-sash={direction}
      style={{
        flex: `0 0 ${SASH_SIZE}px`,
        alignSelf: "stretch",
        cursor: direction === "row" ? "col-resize" : "row-resize",
        touchAction: "none",
      }}
    />
  );
}

function DockLeaf({ leaf, ...shared }: SharedProps & { leaf: DockLeafNode }) {
  const { t } = useI18n();
  const {
    dock, context, active, isMobile, refreshKey, focusedLeafId, leafCount,
    codeRoot, gitRoot, defaultCwd, projectName,
    onChange, onFocusLeaf, onCloseLeaf, onAtMention, onMentionLines,
  } = shared;
  const focused = leafCount > 1 && focusedLeafId === leaf.id;

  const title = useMemo(() => {
    if (leaf.view === "code") {
      const state = dock.code[leaf.id];
      const tab = state?.tabs.find((item) => item.id === state.activeTabId);
      return tab?.label ?? t("dock.code");
    }
    if (leaf.view === "diff") {
      const state = dock.diff[leaf.id];
      return state?.selectedRepoPath ? getFileName(state.selectedRepoPath) || t("dock.diff") : t("dock.diff");
    }
    const state = dock.terminal[leaf.id];
    const tab = state?.tabs.find((item) => item.id === state.activeTabId);
    return tab?.title ?? t("dock.terminal");
  }, [dock, leaf.id, leaf.view, t]);

  const split = useCallback((direction: SplitDirection) => {
    onChange(splitDockLeaf(dock, leaf.id, direction, context));
  }, [context, dock, leaf.id, onChange]);

  return (
    <div
      className={`dock-leaf${focused ? " is-focused" : ""}`}
      data-dock-leaf={leaf.id}
      data-dock-view={leaf.view}
      onPointerDownCapture={() => onFocusLeaf(leaf.id)}
      style={{
        display: "flex",
        flexDirection: "column",
        flex: 1,
        minWidth: 0,
        minHeight: 0,
        overflow: "hidden",
        background: "var(--bg)",
      }}
    >
      <div className="dock-leaf-header">
        <span className="dock-leaf-icon" data-view={leaf.view}>{dockViewIcon(leaf.view)}</span>
        <span className="dock-leaf-title" title={title}>{title}</span>
        <button
          type="button"
          className="dock-leaf-action"
          onClick={() => split("row")}
          title={t("dock.splitRight")}
          aria-label={t("dock.splitRight")}
        >
          <SplitRowIcon />
        </button>
        <button
          type="button"
          className="dock-leaf-action"
          onClick={() => split("column")}
          title={t("dock.splitDown")}
          aria-label={t("dock.splitDown")}
        >
          <SplitColumnIcon />
        </button>
        <button
          type="button"
          className="dock-leaf-action dock-leaf-close"
          onClick={() => onCloseLeaf(leaf.id)}
          title={t("dock.closeView")}
          aria-label={t("dock.closeView")}
        >
          <DockCloseIcon />
        </button>
      </div>
      <div style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", overflow: "hidden" }}>
        {leaf.view === "code" ? (
          <CodeWorkspace
            root={codeRoot}
            projectKey={context.projectKey}
            projectName={projectName}
            state={dock.code[leaf.id] ?? null}
            active={active}
            isMobile={isMobile}
            refreshKey={refreshKey}
            instanceId={leaf.id}
            onOpenFile={(filePath, fileName, page) => {
              const current = dock.code[leaf.id];
              if (!current) return;
              onChange(setDockLeafCode(dock, leaf.id, openCodeFileTab(current, filePath, fileName, page)));
            }}
            onChange={(next) => onChange(setDockLeafCode(dock, leaf.id, next))}
            onAtMention={onAtMention}
            onMentionLines={onMentionLines}
          />
        ) : leaf.view === "diff" ? (
          <GitWorkspace
            projectRoot={gitRoot}
            state={dock.diff[leaf.id] ?? null}
            active={active}
            isMobile={isMobile}
            onChange={(next) => onChange(setDockLeafDiff(dock, leaf.id, next))}
          />
        ) : (
          <TerminalWorkspace
            projectRoot={gitRoot}
            defaultCwd={defaultCwd}
            state={dock.terminal[leaf.id] ?? null}
            active={active}
            isMobile={isMobile}
            onChange={(next) => onChange(setDockLeafTerminal(dock, leaf.id, next))}
          />
        )}
      </div>
    </div>
  );
}

function DockSplitView({ split, ...shared }: SharedProps & { split: DockSplitNode }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const style: CSSProperties = {
    flex: 1,
    minWidth: 0,
    minHeight: 0,
    display: "flex",
    flexDirection: split.direction,
  };
  return (
    <div ref={containerRef} style={style} data-dock-split={split.id}>
      {split.children.map((child, index) => (
        <div key={child.id} style={{ display: "contents" }}>
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
            <DockNodeView node={child} {...shared} />
          </div>
          {index < split.children.length - 1 && (
            <Sash
              direction={split.direction}
              containerRef={containerRef}
              index={index}
              sizes={split.sizes}
              onResize={(sizes) => shared.onChange(resizeDockSplit(shared.dock, split.id, sizes))}
            />
          )}
        </div>
      ))}
    </div>
  );
}

export function DockNodeView({ node, ...shared }: SharedProps & { node: DockNode }) {
  if (node.kind === "leaf") return <DockLeaf leaf={node} {...shared} />;
  return <DockSplitView split={node} {...shared} />;
}