"use client";

import { useCallback, useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { FileIcon } from "./FileIcons";
import { useI18n } from "@/hooks/useI18n";
import type { FileViewerDisplayMode, FileViewerState } from "@/lib/file-viewer-state";

export interface Tab {
  id: string;
  label: string;
  filePath: string;
  kind?: "terminal" | "agent";
  closing?: boolean;
  sourceSessionId?: string | null;
  initialDisplayMode?: FileViewerDisplayMode;
  /** PDF page requested by the link that opened this tab (`#page=N`). */
  page?: number;
  viewerState?: FileViewerState;
  viewerRevision?: number;
}

interface Props {
  tabs: Tab[];
  activeTabId: string;
  onSelectTab: (id: string) => void;
  onCloseTab: (id: string) => void;
  onReorderTab?: (source: string, target: string, after: boolean) => void;
  /** Chevrons that scroll a strip too wide for its row (the code view). */
  scrollControls?: boolean;
  /** Right-click on a tab; the code view opens its tab menu at the pointer. */
  onTabContextMenu?: (tabId: string, event: ReactMouseEvent<HTMLDivElement>) => void;
}

export function TabBar({
  tabs,
  activeTabId,
  onSelectTab,
  onCloseTab,
  onReorderTab,
  scrollControls = false,
  onTabContextMenu,
}: Props) {
  const { t } = useI18n();
  const [hoveredClose, setHoveredClose] = useState<string | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const draggedId = useRef<string | null>(null);
  const [dropTarget, setDropTarget] = useState<{ id: string; after: boolean } | null>(null);
  const [canScrollLeft, setCanScrollLeft] = useState(false);
  const [canScrollRight, setCanScrollRight] = useState(false);

  const updateScrollState = useCallback(() => {
    const list = listRef.current;
    if (!list) return;
    setCanScrollLeft(list.scrollLeft > 1);
    setCanScrollRight(list.scrollLeft + list.clientWidth < list.scrollWidth - 1);
  }, []);

  const scrollTabs = (direction: -1 | 1) => {
    const list = listRef.current;
    if (!list) return;
    list.scrollBy({ left: direction * Math.max(160, list.clientWidth * 0.7), behavior: "smooth" });
  };

  // The strip has no scrollbar (globals.css), so a vertical wheel scrolls it
  // sideways. Non-passive: the page must not scroll as well.
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const onWheel = (event: WheelEvent) => {
      if (event.ctrlKey || Math.abs(event.deltaY) <= Math.abs(event.deltaX) || list.scrollWidth <= list.clientWidth) return;
      event.preventDefault();
      list.scrollLeft += event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? list.clientWidth : 1);
    };
    list.addEventListener("wheel", onWheel, { passive: false });
    return () => list.removeEventListener("wheel", onWheel);
  }, []);

  // The chevrons appear only for a strip that can actually move; a resize or
  // a closed tab changes that without a scroll event.
  useEffect(() => {
    const list = listRef.current;
    if (!list || !scrollControls) return;
    updateScrollState();
    list.addEventListener("scroll", updateScrollState, { passive: true });
    const observer = new ResizeObserver(updateScrollState);
    observer.observe(list);
    return () => {
      list.removeEventListener("scroll", updateScrollState);
      observer.disconnect();
    };
  }, [scrollControls, tabs.length, updateScrollState]);

  // Bring an opened or selected tab into view by moving the strip only:
  // scrollIntoView could also scroll the panels around it.
  useEffect(() => {
    const list = listRef.current;
    const tab = list?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]');
    if (!list || !tab) return;
    const strip = list.getBoundingClientRect();
    const { left, right } = tab.getBoundingClientRect();
    if (left < strip.left) list.scrollLeft -= strip.left - left;
    else if (right > strip.right) list.scrollLeft += right - strip.right;
    updateScrollState();
  }, [activeTabId, tabs.length, updateScrollState]);

  const list = (
    <div
      ref={listRef}
      role="tablist"
      className="tabbar-scroll"
      style={{
        // The scrollbar is hidden (globals.css): it would take its height out
        // of the bar (--top-bar-height), squeezing the tabs or clipping their tops (#1225).
        display: "flex",
        alignItems: "stretch",
        background: "var(--bg-panel)",
        overflowX: "auto",
        overflowY: "hidden",
        flex: scrollControls ? "1 1 auto" : undefined,
        flexShrink: scrollControls ? 1 : 0,
        minWidth: scrollControls ? 0 : undefined,
        height: "var(--top-bar-height)",
      }}
    >
      {tabs.map((tab) => {
        const isActive = tab.id === activeTabId;
        return (
          <div
            key={tab.id}
            role="tab"
            aria-label={tab.kind === "terminal" ? t("terminal.tabLabel", { name: tab.label }) : tab.kind === "agent" ? t("subagent.tabLabel", { name: tab.label }) : tab.label}
            aria-selected={isActive}
            draggable={Boolean(onReorderTab)}
            onDragStart={(event) => {
              if (!onReorderTab) return;
              draggedId.current = tab.id;
              event.dataTransfer.effectAllowed = "move";
              event.dataTransfer.setData("text/plain", tab.id);
            }}
            onDragOver={(event) => {
              if (!draggedId.current || draggedId.current === tab.id) return;
              event.preventDefault();
              event.dataTransfer.dropEffect = "move";
              const rect = event.currentTarget.getBoundingClientRect();
              const after = event.clientX >= rect.left + rect.width / 2;
              setDropTarget((prev) => prev?.id === tab.id && prev.after === after ? prev : { id: tab.id, after });
            }}
            onDrop={(event) => {
              if (!draggedId.current || draggedId.current === tab.id) return;
              event.preventDefault();
              event.stopPropagation();
              const rect = event.currentTarget.getBoundingClientRect();
              onReorderTab?.(draggedId.current, tab.id, event.clientX >= rect.left + rect.width / 2);
              draggedId.current = null;
              setDropTarget(null);
            }}
            onDragLeave={(event) => {
              if (event.currentTarget.contains(event.relatedTarget as Node | null)) return;
              setDropTarget((prev) => prev?.id === tab.id ? null : prev);
            }}
            onDragEnd={() => { draggedId.current = null; setDropTarget(null); }}
            tabIndex={isActive || (!activeTabId && tabs[0].id === tab.id) ? 0 : -1}
            onKeyDown={(event) => {
              if (event.target !== event.currentTarget) return;
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                onSelectTab(tab.id);
              } else if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
                event.preventDefault();
                const index = tabs.findIndex((item) => item.id === tab.id);
                const next = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1
                  : (index + (event.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length;
                onSelectTab(tabs[next].id);
                (event.currentTarget.parentElement?.children[next] as HTMLElement)?.focus();
              }
            }}
            onClick={() => onSelectTab(tab.id)}
            onContextMenu={(event) => {
              if (!onTabContextMenu) return;
              event.preventDefault();
              onTabContextMenu(tab.id, event);
            }}
            onMouseDown={(e) => {
              if (e.button === 1) e.preventDefault();
            }}
            onAuxClick={(e) => {
              if (e.button !== 1) return;
              e.preventDefault();
              e.stopPropagation();
              if (!tab.closing) onCloseTab(tab.id);
            }}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              paddingLeft: 12,
              paddingRight: 6,
              borderRight: "1px solid var(--border)",
              background: isActive ? "var(--bg)" : "var(--bg-panel)",
              cursor: "pointer",
              fontSize: 12,
              color: isActive ? "var(--text)" : "var(--text-muted)",
              whiteSpace: "nowrap",
              maxWidth: 180,
              minWidth: 80,
              flexShrink: 0,
              userSelect: "none",
              transition: "background 0.1s, color 0.1s",
              boxShadow: dropTarget?.id === tab.id ? `inset ${dropTarget.after ? "-2px" : "2px"} 0 var(--accent)` : undefined,
            }}
          >
            <span style={{ flexShrink: 0, opacity: isActive ? 1 : 0.7, display: "flex", alignItems: "center" }}>
              {tab.kind === "terminal" ? (
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <polyline points="4 17 10 11 4 5" /><line x1="12" y1="19" x2="20" y2="19" />
                </svg>
              ) : tab.kind === "agent" ? (
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <rect x="5" y="7" width="14" height="11" rx="2" /><path d="M9 11h.01M15 11h.01M9 15h6M12 7V4M10 4h4" />
                </svg>
              ) : <FileIcon name={tab.label} size={13} />}
            </span>
            <span
              style={{
                overflow: "hidden",
                textOverflow: "ellipsis",
                flex: 1,
                fontWeight: isActive ? 500 : 400,
              }}
              title={tab.filePath}
            >
              {tab.label}
            </span>
            <button
              draggable={false}
              onDragStart={(event) => event.stopPropagation()}
              disabled={tab.closing}
              onClick={(e) => { e.stopPropagation(); onCloseTab(tab.id); }}
              onMouseEnter={() => setHoveredClose(tab.id)}
              onMouseLeave={() => setHoveredClose(null)}
              style={{
                display: "flex", alignItems: "center", justifyContent: "center",
                width: 24, height: 24,
                background: hoveredClose === tab.id ? "var(--bg-hover)" : "transparent",
                border: "none",
                borderRadius: 4,
                color: hoveredClose === tab.id ? "var(--text)" : "var(--text-dim)",
                cursor: "pointer",
                padding: 0,
                flexShrink: 0,
                transition: "background 0.1s, color 0.1s",
              }}
               title={t(tab.kind === "terminal" ? "terminal.close" : "i18n.close")}
               aria-label={`${t(tab.kind === "terminal" ? "terminal.close" : "i18n.close")} ${tab.label}`}
            >
              <svg width="11" height="11" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
                <line x1="2" y1="2" x2="8" y2="8" />
                <line x1="8" y1="2" x2="2" y2="8" />
              </svg>
            </button>
          </div>
        );
      })}
    </div>
  );

  if (!scrollControls) return list;

  return (
    <div
      className="tabbar-scroll-shell"
      style={{
        display: "flex",
        alignItems: "stretch",
        flex: 1,
        minWidth: 0,
        height: "var(--top-bar-height)",
        background: "var(--bg-panel)",
      }}
    >
      <button
        type="button"
        className="tabbar-scroll-button"
        onClick={() => scrollTabs(-1)}
        disabled={!canScrollLeft}
        title={t("code.scrollTabsLeft")}
        aria-label={t("code.scrollTabsLeft")}
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="m15 18-6-6 6-6" />
        </svg>
      </button>
      {list}
      <button
        type="button"
        className="tabbar-scroll-button"
        onClick={() => scrollTabs(1)}
        disabled={!canScrollRight}
        title={t("code.scrollTabsRight")}
        aria-label={t("code.scrollTabsRight")}
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="m9 18 6-6-6-6" />
        </svg>
      </button>
    </div>
  );
}
