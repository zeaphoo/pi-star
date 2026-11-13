"use client";

import { memo } from "react";
import type { TraceEvent } from "@/lib/trace-events";
import { TRACE_BADGE_KEYS, formatTraceDuration } from "@/lib/trace-events";
import { useI18n } from "@/hooks/useI18n";

interface Props {
  event: TraceEvent;
  index: number;
  top: number;
  height: number;
  selected: boolean;
  /** First row of a new turn: draws the separator the "turns" toggle controls. */
  turnStart: boolean;
  showTurn: boolean;
  showDuration: boolean;
  onSelect: (index: number) => void;
}

/** One line of the log, always exactly `height` tall: the virtualizer measured
 *  the list with that assumption and cannot re-measure a row it did not mount. */
export const TraceRow = memo(function TraceRow({
  event,
  index,
  top,
  height,
  selected,
  turnStart,
  showTurn,
  showDuration,
  onSelect,
}: Props) {
  const { t } = useI18n();
  const isTool = event.lane === "tool";
  const badge = TRACE_BADGE_KEYS.has(event.badge) ? t(`trace.badge.${event.badge}`) : event.badge;
  return (
    <button
      type="button"
      className="trace-row"
      id={`trace-row-${index}`}
      style={{ top, height }}
      data-lane={event.lane}
      data-kind={event.kind}
      data-status={event.status}
      data-selected={selected ? "true" : undefined}
      data-turn-start={turnStart ? "true" : undefined}
      data-trace-index={index}
      tabIndex={-1}
      onClick={() => onSelect(index)}
      aria-current={selected ? "true" : undefined}
      title={`${event.badge} · ${event.title}`}
    >
      <span className="trace-row-badge">{badge}</span>
      {/* Only the row that opens a turn carries its number: repeating it on
          every row of a busy turn is noise, not orientation. */}
      {showTurn && <span className="trace-row-turn">{turnStart && event.turn > 0 ? event.turn : ""}</span>}
      <span className="trace-row-body">
        {isTool ? (
          <>
            <span className="trace-row-title">{event.title}</span>
            {event.args && <span className="trace-row-args">{event.args}</span>}
            {event.result && (
              <>
                <span className="trace-row-arrow">→</span>
                <span className="trace-row-result">{event.result}</span>
              </>
            )}
          </>
        ) : (
          <span className="trace-row-text">{event.text || t("trace.value.empty")}</span>
        )}
        {event.nested && (
          <span className="trace-row-nested" title={event.title}>
            ⤷
          </span>
        )}
      </span>
      {showDuration && (
        <span className="trace-row-duration">{formatTraceDuration(event.durationMs)}</span>
      )}
    </button>
  );
});