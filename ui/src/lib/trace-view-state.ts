/**
 * What the trace view remembers, and for how long.
 *
 * Two kinds of state, kept apart on purpose:
 *
 * - **Where I am in this session** — the row that was at the top of the list,
 *   which row was selected, which detail tab was open. This is per session and
 *   lives in memory only, exactly like the chat's `sessionScrollPositionsRef`:
 *   switching tabs or sessions brings it back, a page reload starts over. It is
 *   not worth a storage budget, and a session the user deleted must not leave a
 *   ghost.
 * - **How I like to look at a trace** — the four filter toggles and the detail
 *   panel width. Those are preferences, so they go to localStorage, the way the
 *   dock keeps its layout.
 *
 * The anchor is a row key plus an offset, not a raw `scrollTop`. Rows are a
 * fixed height, so the key resolves to an exact pixel again even after entries
 * were appended, a filter changed, or a poll re-rendered the list. A raw offset
 * would drift the moment any of those happened.
 */

import { TRACE_FILTER_ALL, type TraceFilter } from "./trace-events";

export const TRACE_DETAIL_TABS = ["overview", "args", "result", "schema", "timing"] as const;

export type TraceDetailTab = (typeof TRACE_DETAIL_TABS)[number];

export interface TraceViewAnchor {
  /** `TraceEvent.key` of the row at the top of the viewport. */
  key: string;
  /** That row's top minus the scroll offset: negative once scrolled past it. */
  offset: number;
}

export interface TraceSessionState {
  anchor: TraceViewAnchor | null;
  selectedKey: string | null;
  detailTab: TraceDetailTab;
}

export interface TraceViewPreferences {
  filter: TraceFilter;
  detailWidth: number;
}

export const TRACE_VIEW_STORAGE_KEY = "pi-star:trace-view:v1";
export const TRACE_DETAIL_DEFAULT_WIDTH = 420;
export const TRACE_DETAIL_MIN_WIDTH = 300;
export const TRACE_DETAIL_MAX_WIDTH = 760;

/** Remembered sessions. Older ones are dropped; the trace is a reading aid, not
 *  a database, and a user rarely holds more than a handful open. */
const MAX_REMEMBERED_SESSIONS = 24;

const sessions = new Map<string, TraceSessionState>();

export function emptyTraceSessionState(): TraceSessionState {
  return { anchor: null, selectedKey: null, detailTab: "overview" };
}

export function getTraceSessionState(sessionId: string): TraceSessionState {
  const stored = sessions.get(sessionId);
  if (!stored) return emptyTraceSessionState();
  // Touch, so the entry that is dropped first is the one left longest.
  sessions.delete(sessionId);
  sessions.set(sessionId, stored);
  return stored;
}

export function patchTraceSessionState(
  sessionId: string,
  patch: Partial<TraceSessionState>,
): TraceSessionState {
  const next = { ...getTraceSessionState(sessionId), ...patch };
  sessions.delete(sessionId);
  sessions.set(sessionId, next);
  while (sessions.size > MAX_REMEMBERED_SESSIONS) {
    const oldest = sessions.keys().next().value;
    if (oldest === undefined) break;
    sessions.delete(oldest);
  }
  return next;
}

export function resetTraceViewStateForTests(): void {
  sessions.clear();
}

export function isTraceDetailTab(value: unknown): value is TraceDetailTab {
  return typeof value === "string" && (TRACE_DETAIL_TABS as readonly string[]).includes(value);
}

export function clampTraceDetailWidth(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return TRACE_DETAIL_DEFAULT_WIDTH;
  return Math.min(TRACE_DETAIL_MAX_WIDTH, Math.max(TRACE_DETAIL_MIN_WIDTH, Math.round(value)));
}

/** A stored filter with missing or unreadable keys falls back to on: an absent
 *  preference means "show everything", never "hide the tools". */
export function normalizeTraceFilter(value: unknown): TraceFilter {
  if (!value || typeof value !== "object" || Array.isArray(value)) return { ...TRACE_FILTER_ALL };
  const source = value as Record<string, unknown>;
  return {
    duration: source.duration !== false,
    turns: source.turns !== false,
    tools: source.tools !== false,
    subcalls: source.subcalls !== false,
  };
}

function storage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function loadTraceViewPreferences(): TraceViewPreferences {
  const fallback: TraceViewPreferences = {
    filter: { ...TRACE_FILTER_ALL },
    detailWidth: TRACE_DETAIL_DEFAULT_WIDTH,
  };
  const raw = storage()?.getItem(TRACE_VIEW_STORAGE_KEY);
  if (!raw) return fallback;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return fallback;
    const record = parsed as Record<string, unknown>;
    return {
      filter: normalizeTraceFilter(record.filter),
      detailWidth: clampTraceDetailWidth(record.detailWidth),
    };
  } catch {
    return fallback;
  }
}

export function saveTraceViewPreferences(preferences: TraceViewPreferences): void {
  try {
    storage()?.setItem(TRACE_VIEW_STORAGE_KEY, JSON.stringify(preferences));
  } catch {
    // Storage is best-effort: a full quota must not break the view.
  }
}