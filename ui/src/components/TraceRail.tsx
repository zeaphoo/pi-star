"use client";

import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { TraceEvent, TraceLane } from "@/lib/trace-events";
import { formatTraceDuration } from "@/lib/trace-events";
import { useI18n } from "@/hooks/useI18n";

/** Row pitch the list lays out with; the rail converts row index to y with it. */
export const TRACE_ROW_HEIGHT = 28;
export const TRACE_RAIL_WIDTH = 88;

const LANE_WIDTH = 20;
/** Vertical pitch of a rail mark: 1px drawn, 1px gap, at any density. */
const SLOT_PX = 2;
/** Space reserved at the top of the rail for the three lane labels, so ticks
 *  never run under them. Every ratio below is measured against the rest. */
const TOP_INSET = 13;
const COMPACT_TOP_INSET = 0;

/** One drawn mark: a single event, or a slot's worth of them. */
interface RailMark {
  key: string;
  /** Row a click on this mark jumps to: the first row it stands for. */
  index: number;
  top: number;
  count: number;
  error: boolean;
  nested: boolean;
  /** Height in px: derived from the lane's spacing so marks never touch. */
  height: number;
  selected: boolean;
}

interface Props {
  /** The rows the list is showing, in list order. */
  events: readonly TraceEvent[];
  selectedIndex: number | null;
  scrollTop: number;
  viewportHeight: number;
  /** Height of the whole list; positions the viewport band. */
  totalHeight: number;
  onSelect: (index: number) => void;
  /** Draw the wall-clock distribution strip on the rail's left edge. */
  showTimeScale: boolean;
  /** Phone layout: a 26px barcode of the same three lanes, no labels. */
  compact?: boolean;
}

/**
 * The trace rail: one vertical lane per event family, time running downwards.
 *
 * dsh draws this as a horizontal three-lane strip across the top. Turned on its
 * side it can live where a position indicator belongs — pinned to the right
 * edge of the view, never covered by the detail panel, which opens to its left.
 *
 * Tick y is derived from the **row index**, not the timestamp, so a click lands
 * on the row it points at. Wall-clock spacing is still available, drawn as a
 * separate 4px strip on the left of the rail: long model waits show up as gaps
 * there without ever desynchronising the list.
 */
export const TraceRail = memo(function TraceRail({
  events,
  selectedIndex,
  scrollTop,
  viewportHeight,
  totalHeight,
  onSelect,
  showTimeScale,
  compact = false,
}: Props) {
  const { t } = useI18n();
  const inset = compact ? COMPACT_TOP_INSET : TOP_INSET;
  const railRef = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(0);
  const [hover, setHover] = useState<{ index: number; y: number } | null>(null);

  useEffect(() => {
    const element = railRef.current;
    if (!element) return;
    const measure = (): void => setHeight(element.clientHeight);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const laneWidth = compact ? 6 : LANE_WIDTH;
  const laneGap = compact ? 3 : 4;
  const lanes = useMemo(
    () => [
      { lane: "input" as TraceLane, x: compact ? 2 : 6, label: t("trace.lane.input") },
      { lane: "model" as TraceLane, x: (compact ? 2 : 6) + laneWidth + laneGap, label: t("trace.lane.model") },
      {
        lane: "tool" as TraceLane,
        x: (compact ? 2 : 6) + (laneWidth + laneGap) * 2,
        label: t("trace.lane.tool"),
      },
    ],
    [compact, laneGap, laneWidth, t],
  );

  const count = events.length;

  // One bucket per lane, index preserved: mapping the whole list three times per
  // render costs three thousand callback invocations on a long session.
  const byLane = useMemo(() => {
    const buckets: Record<string, Array<{ index: number; event: TraceEvent }>> = {
      input: [],
      model: [],
      tool: [],
      meta: [],
    };
    events.forEach((event, index) => {
      buckets[event.lane]?.push({ index, event });
    });
    return buckets;
  }, [events]);

  /** Nearest row for a pointer position, or null when there is nothing to point at. */
  const indexAt = useCallback(
    (clientY: number): number | null => {
      const element = railRef.current;
      if (!element || count === 0) return null;
      const rect = element.getBoundingClientRect();
      const usable = rect.height - inset;
      if (usable <= 0) return null;
      const ratio = (clientY - rect.top - inset) / usable;
      return Math.min(count - 1, Math.max(0, Math.round(ratio * count - 0.5)));
    },
    [count, inset],
  );

  /** y in px for a row. Rows are evenly spaced, which is what keeps a click on
   *  the rail landing on the row it points at. */
  const yOf = useCallback(
    (index: number): number => (count > 0 ? inset + ((index + 0.5) / count) * (height - inset) : 0),
    [count, height, inset],
  );

  // Wall-clock positions, binned per pixel: a 1000-event session would otherwise
  // put a 1000-node strip in a 4px column, and the strip is only a density hint.
  const timeTicks = useMemo(() => {
    if (!showTimeScale || compact || height <= 0 || count === 0) return [];
    let min = Number.POSITIVE_INFINITY;
    let max = Number.NEGATIVE_INFINITY;
    for (const event of events) {
      if (event.timestamp === null) continue;
      if (event.timestamp < min) min = event.timestamp;
      if (event.timestamp > max) max = event.timestamp;
    }
    if (!Number.isFinite(min) || max <= min) return [];
    const seen = new Set<number>();
    const ticks: number[] = [];
    for (const event of events) {
      if (event.timestamp === null) continue;
      const y = Math.round(inset + ((event.timestamp - min) / (max - min)) * (height - inset - 2));
      if (seen.has(y)) continue;
      seen.add(y);
      ticks.push(y);
    }
    return ticks;
  }, [compact, count, events, height, inset, showTimeScale]);

  /** Top edge of a row, as opposed to `yOf`'s centre: bins sit on their first row. */
  const topOf = useCallback(
    (index: number): number => (count > 0 ? inset + (index / count) * (height - inset) : 0),
    [count, height, inset],
  );

  /**
   * One mark per lane per 2px slot.
   *
   * A busy tool lane has more events than the rail has pixels, so drawing them
   * all is not "dense", it is a solid bar — the earlier version read as a strip
   * of paint. Binning to slots keeps a 1px gap between marks at any density and
   * costs nothing that was visible: at that pitch two neighbouring calls are the
   * same pixel anyway. An error inside a slot still paints the slot red, so the
   * one thing a reader scans the rail for is never swallowed.
   *
   * Sparse lanes keep one mark per event, so an input turn stays a landmark.
   */
  const marksByLane = useMemo(() => {
    const usable = Math.max(1, height - inset);
    const slots = Math.max(1, Math.floor(usable / SLOT_PX));
    const result: Record<string, RailMark[]> = { input: [], model: [], tool: [] };
    for (const lane of ["input", "model", "tool"] as const) {
      const bucket = byLane[lane] ?? [];
      if (bucket.length === 0) continue;
      const step = Math.max(1, Math.ceil(bucket.length / slots));
      // A mark may only be as tall as its slot leaves room for. Drawing them all
      // at 3px is what merged a full lane into a solid bar.
      const laneSlots = Math.ceil(bucket.length / step);
      const spacing = usable / laneSlots;
      const height = Math.max(1, Math.min(3, Math.floor(spacing) - 1));
      const marks: RailMark[] = [];
      for (let start = 0; start < bucket.length; start += step) {
        const end = Math.min(bucket.length, start + step);
        let error = false;
        let nested = false;
        for (let cursor = start; cursor < end; cursor += 1) {
          const event = bucket[cursor].event;
          if (event.isError) error = true;
          if (event.nested) nested = true;
        }
        const first = bucket[start];
        marks.push({
          key: `${first.event.key}~${end - start}`,
          index: first.index,
          top: Math.round(topOf(first.index)),
          count: end - start,
          error,
          nested,
          height,
          selected:
            selectedIndex !== null &&
            selectedIndex >= first.index &&
            selectedIndex <= bucket[end - 1].index,
        });
      }
      result[lane] = marks;
    }
    return result;
  }, [byLane, height, inset, selectedIndex, topOf]);

  const viewportStyle = useMemo(() => {
    if (totalHeight <= 0 || height <= 0) return null;
    const usable = height - inset;
    const top = inset + Math.max(0, Math.min(usable - 2, (scrollTop / totalHeight) * usable));
    const size = Math.max(6, (viewportHeight / totalHeight) * usable);
    return { top, height: Math.min(size, height - top) };
  }, [height, inset, scrollTop, totalHeight, viewportHeight]);

  const hovered = hover ? events[hover.index] : null;
  /** Marks the hovered row falls into, so a binned slot can say how many rows it holds. */
  const hoverCount = useMemo(() => {
    if (!hover) return 0;
    for (const lane of ["input", "model", "tool"] as const) {
      for (const mark of marksByLane[lane] ?? []) {
        if (hover.index >= mark.index && hover.index < mark.index + mark.count) return mark.count;
      }
    }
    return 1;
  }, [hover, marksByLane]);

  // A redundant aid: the list itself is focusable and arrow-navigable, so
  // screen readers are pointed at the rows rather than at this map.
  return (
    <div
      ref={railRef}
      className="trace-rail"
      style={{ width: compact ? 26 : TRACE_RAIL_WIDTH }}
      aria-hidden="true"
      onMouseMove={(event) => {
        const index = indexAt(event.clientY);
        if (index === null) return;
        const rect = event.currentTarget.getBoundingClientRect();
        setHover({ index, y: event.clientY - rect.top });
      }}
      onMouseLeave={() => setHover(null)}
      onClick={(event) => {
        const index = indexAt(event.clientY);
        if (index !== null) onSelect(index);
      }}
      data-trace-rail=""
      data-compact={compact ? "true" : undefined}
    >
      {showTimeScale && timeTicks.length > 0 && (
        <div className="trace-rail-time" aria-hidden="true">
          {timeTicks.map((y) => (
            <div key={y} className="trace-rail-time-tick" style={{ top: y }} />
          ))}
        </div>
      )}

      {lanes.map(({ lane, x, label }) => (
        <div
          key={lane}
          className="trace-rail-lane"
          style={{ left: x, width: laneWidth }}
          aria-hidden="true"
          data-trace-lane={lane}
          title={label}
        >
          {(marksByLane[lane] ?? []).map((mark) => (
            <div
              key={mark.key}
              className="trace-rail-tick"
              style={{ top: mark.top, height: mark.error ? Math.max(2, mark.height) : mark.height }}
              data-lane={lane}
              data-error={mark.error ? "true" : undefined}
              data-nested={mark.nested ? "true" : undefined}
              data-selected={mark.selected ? "true" : undefined}
              data-active={
                hover !== null && hover.index >= mark.index && hover.index < mark.index + mark.count
                  ? "true"
                  : undefined
              }
            />
          ))}
        </div>
      ))}

      {/* Configuration changes are not per-lane events: they cut across the run. */}
      {(byLane.meta ?? []).map(({ index, event }) => (
        <div
          key={`meta-${event.key}`}
          className="trace-rail-meta"
          style={{ top: yOf(index) }}
          aria-hidden="true"
        />
      ))}

      {viewportStyle && (
        <div className="trace-rail-viewport" style={{ top: viewportStyle.top, height: viewportStyle.height }} aria-hidden="true" />
      )}

      {!compact && (
        <div className="trace-rail-labels" aria-hidden="true">
          <span>{t("trace.lane.input")}</span>
          <span>{t("trace.lane.model")}</span>
          <span>{t("trace.lane.tool")}</span>
        </div>
      )}

      {hovered && (
        <div className="trace-rail-tooltip" style={{ top: Math.max(0, (hover?.y ?? 0) - 10) }}>
          <strong>{hovered.title}</strong>
          {hovered.lane === "tool" && hovered.durationMs !== null ? ` · ${formatTraceDuration(hovered.durationMs)}` : ""}
          {hovered.text ? ` · ${hovered.text}` : ""}
          {hoverCount > 1 ? ` · ${t("trace.rail.group", { count: hoverCount })}` : ""}
        </div>
      )}
    </div>
  );
});