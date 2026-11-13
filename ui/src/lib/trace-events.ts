/**
 * The trace view's model: a session entry log flattened into inspectable rows.
 *
 * `GET /api/sessions/[id]/entries` returns entries exactly as pi appended them,
 * so every interpretation lives here: which entry becomes which row, how a tool
 * call finds its result, which turn a row belongs to, what the badges say. Pure
 * functions only — the list, the rail and the detail panel all read this one
 * model, and it is testable without a DOM.
 *
 * The shape differs from the chat transcript on purpose. The chat shows
 * `MessageView` bubbles: markdown, images, collapsible tool cards. The trace
 * shows one dense line per event so a thousand of them can be scanned, which is
 * also why it reads the raw log instead of `/context`: the projection there
 * drops model changes, thinking-level changes, labels and tool-selection
 * entries, and those are exactly the events a trace is for.
 */

/** Rail column a row belongs to. */
export type TraceLane = "input" | "model" | "tool" | "meta";

export type TraceStatus = "ok" | "error" | "running" | "aborted" | "missing";

export interface TraceToolCall {
  id: string;
  name: string;
  input: Record<string, unknown>;
}

export interface TraceToolResult {
  toolCallId: string;
  toolName: string;
  text: string;
  isError: boolean;
  durationMs: number | null;
  /** Images are never inlined into a list page; this is how many were dropped. */
  imageCount: number;
  details: unknown;
}

export interface TraceEvent {
  /** Stable across refetches: an entry id, plus a block index when one message
   *  yields several rows. */
  key: string;
  lane: TraceLane;
  entryId: string;
  /** Index of the content block inside the message; `null` for whole entries. */
  blockIndex: number | null;
  /** Position of the owning entry in the log, 0-based. */
  entryIndex: number;
  /** 1-based turn. A turn starts at every user message. */
  turn: number;
  /** Entry type (`message`, `compaction`, …) or the message role. */
  kind: string;
  /** Row badge: 用户 / 助手 / 思考 / 工具 / 模型 / 压缩… */
  badge: string;
  /** Name of the tool a `tool` row is about; equals `badge` otherwise. */
  title: string;
  /** The row's main text. */
  text: string;
  /** Tool rows: the call's arguments as compact JSON. */
  args: string;
  /** Tool rows: the result's first line. */
  result: string;
  timestamp: number | null;
  durationMs: number | null;
  status: TraceStatus;
  isError: boolean;
  /** A `pi-star-subagent` call: the "subcalls" filter hides these. */
  nested: boolean;
  entry: Record<string, unknown>;
  toolCall: TraceToolCall | null;
  toolResult: TraceToolResult | null;
  /** Entry the result came from: the detail panel re-reads it uncapped. */
  resultEntryId: string | null;
}

/** Which parts of the list are visible. Mirrors the four toggles above it. */
export interface TraceFilter {
  /** The duration column. */
  duration: boolean;
  /** Turn separators and the turn column. */
  turns: boolean;
  /** Tool rows. */
  tools: boolean;
  /** Tool rows that spawned a subagent. */
  subcalls: boolean;
}

export const TRACE_FILTER_ALL: TraceFilter = {
  duration: true,
  turns: true,
  tools: true,
  subcalls: true,
};

/** Badges the UI has a `trace.badge.*` message for; anything else (a role this
 *  build does not know) is shown verbatim. */
export const TRACE_BADGE_KEYS: ReadonlySet<string> = new Set([
  "assistant",
  "branch_summary",
  "compaction",
  "context_edit",
  "custom",
  "custom_message",
  "label",
  "model",
  "session_info",
  "system",
  "thinking",
  "thinking_level",
  "tool",
  "tools",
  "user",
]);

const SUBAGENT_DETAILS_KIND = "pi-star-subagent";
/** `(仅工具调用)` for an assistant message that only issues calls. */
const ASSISTANT_CALLS_ONLY = "(仅工具调用)";
const ASSISTANT_NO_TEXT = "(空回复)";

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function asString(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function asNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/** Collapse every run of whitespace to one space and trim: rows are one line. */
export function traceOneLine(text: string): string {
  return text.replace(/\s+/gu, " ").trim();
}

/**
 * The first non-empty line of a multi-line block. Tool output leads with the
 * interesting part (a path, an exit status, the first traceback line) far more
 * often than it leads with nothing.
 */
export function traceFirstLine(text: string): string {
  for (const line of text.split(/\r?\n/u)) {
    const trimmed = line.trim();
    if (trimmed) return trimmed;
  }
  return "";
}

/**
 * Visible text of a message `content`, which is either a string or a block
 * array. Only `text` blocks count: a thinking block is its own row and an image
 * carries no text.
 */
export function traceText(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  const parts: string[] = [];
  for (const raw of content) {
    const block = asRecord(raw);
    if (!block || block.type !== "text") continue;
    const text = asString(block.text);
    if (text) parts.push(text);
  }
  return parts.join("\n");
}

/**
 * Entry timestamps are RFC 3339 strings (`"2026-10-31T06:55:26.345Z"`), message
 * timestamps are epoch milliseconds, and pi writes both. Prefer the message's
 * precise clock, fall back to the entry's.
 */
export function traceTimestamp(
  entry: Record<string, unknown>,
  message: Record<string, unknown> | null,
): number | null {
  const fromMessage = asNumber(message?.timestamp);
  if (fromMessage !== null) return fromMessage;
  const raw = asString(entry.timestamp);
  if (!raw) return null;
  const parsed = Date.parse(raw);
  return Number.isFinite(parsed) ? parsed : null;
}

/**
 * Normalize a `toolCall` block. pi has written `id`/`name`/`input` and
 * `toolCallId`/`toolName`/`arguments` across versions; accept both, like the
 * server's `normalize_message` does.
 */
export function traceToolCall(block: Record<string, unknown>): TraceToolCall | null {
  if (block.type !== "toolCall") return null;
  const id = asString(block.toolCallId) || asString(block.id);
  const name = asString(block.toolName) || asString(block.name);
  if (!name) return null;
  return {
    id,
    name,
    input: asRecord(block.input) ?? asRecord(block.arguments) ?? {},
  };
}

/** Compact JSON for a call's arguments; one line, quotes kept like dsh's rows. */
export function traceArgs(call: TraceToolCall): string {
  try {
    const encoded = JSON.stringify(call.input);
    return typeof encoded === "string" ? encoded : "";
  } catch {
    return "";
  }
}

function traceToolResult(entry: Record<string, unknown>, message: Record<string, unknown>): TraceToolResult {
  const content = message.content;
  let imageCount = 0;
  if (Array.isArray(content)) {
    for (const raw of content) {
      if (asRecord(raw)?.type === "image") imageCount += 1;
    }
  }
  return {
    toolCallId: asString(message.toolCallId),
    toolName: asString(message.toolName),
    text: traceText(content),
    isError: message.isError === true,
    durationMs: asNumber(message.durationMs),
    imageCount,
    details: message.details,
  };
}

function isSubagentResult(result: TraceToolResult): boolean {
  const details = asRecord(result.details);
  return Boolean(details) && details?.kind === SUBAGENT_DETAILS_KIND;
}

/** `1.4s`, `237ms`, `2m 03s` — the trace labels durations, it does not sum them. */
export function formatTraceDuration(ms: number | null | undefined): string {
  if (typeof ms !== "number" || !Number.isFinite(ms) || ms < 0) return "";
  if (ms < 1000) return `${Math.round(ms)}ms`;
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 1 : 0)}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = Math.round(seconds % 60);
  return `${minutes}m ${String(rest).padStart(2, "0")}s`;
}

/** `09:22:39.909` — wall clock for the detail panel. */
export function formatTraceClock(timestamp: number | null): string {
  if (timestamp === null) return "";
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return "";
  const pad = (value: number, width = 2): string => String(value).padStart(width, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds(), 3)}`;
}

export interface TraceBuildOptions {
  /** The session still has a live run: a call without a result is running, not missing. */
  running?: boolean;
}

interface TurnCounter {
  turn: number;
}

/** One row for a whole non-message entry (compaction, model change, label…). */
function metaEvent(
  entry: Record<string, unknown>,
  entryIndex: number,
  turn: number,
  running: boolean,
): TraceEvent | null {
  const type = asString(entry.type);
  const base = {
    lane: "meta" as const,
    entryId: asString(entry.id),
    blockIndex: null,
    entryIndex,
    turn,
    kind: type,
    timestamp: traceTimestamp(entry, null),
    durationMs: null,
    status: (running ? "ok" : "ok") as TraceStatus,
    isError: false,
    nested: false,
    args: "",
    result: "",
    text: "",
    entry,
    toolCall: null,
    toolResult: null,
    resultEntryId: null,
  };
  const key = `${base.entryId || `entry-${entryIndex}`}#meta`;
  switch (type) {
    case "compaction":
      return {
        ...base,
        key,
        badge: "compaction",
        title: "compaction",
        text: traceFirstLine(asString(entry.summary)),
        status: "ok",
      };
    case "branch_summary":
      return {
        ...base,
        key,
        badge: "branch_summary",
        title: "branch_summary",
        text: traceFirstLine(asString(entry.summary)),
      };
    case "model_change": {
      const provider = asString(entry.provider);
      const modelId = asString(entry.modelId);
      const label = [provider, modelId].filter(Boolean).join("/");
      if (!label) return null;
      return { ...base, key, badge: "model", title: label, text: label };
    }
    case "thinking_level_change": {
      const level = asString(entry.thinkingLevel);
      if (!level) return null;
      return { ...base, key, badge: "thinking_level", title: level, text: level };
    }
    case "session_info": {
      const name = asString(entry.name);
      if (!name) return null;
      return { ...base, key, badge: "session_info", title: name, text: traceOneLine(name) };
    }
    case "context_edit": {
      const target = asString(entry.targetId);
      const replacement = entry.replacement;
      const text = replacement === null || replacement === undefined ? "移除内容" : "替换内容";
      return { ...base, key, badge: "context_edit", title: target, text: `${text} → ${target}` };
    }
    case "label": {
      const label = asString(entry.label);
      return { ...base, key, badge: "label", title: label, text: label || asString(entry.targetId) };
    }
    case "custom": {
      const customType = asString(entry.customType);
      return { ...base, key, badge: "custom", title: customType || "custom", text: customType };
    }
    case "custom_message": {
      // `display: false` notes are real events the model saw; a trace shows them
      // anyway, unlike the chat which hides them.
      const customType = asString(entry.customType);
      return {
        ...base,
        key,
        badge: "custom_message",
        title: customType || "custom_message",
        text: traceFirstLine(traceText(entry.content)) || customType,
      };
    }
    case "active_tools_change": {
      const tools = Array.isArray(entry.tools) ? entry.tools : [];
      const names = tools.filter((tool): tool is string => typeof tool === "string");
      return { ...base, key, badge: "tools", title: "tools", text: names.join(", ") };
    }
    // `session` is the file header (read_session strips it, but a hand-written
    // fixture may not) and anything unknown carries nothing to show. Dropping
    // them beats rendering an empty row that says "事件".
    default:
      return null;
  }
}

/**
 * Flatten a page of raw session entries into rows.
 *
 * One assistant message yields up to three kinds of row — its text, one per
 * thinking block, one per tool call — because a single message routinely
 * carries all three and the trace is a log of what happened, not of what was
 * sent. Tool results are not rows of their own: they are folded into the call
 * that produced them, matched by `toolCallId`, which is how a call that never
 * returned stays visibly `missing`.
 */
export function buildTraceEvents(
  entries: readonly Record<string, unknown>[],
  options: TraceBuildOptions = {},
): TraceEvent[] {
  const running = options.running === true;

  // Tool results are appended after the assistant message that called them.
  // A result is shown through its call, so first collect both sides: the ids
  // that were called (to tell a folded result from an orphan whose call fell on
  // an earlier page) and the results themselves.
  const calledIds = new Set<string>();
  const resultsByCallId = new Map<string, { entryId: string; result: TraceToolResult }>();

  for (const entry of entries) {
    if (asString(entry.type) !== "message") continue;
    const message = asRecord(entry.message);
    if (!message) continue;
    const role = asString(message.role);
    if (role === "assistant" && Array.isArray(message.content)) {
      for (const raw of message.content) {
        const block = asRecord(raw);
        const toolCall = block ? traceToolCall(block) : null;
        if (toolCall?.id) calledIds.add(toolCall.id);
      }
      continue;
    }
    if (role !== "toolResult") continue;
    const result = traceToolResult(entry, message);
    if (result.toolCallId) {
      resultsByCallId.set(result.toolCallId, { entryId: asString(entry.id), result });
    }
  }

  const events: TraceEvent[] = [];
  const counter: TurnCounter = { turn: 0 };

  for (let entryIndex = 0; entryIndex < entries.length; entryIndex += 1) {
    const entry = entries[entryIndex];
    const type = asString(entry.type);
    if (type !== "message") {
      const event = metaEvent(entry, entryIndex, counter.turn, running);
      if (event) events.push(event);
      continue;
    }
    const message = asRecord(entry.message);
    if (!message) continue;
    const role = asString(message.role);
    const entryId = asString(entry.id);
    const timestamp = traceTimestamp(entry, message);
    const base = {
      entryId,
      entryIndex,
      timestamp,
      entry,
      nested: false,
      args: "",
      result: "",
    };

    if (role === "user") {
      counter.turn += 1;
      events.push({
        ...base,
        key: `${entryId || `entry-${entryIndex}`}#user`,
        lane: "input",
        blockIndex: null,
        turn: counter.turn,
        kind: "user",
        badge: "user",
        title: "user",
        text: traceFirstLine(traceText(message.content)),
        durationMs: null,
        status: "ok",
        isError: false,
        toolCall: null,
        toolResult: null,
        resultEntryId: null,
      });
      continue;
    }

    if (role === "toolResult") {
      // Rendered through its call. A result whose call fell on an earlier page
      // still deserves a row, otherwise the log visibly loses output.
      const toolCallId = asString(message.toolCallId);
      if (toolCallId && calledIds.has(toolCallId)) continue;
      const result = traceToolResult(entry, message);
      events.push({
        ...base,
        key: `${entryId || `entry-${entryIndex}`}#orphan`,
        lane: "tool",
        blockIndex: null,
        turn: counter.turn,
        kind: "toolResult",
        badge: "tool",
        title: result.toolName || "tool",
        text: "",
        args: "",
        result: traceFirstLine(result.text),
        durationMs: result.durationMs,
        status: result.isError ? "error" : "ok",
        isError: result.isError,
        toolCall: null,
        toolResult: result,
        resultEntryId: entryId || null,
      });
      continue;
    }

    if (role !== "assistant") {
      // `custom` and `bashExecution` messages: one row, no block split.
      events.push({
        ...base,
        key: `${entryId || `entry-${entryIndex}`}#custom`,
        lane: "model",
        blockIndex: null,
        turn: counter.turn,
        kind: role,
        badge: role,
        title: asString(message.customType) || role,
        text: traceFirstLine(traceText(message.content)),
        durationMs: asNumber(message.durationMs),
        status: "ok",
        isError: false,
        toolCall: null,
        toolResult: null,
        resultEntryId: null,
      });
      continue;
    }

    const content = Array.isArray(message.content) ? message.content : [];
    const stopReason = asString(message.stopReason);
    const errored = Boolean(asString(message.errorMessage));
    const messageStatus: TraceStatus =
      stopReason === "aborted" ? "aborted" : errored ? "error" : "ok";
    const messageDuration = asNumber(message.durationMs);
    const textParts: string[] = [];
    const thinkingRows: TraceEvent[] = [];
    const toolRows: TraceEvent[] = [];

    for (let blockIndex = 0; blockIndex < content.length; blockIndex += 1) {
      const block = asRecord(content[blockIndex]);
      if (!block) continue;
      if (block.type === "text") {
        const text = asString(block.text);
        if (text) textParts.push(text);
        continue;
      }
      if (block.type === "thinking") {
        const thinking = traceFirstLine(asString(block.thinking));
        if (!thinking) continue;
        thinkingRows.push({
          ...base,
          key: `${entryId || `entry-${entryIndex}`}#thinking-${blockIndex}`,
          lane: "model",
          blockIndex,
          turn: counter.turn,
          kind: "thinking",
          badge: "thinking",
          title: "thinking",
          text: thinking,
          durationMs: null,
          status: messageStatus,
          isError: errored,
          toolCall: null,
          toolResult: null,
          resultEntryId: null,
        });
        continue;
      }
      const call = traceToolCall(block);
      if (!call) continue;
      const found = call.id ? resultsByCallId.get(call.id) : undefined;
      const result = found?.result;
      const status: TraceStatus = result
        ? result.isError
          ? "error"
          : "ok"
        : running
          ? "running"
          : "missing";
      toolRows.push({
        ...base,
        key: `${entryId || `entry-${entryIndex}`}#call-${blockIndex}`,
        lane: "tool",
        blockIndex,
        turn: counter.turn,
        kind: "toolCall",
        badge: "tool",
        title: call.name,
        text: "",
        args: traceArgs(call),
        result: result ? traceFirstLine(result.text) : "",
        durationMs: result?.durationMs ?? null,
        status,
        isError: result?.isError === true,
        nested: result ? isSubagentResult(result) : false,
        toolCall: call,
        toolResult: result ?? null,
        resultEntryId: found?.entryId ?? null,
      });
    }

    const text = textParts.join("\n");
    const assistantText = text
      ? traceFirstLine(text)
      : toolRows.length > 0
        ? ASSISTANT_CALLS_ONLY
        : ASSISTANT_NO_TEXT;
    events.push({
      ...base,
      key: `${entryId || `entry-${entryIndex}`}#assistant`,
      lane: "model",
      blockIndex: null,
      turn: counter.turn,
      kind: "assistant",
      badge: "assistant",
      title: "assistant",
      text: traceOneLine(assistantText),
      durationMs: messageDuration,
      status: messageStatus,
      isError: errored,
      toolCall: null,
      toolResult: null,
      resultEntryId: null,
    });
    events.push(...thinkingRows);
    // Calls after the reply that produced them: the reply leads, the calls it
    // made follow, which is the order dsh shows and the order they happened.
    events.push(...toolRows);
  }

  return events;
}

/** Rows a filter keeps. Turn counting stays on the full list, so hiding tools
 *  never renumbers the turns. */
export function filterTraceEvents(events: readonly TraceEvent[], filter: TraceFilter): TraceEvent[] {
  return events.filter((event) => {
    if (!filter.tools && event.lane === "tool") return false;
    if (!filter.subcalls && event.nested) return false;
    return true;
  });
}

export interface TraceCounts {
  entries: number;
  turns: number;
  calls: number;
  errors: number;
  subcalls: number;
  totalDurationMs: number;
}

export function summarizeTraceEvents(events: readonly TraceEvent[]): TraceCounts {
  const turns = new Set<number>();
  let calls = 0;
  let errors = 0;
  let subcalls = 0;
  let totalDurationMs = 0;
  for (const event of events) {
    if (event.turn > 0) turns.add(event.turn);
    if (event.lane === "tool") calls += 1;
    if (event.isError) errors += 1;
    if (event.nested) subcalls += 1;
    if (event.lane === "tool" && event.durationMs !== null) totalDurationMs += event.durationMs;
  }
  return { entries: events.length, turns: turns.size, calls, errors, subcalls, totalDurationMs };
}