"use client";

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { TraceDetail } from "./TraceDetail";
import { TraceRail, TRACE_ROW_HEIGHT } from "./TraceRail";
import { TraceRow } from "./TraceRow";
import { getVisibleRowIndices, revealScrollTop } from "@/lib/session-tree";
import {
  TRACE_FILTER_ALL,
  buildTraceEvents,
  filterTraceEvents,
  formatTraceDuration,
  summarizeTraceEvents,
  type TraceEvent,
  type TraceFilter,
} from "@/lib/trace-events";
import {
  clampTraceDetailWidth,
  getTraceSessionState,
  loadTraceViewPreferences,
  patchTraceSessionState,
  saveTraceViewPreferences,
  type TraceDetailTab,
  type TraceViewAnchor,
} from "@/lib/trace-view-state";
import type { ToolEntry } from "@/lib/tool-presets";
import { useI18n } from "@/hooks/useI18n";

interface Props {
  sessionId: string | null;
  /** The session still has a live run, so an unanswered call is running, not lost. */
  running: boolean;
  /** Active tool definitions; the schema tab reads them. */
  tools: ToolEntry[] | null;
  isMobile: boolean;
  /** False keeps polling and measurement work off a hidden view. */
  active: boolean;
}

interface EntriesResponse {
  entries?: unknown;
  total?: unknown;
  hasMore?: unknown;
  nextAfter?: unknown;
}

/** Entries per request. The server caps this at 2000; 400 keeps a page under a
 *  megabyte even before previews, which matters for the 20 MB sessions. */
const PAGE_LIMIT = 400;
/** Characters the list page keeps per text block. */
const PREVIEW_CHARS = 160;
/** Stop paging here: a pathological session must not lock the tab. */
const MAX_ENTRIES = 20_000;
const OVERSCAN_PX = 240;
const POLL_MS = 3000;

function entryIdOf(entry: unknown): string {
  if (!entry || typeof entry !== "object") return "";
  const id = (entry as Record<string, unknown>).id;
  return typeof id === "string" ? id : "";
}

const FILTERS: Array<{ id: keyof TraceFilter; label: string }> = [
  { id: "duration", label: "trace.filter.duration" },
  { id: "turns", label: "trace.filter.turns" },
  { id: "tools", label: "trace.filter.tools" },
  { id: "subcalls", label: "trace.filter.subcalls" },
];

/**
 * The trace view: the session's append log as a dense, scannable list.
 *
 * This is deliberately not the chat. The chat renders `MessageView` bubbles —
 * markdown, images, expandable tool cards — which is right for reading a reply
 * and wrong for finding the one call that failed among eleven hundred. Here
 * every event is one line, the whole run is loaded up front so the rail is a
 * true map, and the right-hand panel is the only place detail expands.
 */
export function TraceView({ sessionId, running, tools, isMobile, active }: Props) {
  const { t } = useI18n();
  const [entries, setEntries] = useState<Record<string, unknown>[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<TraceFilter>(() => loadTraceViewPreferences().filter);
  const [query, setQuery] = useState("");
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [detailTab, setDetailTab] = useState<TraceDetailTab>("overview");
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);
  const [detailWidth, setDetailWidth] = useState(() => loadTraceViewPreferences().detailWidth);
  /** Bumped to ask the layout effect below to put the scroll offset back. */
  const [restoreToken, setRestoreToken] = useState(0);

  const scrollRef = useRef<HTMLDivElement>(null);
  const scrollFrameRef = useRef<number | null>(null);
  const lastEntryIdRef = useRef<string | null>(null);
  /** One sync at a time: a poll firing mid-load would otherwise append the same
   *  tail twice, because the cursor only advances once the loop finishes. */
  const syncingRef = useRef(false);
  const dragRef = useRef<{ pointerId: number; startX: number; startWidth: number } | null>(null);
  /** The remembered position, waiting for the rows it refers to. */
  const pendingAnchorRef = useRef<TraceViewAnchor | null>(null);
  /** `active` for callbacks that outlive the render they were created in. */
  const activeRef = useRef(active);
  activeRef.current = active;
  const sessionIdRef = useRef(sessionId);
  sessionIdRef.current = sessionId;
  const eventsRef = useRef<readonly TraceEvent[]>([]);

  /**
   * Pull entries after the cursor. The first call for a session starts at the
   * beginning and walks forward; later calls (a poll, a refresh) only ask for
   * what was appended since, so a running session costs one small request.
   */
  const sync = useCallback(
    async (options: { reset?: boolean; silent?: boolean; signal?: AbortSignal } = {}): Promise<void> => {
      if (!sessionId) return;
      if (syncingRef.current) return;
      syncingRef.current = true;
      if (options.reset) {
        lastEntryIdRef.current = null;
        setEntries([]);
        setTotal(0);
      }
      if (!options.silent) {
        setLoading(true);
        setError(null);
      }
      const startedFresh = lastEntryIdRef.current === null;
      const accumulated: Record<string, unknown>[] = [];
      try {
        let after = lastEntryIdRef.current;
        for (;;) {
          const params = new URLSearchParams({
            limit: String(PAGE_LIMIT),
            previewChars: String(PREVIEW_CHARS),
          });
          if (after) params.set("after", after);
          const response = await fetch(
            `/api/sessions/${encodeURIComponent(sessionId)}/entries?${params.toString()}`,
            { cache: "no-store", signal: options.signal },
          );
          if (!response.ok) throw new Error(`HTTP ${response.status}`);
          const data = (await response.json()) as EntriesResponse;
          const page = Array.isArray(data.entries)
            ? data.entries.filter(
                (entry): entry is Record<string, unknown> =>
                  Boolean(entry) && typeof entry === "object" && !Array.isArray(entry),
              )
            : [];
          if (typeof data.total === "number") setTotal(data.total);
          accumulated.push(...page);
          if (accumulated.length > 0) {
            const snapshot = accumulated.slice();
            // A poll appends to what the list already holds; a first load
            // replaces it, so a session switch cannot interleave two logs.
            setEntries((previous) => (startedFresh ? snapshot : [...previous, ...snapshot]));
          }
          const next = typeof data.nextAfter === "string" ? data.nextAfter : null;
          if (data.hasMore !== true || !next || next === after || accumulated.length >= MAX_ENTRIES) break;
          after = next;
        }
        const tail = accumulated.at(-1);
        if (tail) lastEntryIdRef.current = entryIdOf(tail) || lastEntryIdRef.current;
        else if (startedFresh) lastEntryIdRef.current = null;
      } catch (cause) {
        if (cause instanceof Error && cause.name === "AbortError") return;
        setError(cause instanceof Error ? cause.message : String(cause));
      } finally {
        syncingRef.current = false;
        if (!options.silent) setLoading(false);
      }
    },
    [sessionId],
  );

  useEffect(() => {
    if (!sessionId) {
      lastEntryIdRef.current = null;
      setEntries([]);
      setTotal(0);
      setSelectedKey(null);
      setError(null);
      pendingAnchorRef.current = null;
      return;
    }
    // Put this session back the way it was left: its row, its selection, its
    // detail tab. The scroll offset waits for the rows it points at.
    const remembered = getTraceSessionState(sessionId);
    setSelectedKey(remembered.selectedKey);
    setDetailTab(remembered.detailTab);
    pendingAnchorRef.current = remembered.anchor;
    setRestoreToken((token) => token + 1);
    setError(null);
    const controller = new AbortController();
    void sync({ reset: true, signal: controller.signal });
    return () => controller.abort();
  }, [sessionId, sync]);

  // A running session appends entries while the view is open; polling only the
  // tail keeps that live without re-reading megabytes every few seconds.
  useEffect(() => {
    if (!sessionId || !running || !active) return;
    const timer = setInterval(() => void sync({ silent: true }), POLL_MS);
    return () => clearInterval(timer);
  }, [active, running, sessionId, sync]);

  useEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    const measure = (): void => setViewportHeight(element.clientHeight);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    saveTraceViewPreferences({ filter, detailWidth });
  }, [detailWidth, filter]);

  const allEvents = useMemo(() => buildTraceEvents(entries, { running }), [entries, running]);
  const counts = useMemo(() => summarizeTraceEvents(allEvents), [allEvents]);

  const searched = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return allEvents;
    return allEvents.filter((event) =>
      `${event.badge} ${event.title} ${event.text} ${event.args} ${event.result}`
        .toLowerCase()
        .includes(needle),
    );
  }, [allEvents, query]);

  const events = useMemo(() => filterTraceEvents(searched, filter), [filter, searched]);
  eventsRef.current = events;

  const offsets = useMemo(() => {
    const table = new Array<number>(events.length + 1);
    for (let index = 0; index <= events.length; index += 1) table[index] = index * TRACE_ROW_HEIGHT;
    return table;
  }, [events.length]);

  const visible = useMemo(
    () => getVisibleRowIndices(offsets, scrollTop, viewportHeight, OVERSCAN_PX),
    [offsets, scrollTop, viewportHeight],
  );

  const selectedIndex = useMemo(() => {
    if (!selectedKey) return null;
    const found = events.findIndex((event) => event.key === selectedKey);
    return found === -1 ? null : found;
  }, [events, selectedKey]);

  const selected = selectedIndex === null ? null : events[selectedIndex];

  /**
   * Remember the row at the top of the viewport, as a key plus an offset. Rows
   * are a fixed height, so the key resolves back to an exact pixel even after
   * the log grew, a filter changed, or the list was re-rendered.
   */
  const recordAnchor = useCallback((element: HTMLDivElement): void => {
    const session = sessionIdRef.current;
    const list = eventsRef.current;
    if (!session || list.length === 0) return;
    const top = element.scrollTop;
    const index = Math.min(list.length - 1, Math.max(0, Math.floor(top / TRACE_ROW_HEIGHT)));
    const event = list[index];
    if (!event) return;
    patchTraceSessionState(session, {
      anchor: { key: event.key, offset: index * TRACE_ROW_HEIGHT - top },
    });
  }, []);

  const handleScroll = useCallback(() => {
    const element = scrollRef.current;
    if (!element) return;
    if (scrollFrameRef.current !== null) return;
    scrollFrameRef.current = requestAnimationFrame(() => {
      scrollFrameRef.current = null;
      // A hidden scroll box reports offset 0; recording that would throw away
      // the position the user is coming back to.
      if (!activeRef.current) return;
      setScrollTop(element.scrollTop);
      recordAnchor(element);
    });
  }, [recordAnchor]);

  /**
   * Put the remembered offset back. It waits until there are rows to point at,
   * which is why it runs on `events` and not on the session change itself.
   */
  useLayoutEffect(() => {
    const anchor = pendingAnchorRef.current;
    if (!anchor) return;
    const element = scrollRef.current;
    if (!element || events.length === 0) return;
    const index = events.findIndex((event) => event.key === anchor.key);
    pendingAnchorRef.current = null;
    if (index === -1) return;
    const target = Math.max(0, index * TRACE_ROW_HEIGHT - anchor.offset);
    element.scrollTop = target;
    setScrollTop(target);
  }, [events, restoreToken, viewportHeight]);

  /**
   * Changing tabs only hides this view (`display: none`), which makes the
   * browser forget the scroll offset. Restore it when it comes back.
   */
  const wasActiveRef = useRef(false);
  useEffect(() => {
    const wasActive = wasActiveRef.current;
    wasActiveRef.current = active;
    if (!active || wasActive) return;
    pendingAnchorRef.current = sessionId ? getTraceSessionState(sessionId).anchor : null;
    setRestoreToken((token) => token + 1);
  }, [active, sessionId]);

  useEffect(
    () => () => {
      if (scrollFrameRef.current !== null) cancelAnimationFrame(scrollFrameRef.current);
    },
    [],
  );

  const reveal = useCallback(
    (index: number): void => {
      const element = scrollRef.current;
      if (!element) return;
      const next = revealScrollTop(offsets, index, element.scrollTop, element.clientHeight);
      if (next !== null) element.scrollTop = next;
      setScrollTop(element.scrollTop);
    },
    [offsets],
  );

  const selectAt = useCallback(
    (index: number, options: { reveal?: boolean } = {}): void => {
      const event = events[index];
      if (!event) return;
      setSelectedKey(event.key);
      if (sessionIdRef.current) {
        patchTraceSessionState(sessionIdRef.current, { selectedKey: event.key });
      }
      if (options.reveal !== false) reveal(index);
    },
    [events, reveal],
  );

  const chooseTab = useCallback((tab: TraceDetailTab): void => {
    setDetailTab(tab);
    if (sessionIdRef.current) patchTraceSessionState(sessionIdRef.current, { detailTab: tab });
  }, []);

  const closeDetail = useCallback((): void => {
    setSelectedKey(null);
    if (sessionIdRef.current) patchTraceSessionState(sessionIdRef.current, { selectedKey: null });
  }, []);

  const handleKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>): void => {
      if (event.key === "Escape" && selectedKey) {
        closeDetail();
        event.preventDefault();
        return;
      }
      const current = selectedIndex ?? -1;
      const page = Math.max(1, Math.floor(viewportHeight / TRACE_ROW_HEIGHT) - 1);
      let next: number | null = null;
      if (event.key === "ArrowDown") next = current + 1;
      else if (event.key === "ArrowUp") next = current <= 0 ? 0 : current - 1;
      else if (event.key === "PageDown") next = current + page;
      else if (event.key === "PageUp") next = current - page;
      else if (event.key === "Home") next = 0;
      else if (event.key === "End") next = events.length - 1;
      if (next === null) return;
      event.preventDefault();
      selectAt(Math.min(events.length - 1, Math.max(0, next)));
    },
    [closeDetail, events.length, selectedIndex, selectedKey, selectAt, viewportHeight],
  );

  const toggle = useCallback((id: keyof TraceFilter): void => {
    setFilter((previous) => ({ ...previous, [id]: !previous[id] }));
  }, []);

  const totalHeight = events.length * TRACE_ROW_HEIGHT;

  // --- detail resize ---------------------------------------------------
  const beginResize = useCallback(
    (event: React.PointerEvent<HTMLDivElement>): void => {
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      dragRef.current = { pointerId: event.pointerId, startX: event.clientX, startWidth: detailWidth };
    },
    [detailWidth],
  );
  const moveResize = useCallback((event: React.PointerEvent<HTMLDivElement>): void => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    // The panel grows leftwards: dragging left widens it.
    const next = drag.startWidth - (event.clientX - drag.startX);
    setDetailWidth(clampTraceDetailWidth(next));
  }, []);
  const endResize = useCallback((event: React.PointerEvent<HTMLDivElement>): void => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    dragRef.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
  }, []);

  return (
    <div className="trace-view" data-trace-view="">
      <div className="trace-main">
        <div className="trace-toolbar">
          {FILTERS.map((item) => (
            <button
              key={item.id}
              type="button"
              className="trace-chip"
              aria-pressed={filter[item.id]}
              onClick={() => toggle(item.id)}
              title={t(item.label)}
            >
              {t(item.label)}
              {item.id === "tools" && <span className="trace-chip-count">{counts.calls}</span>}
              {item.id === "turns" && <span className="trace-chip-count">{counts.turns}</span>}
              {item.id === "subcalls" && <span className="trace-chip-count">{counts.subcalls}</span>}
              {item.id === "duration" && counts.totalDurationMs > 0 && (
                <span className="trace-chip-count">{formatTraceDuration(counts.totalDurationMs)}</span>
              )}
            </button>
          ))}
          <span className="trace-toolbar-spacer" />
          {counts.errors > 0 && (
            <span className="trace-summary" style={{ color: "#dc2626" }}>
              {t("trace.summary.errors", { count: counts.errors })}
            </span>
          )}
          <label className="trace-search">
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" aria-hidden="true">
              <circle cx="11" cy="11" r="7" />
              <path d="m20 20-3.5-3.5" />
            </svg>
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("trace.search")}
              aria-label={t("trace.search")}
              spellCheck={false}
            />
          </label>
        </div>

        {loading && entries.length === 0 && (
          <div className="trace-progress" aria-hidden="true">
            <div style={{ width: "35%" }} />
          </div>
        )}
        {!loading && total > entries.length && (
          <div className="trace-progress" aria-hidden="true">
            <div style={{ width: `${Math.round((entries.length / Math.max(1, total)) * 100)}%` }} />
          </div>
        )}

        <div className="trace-body">
          <div
            ref={scrollRef}
            className="trace-scroll"
            onScroll={handleScroll}
            onKeyDown={handleKeyDown}
            tabIndex={0}
            aria-label={t("trace.list.label")}
          >
            {error ? (
              <div className="trace-empty">
                <span>{t("trace.error.load", { message: error })}</span>
                <button type="button" className="trace-load" onClick={() => void sync({ reset: true })}>
                  {t("trace.retry")}
                </button>
              </div>
            ) : !sessionId ? (
              <div className="trace-empty">{t("trace.empty.noSession")}</div>
            ) : events.length === 0 ? (
              <div className="trace-empty">
                {loading ? t("trace.loading") : query ? t("trace.empty.search") : t("trace.empty.events")}
              </div>
            ) : (
              <div className="trace-inner" style={{ height: totalHeight }}>
                {visible.map((index) => {
                  const event = events[index];
                  if (!event) return null;
                  return (
                    <TraceRow
                      key={event.key}
                      event={event}
                      index={index}
                      top={offsets[index]}
                      height={TRACE_ROW_HEIGHT}
                      selected={index === selectedIndex}
                      turnStart={index === 0 || events[index - 1].turn !== event.turn}
                      showTurn={filter.turns}
                      showDuration={filter.duration}
                      onSelect={selectAt}
                    />
                  );
                })}
              </div>
            )}
          </div>

          {selected && (
            <>
              <div
                role="separator"
                aria-orientation="vertical"
                aria-label={t("trace.detail.resize")}
                className="panel-resize-handle"
                style={{ flexShrink: 0, cursor: "col-resize", width: 5 }}
                onPointerDown={beginResize}
                onPointerMove={moveResize}
                onPointerUp={endResize}
                onPointerCancel={endResize}
              />
              <TraceDetail
                sessionId={sessionId}
                event={selected}
                tools={tools}
                width={detailWidth}
                tab={detailTab}
                onTabChange={chooseTab}
                onClose={closeDetail}
              />
            </>
          )}
        </div>
      </div>

      <TraceRail
        events={events}
        selectedIndex={selectedIndex}
        scrollTop={scrollTop}
        viewportHeight={viewportHeight}
        totalHeight={totalHeight}
        onSelect={selectAt}
        showTimeScale={filter.duration}
        compact={isMobile}
      />
    </div>
  );
}