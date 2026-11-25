"use client";

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { TraceDetail } from "./TraceDetail";
import { TraceRow } from "./TraceRow";
import { TRACE_ROW_HEIGHT } from "./TraceRail";
import { buildTraceEvents, type TraceEvent } from "@/lib/trace-events";
import type { TraceDetailTab } from "@/lib/trace-view-state";
import type { ToolEntry } from "@/lib/tool-presets";
import { useI18n } from "@/hooks/useI18n";

interface Props {
  sessionId: string;
  /** The run is still live, so an unanswered call is running, not lost. */
  running: boolean;
  /** Active tool definitions; the schema tab reads them. */
  tools: ToolEntry[] | null;
  /** Changes when the session's log grew (`modified` plus message count). */
  revision: string;
}

interface EntriesResponse {
  entries?: unknown;
  hasMore?: unknown;
  nextAfter?: unknown;
}

const PAGE_LIMIT = 400;
/** Characters the list page keeps per text block. */
const PREVIEW_CHARS = 160;
/** A subagent's log is small; this only guards a pathological one. */
const MAX_ENTRIES = 4_000;

function entryIdOf(entry: unknown): string {
  if (!entry || typeof entry !== "object") return "";
  const id = (entry as Record<string, unknown>).id;
  return typeof id === "string" ? id : "";
}

/**
 * A subagent's execution trace: the same dense log the main Trace tab shows,
 * but scoped to the child session and rendered inside a right-panel tab. It
 * deliberately reads `/entries` instead of subscribing to the agent stream: a
 * viewer must never resume or disturb a subagent run.
 */
export function SubagentTrace({ sessionId, running, tools, revision }: Props) {
  const { t } = useI18n();
  const [entries, setEntries] = useState<Record<string, unknown>[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [detailTab, setDetailTab] = useState<TraceDetailTab>("overview");
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);
  const scrollRef = useRef<HTMLDivElement>(null);
  const lastEntryIdRef = useRef<string | null>(null);
  const syncingRef = useRef(false);

  const sync = useCallback(
    async (reset: boolean, signal?: AbortSignal) => {
      if (syncingRef.current && !reset) return;
      syncingRef.current = true;
      if (reset) {
        lastEntryIdRef.current = null;
        setEntries([]);
        setLoading(true);
      }
      const collected: Record<string, unknown>[] = [];
      try {
        let after = reset ? null : lastEntryIdRef.current;
        for (;;) {
          const params = new URLSearchParams({
            limit: String(PAGE_LIMIT),
            previewChars: String(PREVIEW_CHARS),
          });
          if (after) params.set("after", after);
          const response = await fetch(
            `/api/sessions/${encodeURIComponent(sessionId)}/entries?${params.toString()}`,
            { cache: "no-store", signal },
          );
          if (!response.ok) throw new Error(`HTTP ${response.status}`);
          const data = (await response.json()) as EntriesResponse;
          const page = Array.isArray(data.entries)
            ? data.entries.filter(
                (entry): entry is Record<string, unknown> =>
                  Boolean(entry) && typeof entry === "object" && !Array.isArray(entry),
              )
            : [];
          collected.push(...page);
          const next = typeof data.nextAfter === "string" ? data.nextAfter : null;
          if (data.hasMore !== true || !next || next === after || collected.length >= MAX_ENTRIES) break;
          after = next;
        }
        setEntries((previous) => (reset ? collected : [...previous, ...collected]));
        const tail = collected.at(-1);
        if (tail) lastEntryIdRef.current = entryIdOf(tail) || lastEntryIdRef.current;
        setError(null);
      } catch (cause) {
        if (cause instanceof Error && cause.name === "AbortError") return;
        setError(cause instanceof Error ? cause.message : String(cause));
      } finally {
        syncingRef.current = false;
        setLoading(false);
      }
    },
    [sessionId],
  );

  useEffect(() => {
    const controller = new AbortController();
    setSelectedKey(null);
    void sync(true, controller.signal);
    return () => controller.abort();
  }, [sessionId, sync]);

  // The log grows while the run is live: poll only the tail.
  useEffect(() => {
    if (!running) return;
    const timer = setInterval(() => {
      if (document.visibilityState === "visible") void sync(false);
    }, 2000);
    return () => clearInterval(timer);
  }, [running, sync]);

  // A finished run appends its result entry after the last poll: catch up once.
  useEffect(() => {
    if (running) return;
    void sync(false);
  }, [revision, running, sync]);

  useEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    const measure = () => setViewportHeight(element.clientHeight);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const events = useMemo(() => buildTraceEvents(entries, { running }), [entries, running]);
  const selectedIndex = selectedKey ? events.findIndex((event) => event.key === selectedKey) : -1;
  const selected = selectedIndex >= 0 ? events[selectedIndex] : null;

  // Subagent logs are short enough to mount whole; the rows are absolutely
  // positioned, so the inner height is what the scroll container measures.
  const totalHeight = events.length * TRACE_ROW_HEIGHT;
  const firstVisible = Math.max(0, Math.floor(scrollTop / TRACE_ROW_HEIGHT) - 5);
  const visibleCount = Math.ceil(viewportHeight / TRACE_ROW_HEIGHT) + 10;
  const visible = useMemo(
    () => events.slice(firstVisible, firstVisible + visibleCount),
    [events, firstVisible, visibleCount],
  );

  const selectAt = useCallback(
    (index: number) => {
      const event = events[index];
      if (!event) return;
      setSelectedKey(event.key);
      setScrollTop(scrollRef.current?.scrollTop ?? 0);
    },
    [events],
  );

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent) => {
      if (events.length === 0) return;
      const current = selectedIndex >= 0 ? selectedIndex : 0;
      const page = Math.max(1, Math.floor(viewportHeight / TRACE_ROW_HEIGHT) - 1);
      const next = event.key === "ArrowDown" ? current + 1
        : event.key === "ArrowUp" ? current - 1
        : event.key === "PageDown" ? current + page
        : event.key === "PageUp" ? current - page
        : event.key === "Home" ? 0
        : event.key === "End" ? events.length - 1
        : null;
      if (next === null) return;
      event.preventDefault();
      const clamped = Math.min(events.length - 1, Math.max(0, next));
      setSelectedKey(events[clamped].key);
      const element = scrollRef.current;
      if (element) {
        const top = clamped * TRACE_ROW_HEIGHT;
        if (top < element.scrollTop) element.scrollTop = top;
        else if (top + TRACE_ROW_HEIGHT > element.scrollTop + element.clientHeight) {
          element.scrollTop = top + TRACE_ROW_HEIGHT - element.clientHeight;
        }
      }
    },
    [events, selectedIndex, viewportHeight],
  );

  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    const measure = () => setViewportHeight(element.clientHeight);
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  if (error && events.length === 0) {
    return (
      <div className="trace-empty">
        <span>{t("trace.error.load", { message: error })}</span>
        <button type="button" className="trace-load" onClick={() => void sync(true)}>
          {t("trace.retry")}
        </button>
      </div>
    );
  }

  return (
    <div className="trace-body" style={{ display: "flex", flex: 1, minHeight: 0 }}>
      <div
        ref={scrollRef}
        className="trace-scroll"
        onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
        onKeyDown={handleKeyDown}
        tabIndex={0}
        aria-label={t("trace.list.label")}
      >
        {events.length === 0 ? (
          <div className="trace-empty">{loading ? t("trace.loading") : t("trace.empty.events")}</div>
        ) : (
          <div className="trace-inner" style={{ height: totalHeight }}>
            {visible.map((event, offset) => {
              const index = firstVisible + offset;
              return (
                <TraceRow
                  key={event.key}
                  event={event}
                  index={index}
                  top={index * TRACE_ROW_HEIGHT}
                  height={TRACE_ROW_HEIGHT}
                  selected={index === selectedIndex}
                  turnStart={index === 0 || events[index - 1].turn !== event.turn}
                  showTurn={true}
                  showDuration={true}
                  onSelect={selectAt}
                />
              );
            })}
          </div>
        )}
      </div>
      {selected && (
        <TraceDetail
          sessionId={sessionId}
          event={selected as TraceEvent}
          tools={tools}
          width={360}
          tab={detailTab}
          onTabChange={setDetailTab}
          onClose={() => setSelectedKey(null)}
        />
      )}
    </div>
  );
}
