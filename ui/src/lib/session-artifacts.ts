import { getRelativeFilePath } from "./file-paths";
import { extractWrittenFileEvents } from "./turn-written-files";
import type { AgentMessage, ToolResultMessage } from "./types";

/**
 * A file this session actually wrote. Derived from successful `write`, `edit`
 * and `apply_patch` calls (see turn-written-files.ts), never from prose or a
 * merely-read file; deletes are not artifacts.
 */
export interface SessionArtifact {
  /** Absolute path, as the tool call resolved it. */
  filePath: string;
  /** The path shown in the pane: relative to the session's cwd when inside it. */
  relativePath: string;
  /** When the last successful write's result arrived, ms. Absent in old sessions. */
  lastWriteAt?: number;
  /** How many successful writes touched this path in the scanned transcript. */
  writes: number;
}

const ARTIFACT_PAGE_TAIL = 1000;
/** Bound one scan of a very long session: 20 × 1000 entries is far past normal. */
const ARTIFACT_SCAN_PAGES = 20;

/**
 * Fold write events into one row per file: first-seen order for equal times,
 * then newest write first. `writes` counts writes which the caller may have
 * received from more than one source, so merging lists takes the max, not a sum.
 */
export function summarizeArtifactEvents(
  events: readonly { filePath: string; timestamp?: number }[],
  cwd?: string,
): SessionArtifact[] {
  const byPath = new Map<string, SessionArtifact>();
  for (const event of events) {
    const existing = byPath.get(event.filePath);
    if (!existing) {
      byPath.set(event.filePath, {
        filePath: event.filePath,
        relativePath: getRelativeFilePath(event.filePath, cwd),
        lastWriteAt: event.timestamp,
        writes: 1,
      });
      continue;
    }
    existing.writes += 1;
    if (event.timestamp !== undefined) {
      existing.lastWriteAt = Math.max(existing.lastWriteAt ?? 0, event.timestamp);
    }
  }
  return [...byPath.values()].sort((a, b) => (
    (b.lastWriteAt ?? 0) - (a.lastWriteAt ?? 0)
    || a.relativePath.localeCompare(b.relativePath)
  ));
}

/** The artifacts of the transcript in hand (loaded messages or a full scan). */
export function collectSessionArtifacts(
  messages: readonly AgentMessage[],
  toolResults: ReadonlyMap<string, ToolResultMessage> | undefined,
  cwd?: string,
): SessionArtifact[] {
  const events: { filePath: string; timestamp?: number }[] = [];
  for (const message of messages) {
    if (message.role !== "assistant") continue;
    for (const event of extractWrittenFileEvents(message.content, toolResults as Map<string, ToolResultMessage> | undefined, cwd)) {
      events.push({ filePath: event.filePath, timestamp: event.timestamp ?? message.timestamp });
    }
  }
  return summarizeArtifactEvents(events, cwd);
}

/**
 * Union two views of the same session (a full history scan and the live
 * transcript, which usually overlap). Per file the newest write wins; the
 * write count is the larger one, so an overlapping file is not counted twice.
 */
export function mergeSessionArtifacts(
  ...lists: readonly (readonly SessionArtifact[])[]
): SessionArtifact[] {
  const byPath = new Map<string, SessionArtifact>();
  for (const list of lists) {
    for (const artifact of list) {
      const existing = byPath.get(artifact.filePath);
      if (!existing) {
        byPath.set(artifact.filePath, { ...artifact });
        continue;
      }
      existing.writes = Math.max(existing.writes, artifact.writes);
      if (artifact.lastWriteAt !== undefined) {
        existing.lastWriteAt = Math.max(existing.lastWriteAt ?? 0, artifact.lastWriteAt);
      }
    }
  }
  return [...byPath.values()].sort((a, b) => (
    (b.lastWriteAt ?? 0) - (a.lastWriteAt ?? 0)
    || a.relativePath.localeCompare(b.relativePath)
  ));
}

interface ArtifactContextPage {
  messages: AgentMessage[];
  oldestEntryId: string | null;
  hasMore: boolean;
}

/**
 * Scan the whole session transcript for artifacts, page by page, through the
 * context endpoint the chat itself pages with. Client-side on purpose: the
 * write/edit/apply_patch rules stay in one place (turn-written-files.ts) and
 * this path never loads the transcript into the chat's state.
 */
export async function loadSessionArtifacts({
  sessionId,
  leafId,
  cwd,
  signal,
  maxPages = ARTIFACT_SCAN_PAGES,
}: {
  sessionId: string;
  leafId?: string | null;
  cwd?: string;
  signal?: AbortSignal;
  maxPages?: number;
}): Promise<SessionArtifact[]> {
  const pages: ArtifactContextPage[] = [];
  let before: string | null = null;
  for (let page = 0; page < maxPages; page += 1) {
    const params = new URLSearchParams({
      deferThinking: "1",
      deferMedia: "1",
      tail: String(ARTIFACT_PAGE_TAIL),
    });
    if (leafId) params.set("leafId", leafId);
    if (before) params.set("before", before);
    const response = await fetch(`/api/sessions/${encodeURIComponent(sessionId)}/context?${params}`, { signal });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const data = await response.json() as { context?: ArtifactContextPage };
    const context = data.context;
    if (!context) break;
    pages.push(context);
    const oldest = context.oldestEntryId;
    if (!context.hasMore || !oldest || oldest === before) break;
    before = oldest;
  }
  const messages = pages.reverse().flatMap((page) => page.messages);
  const toolResults = new Map<string, ToolResultMessage>();
  for (const message of messages) {
    if (message.role !== "toolResult") continue;
    const result = message as ToolResultMessage;
    toolResults.set(result.toolCallId, result);
  }
  return collectSessionArtifacts(messages, toolResults, cwd);
}
