"use client";

import { memo, useEffect, useMemo, useState } from "react";
import type { TraceEvent, TraceStatus } from "@/lib/trace-events";
import { formatTraceClock, formatTraceDuration, traceText } from "@/lib/trace-events";
import type { ToolEntry } from "@/lib/tool-presets";
import { TRACE_DETAIL_TABS, type TraceDetailTab } from "@/lib/trace-view-state";
import { useI18n } from "@/hooks/useI18n";

const ENTRY_CACHE_LIMIT = 48;
/** Uncapped entries, most recently used last. The list page only carries
 *  previews, so the panel re-reads what it needs; one entry is small enough. */
const entryCache = new Map<string, Promise<Record<string, unknown> | null>>();

function loadEntry(sessionId: string, entryId: string): Promise<Record<string, unknown> | null> {
  const key = `${sessionId}/${entryId}`;
  const cached = entryCache.get(key);
  if (cached) {
    entryCache.delete(key);
    entryCache.set(key, cached);
    return cached;
  }
  const request = fetch(
    `/api/sessions/${encodeURIComponent(sessionId)}/entries/${encodeURIComponent(entryId)}`,
    { cache: "no-store" },
  )
    .then(async (response) => {
      if (!response.ok) return null;
      const data = (await response.json()) as { entry?: unknown };
      const entry = data.entry;
      return entry && typeof entry === "object" && !Array.isArray(entry)
        ? (entry as Record<string, unknown>)
        : null;
    })
    .catch(() => null);
  entryCache.set(key, request);
  void request.then((entry) => {
    // A failed read must not be remembered as permanent.
    if (entry === null && entryCache.get(key) === request) entryCache.delete(key);
  });
  if (entryCache.size > ENTRY_CACHE_LIMIT) {
    const oldest = entryCache.keys().next().value;
    if (oldest !== undefined && oldest !== key) entryCache.delete(oldest);
  }
  return request;
}

/** The entry the panel is looking at, re-read without the list page's cap. */
function useFullEntry(sessionId: string | null, entryId: string | null): {
  entry: Record<string, unknown> | null;
  loading: boolean;
} {
  const [state, setState] = useState<{ entry: Record<string, unknown> | null; loading: boolean }>({
    entry: null,
    loading: false,
  });
  useEffect(() => {
    if (!sessionId || !entryId) {
      setState({ entry: null, loading: false });
      return;
    }
    let cancelled = false;
    setState({ entry: null, loading: true });
    void loadEntry(sessionId, entryId).then((entry) => {
      if (!cancelled) setState({ entry, loading: false });
    });
    return () => {
      cancelled = true;
    };
  }, [entryId, sessionId]);
  return state;
}

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function contentBlocks(entry: Record<string, unknown> | null): Record<string, unknown>[] {
  const message = entry ? record(entry.message) : null;
  const content = message?.content;
  if (!Array.isArray(content)) return [];
  return content.filter((block): block is Record<string, unknown> => Boolean(record(block)));
}

function formatJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function formatUsage(value: unknown): string | null {
  const usage = record(value);
  if (!usage) return null;
  const parts: string[] = [];
  const input = usage.input;
  const output = usage.output;
  const cacheRead = usage.cacheRead;
  const total = usage.totalTokens;
  if (typeof input === "number") parts.push(`in ${input}`);
  if (typeof output === "number") parts.push(`out ${output}`);
  if (typeof cacheRead === "number" && cacheRead > 0) parts.push(`cache ${cacheRead}`);
  if (typeof total === "number") parts.push(`total ${total}`);
  const cost = record(usage.cost);
  const totalCost = typeof usage.cost === "number" ? usage.cost : cost && typeof cost.total === "number" ? cost.total : null;
  if (totalCost !== null) parts.push(`$${totalCost.toFixed(4)}`);
  return parts.length > 0 ? parts.join(" · ") : null;
}

function StatusValue({ status }: { status: TraceStatus }) {
  const { t } = useI18n();
  const className =
    status === "error"
      ? "trace-badge-error"
      : status === "ok"
        ? "trace-badge-ok"
        : status === "aborted"
          ? "trace-badge-error"
          : status === "running"
            ? "trace-badge-running"
            : undefined;
  return <span className={className}>{t(`trace.status.${status}`)}</span>;
}

function Fields({ rows }: { rows: Array<[string, React.ReactNode]> }) {
  return (
    <dl className="trace-fields">
      {rows.map(([label, value]) => (
        <div key={label} style={{ display: "contents" }}>
          <dt>{label}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

interface Props {
  sessionId: string | null;
  event: TraceEvent;
  /** Active tool definitions, for the schema tab. Null while unknown. */
  tools: ToolEntry[] | null;
  width: number;
  /** Controlled by the parent: the open tab is part of the view's memory. */
  tab: TraceDetailTab;
  onTabChange: (tab: TraceDetailTab) => void;
  onClose: () => void;
}

export const TraceDetail = memo(function TraceDetail({
  sessionId,
  event,
  tools,
  width,
  tab,
  onTabChange,
  onClose,
}: Props) {
  const { t } = useI18n();
  // A new row keeps the tab the user chose, which is what makes stepping
  // through a run with the arrow keys useful.
  const call = useFullEntry(sessionId, event.entryId || null);
  const result = useFullEntry(sessionId, event.resultEntryId);

  const blocks = useMemo(() => contentBlocks(call.entry ?? event.entry), [call.entry, event.entry]);
  const block = event.blockIndex === null ? null : (blocks[event.blockIndex] ?? null);

  const args = useMemo(() => {
    const fromBlock = block ? (record(block.input) ?? record(block.arguments)) : null;
    if (fromBlock) return fromBlock;
    return event.toolCall?.input ?? null;
  }, [block, event.toolCall]);

  const resultText = useMemo(() => {
    // Reasoning is a block, not message text: read it from the block so the
    // uncapped entry, once loaded, replaces the preview.
    if (event.kind === "thinking") return text(block?.thinking) || event.text;
    if (event.lane !== "tool") return traceText(record(event.entry.message)?.content);
    const message = result.entry ? record(result.entry.message) : null;
    if (message) return traceText(message.content);
    return event.toolResult?.text ?? "";
  }, [block, event.entry, event.kind, event.lane, event.text, event.toolResult, result.entry]);

  const message = record((call.entry ?? event.entry).message);
  const schema = useMemo(
    () => (event.lane === "tool" && tools ? (tools.find((tool) => tool.name === event.title) ?? null) : null),
    [event.lane, event.title, tools],
  );

  const entryId = text(event.entry.id) || event.entryId;
  const parentId = text(event.entry.parentId);

  return (
    <aside className="trace-detail" style={{ width }} aria-label={t("trace.detail.label")}>
      <div className="trace-detail-head">
        <span className="trace-detail-title" title={`${event.badge} · ${event.title}`}>
          {event.title}
        </span>
        <button type="button" className="trace-detail-close" onClick={onClose} aria-label={t("trace.detail.close")}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" aria-hidden="true">
            <path d="M6 6l12 12M18 6L6 18" />
          </svg>
        </button>
      </div>

      <div className="trace-tabs" role="tablist" aria-label={t("trace.detail.label")}>
        {TRACE_DETAIL_TABS.map((id) => {
          // The schema tab only means something for a tool call that we have a
          // definition for; hiding it beats an empty panel.
          if (id === "schema" && !schema) return null;
          return (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={tab === id}
              className="trace-tab"
              onClick={() => onTabChange(id)}
            >
              {t(`trace.tab.${id}`)}
            </button>
          );
        })}
      </div>

      <div className="trace-detail-body">
        {tab === "overview" && (
          <Fields
            rows={[
              [t("trace.field.kind"), event.kind || "—"],
              [t("trace.field.role"), text(message?.role) || event.lane],
              [t("trace.field.status"), <StatusValue key="status" status={event.status} />],
              [t("trace.field.turn"), event.turn > 0 ? String(event.turn) : "—"],
              [t("trace.field.position"), `#${event.entryIndex + 1}`],
              [t("trace.field.entryId"), entryId || "—"],
              [t("trace.field.parentId"), parentId || "—"],
              ...(text(message?.model) || text(message?.provider)
                ? ([[t("trace.field.model"), [text(message?.provider), text(message?.model)].filter(Boolean).join("/")]] as Array<[string, React.ReactNode]>)
                : []),
              ...(text(message?.stopReason)
                ? ([[t("trace.field.stopReason"), text(message?.stopReason)]] as Array<[string, React.ReactNode]>)
                : []),
              ...(formatUsage(message?.usage)
                ? ([[t("trace.field.usage"), formatUsage(message?.usage) as string]] as Array<[string, React.ReactNode]>)
                : []),
              ...(event.toolCall?.id
                ? ([[t("trace.field.toolCallId"), event.toolCall.id]] as Array<[string, React.ReactNode]>)
                : []),
              ...(event.nested
                ? ([[t("trace.field.subagent"), t("trace.value.subagentCall")]] as Array<[string, React.ReactNode]>)
                : []),
            ]}
          />
        )}

        {tab === "args" &&
          (args ? (
            <pre className="trace-block">{formatJson(args)}</pre>
          ) : (
            <p className="trace-note">{t("trace.empty.args")}</p>
          ))}

        {tab === "result" && (
          <>
            {event.lane === "tool" && result.loading && <p className="trace-note">{t("trace.loading")}</p>}
            {resultText ? (
              <pre className="trace-block">{resultText}</pre>
            ) : (
              <p className="trace-note">
                {event.status === "running"
                  ? t("trace.empty.running")
                  : event.status === "missing"
                    ? t("trace.empty.missing")
                    : t("trace.empty.result")}
              </p>
            )}
            {event.toolResult && event.toolResult.imageCount > 0 && (
              <p className="trace-note">
                {t("trace.note.images", { count: event.toolResult.imageCount })}
              </p>
            )}
          </>
        )}

        {tab === "schema" && schema && (
          <>
            {schema.description && <p className="trace-note">{schema.description}</p>}
            {schema.parameters ? (
              <pre className="trace-block">{formatJson(schema.parameters)}</pre>
            ) : (
              <p className="trace-note">{t("trace.empty.schema")}</p>
            )}
            {schema.promptGuidelines && schema.promptGuidelines.length > 0 && (
              <pre className="trace-block">{schema.promptGuidelines.join("\n")}</pre>
            )}
          </>
        )}

        {tab === "timing" && (
          <Fields
            rows={[
              [t("trace.field.startedAt"), formatTraceClock(event.timestamp) || "—"],
              [t("trace.field.duration"), formatTraceDuration(event.durationMs) || "—"],
              [
                t("trace.field.messageDuration"),
                formatTraceDuration(typeof message?.durationMs === "number" ? message.durationMs : null) || "—",
              ],
              [
                t("trace.field.toolDuration"),
                formatTraceDuration(event.toolResult?.durationMs ?? null) || "—",
              ],
              [
                t("trace.field.finishedAt"),
                event.timestamp !== null && event.durationMs !== null
                  ? formatTraceClock(event.timestamp + event.durationMs)
                  : "—",
              ],
              [t("trace.field.clock"), event.timestamp !== null ? new Date(event.timestamp).toISOString() : "—"],
            ]}
          />
        )}
      </div>
    </aside>
  );
});