import type { CustomMessage, SubagentSessionStatus } from "./types";

/**
 * A background subagent's report arrives as a custom message whose `content`
 * is written for the model: an English instruction prefix, a header naming the
 * run, then the report itself. `details` is the human-facing half. These helpers
 * turn the two back into something a card can show.
 */
export const SUBAGENT_NOTIFICATION_TYPES = [
  "pi-star:subagent-notification",
  "subagent-notification",
] as const;

const NOTIFICATION_PREFIX = "The following is a background subagent's report delivered by Pi Star";
const RESUMED_MARKER = "This report is from a resumed run of subagent";

const TERMINAL_STATUSES: readonly SubagentSessionStatus[] = [
  "starting",
  "running",
  "completed",
  "failed",
  "aborted",
  "interrupted",
];

export interface SubagentNotificationDetails {
  kind?: unknown;
  sessionId?: unknown;
  profile?: unknown;
  description?: unknown;
  status?: unknown;
  runInBackground?: unknown;
  createdAt?: unknown;
  completedAt?: unknown;
  error?: unknown;
}

export interface SubagentReport {
  sessionId: string;
  /** What the parent called this run: its description, else the profile, else a short id. */
  label: string;
  profile: string;
  description: string;
  status: SubagentSessionStatus;
  runInBackground: boolean;
  createdAt?: number;
  completedAt?: number;
  durationMs?: number;
  error?: string;
  /** The report markdown with the model-facing prefix and header removed. */
  body: string;
  /** A reused session id: this report supersedes an earlier one from the same subagent. */
  resumed: boolean;
}

function isNotificationType(value: string): boolean {
  return (SUBAGENT_NOTIFICATION_TYPES as readonly string[]).includes(value);
}

function isSubagentStatus(value: unknown): value is SubagentSessionStatus {
  return typeof value === "string" && (TERMINAL_STATUSES as readonly string[]).includes(value);
}

function toMillis(value: unknown): number | undefined {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string") {
    const parsed = Date.parse(value);
    return Number.isFinite(parsed) ? parsed : undefined;
  }
  return undefined;
}

/** The details both backends attach; null for any other custom message. */
export function readSubagentNotificationDetails(
  message: CustomMessage,
): SubagentNotificationDetails | null {
  if (!isNotificationType(message.customType)) return null;
  const details = message.details;
  if (!details || typeof details !== "object" || Array.isArray(details)) return null;
  const candidate = details as SubagentNotificationDetails;
  if (candidate.kind !== "pi-star-subagent" || typeof candidate.sessionId !== "string") return null;
  return candidate;
}

export function shortSubagentId(sessionId: string): string {
  return sessionId.slice(0, 8);
}

export function subagentReportLabel(details: SubagentNotificationDetails, sessionId: string): string {
  const description = typeof details.description === "string" ? details.description.trim() : "";
  if (description) return description;
  const profile = typeof details.profile === "string" ? details.profile.trim() : "";
  if (profile) return profile;
  return shortSubagentId(sessionId);
}

/** The label alone, without parsing the report body (the minimap's outline uses it). */
export function subagentNotificationLabel(message: CustomMessage): string | null {
  const details = readSubagentNotificationDetails(message);
  if (!details) return null;
  return subagentReportLabel(details, details.sessionId as string);
}

function messageText(content: CustomMessage["content"]): string {
  if (typeof content === "string") return content;
  return content
    .filter((block) => block.type === "text")
    .map((block) => (block.type === "text" ? block.text : ""))
    .join("\n");
}

/**
 * The report with the prompt-facing wrapper removed. The header names the run
 * and ends at the first newline; everything after the blank line is the report.
 * A failure may keep its error on the header line, which `details.error` carries
 * anyway. If the header cannot be found the whole content is shown rather than
 * hiding a report.
 */
function extractReportBody(text: string, sessionId: string): string {
  const marker = `Subagent ${sessionId}`;
  const start = text.indexOf(marker);
  if (start < 0) {
    const trimmed = text.trim();
    if (!trimmed.startsWith(NOTIFICATION_PREFIX)) return trimmed;
    const separator = trimmed.indexOf("\n\n");
    return separator >= 0 ? trimmed.slice(separator + 2).trim() : "";
  }
  const afterMarker = text.slice(start + marker.length);
  const newline = afterMarker.indexOf("\n");
  if (newline < 0) return "";
  return afterMarker.slice(newline + 1).trim();
}

/** Fold markdown into one plain line for a collapsed preview. */
export function subagentReportPreview(body: string, maxChars = 240): string {
  const plain = body
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]*)`/g, "$1")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/^\s{0,3}#{1,6}\s+/gm, "")
    .replace(/^\s{0,3}>\s?/gm, "")
    .replace(/^\s*(?:[-*+]|\d+\.)\s+/gm, "")
    .replace(/^\s*\|?[\s:|-]+\|?\s*$/gm, " ")
    .replace(/[*_~`#>|]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (plain.length <= maxChars) return plain;
  return `${plain.slice(0, maxChars).trimEnd()}…`;
}

export function parseSubagentNotification(message: CustomMessage): SubagentReport | null {
  const details = readSubagentNotificationDetails(message);
  if (!details) return null;
  const sessionId = details.sessionId as string;
  const status = isSubagentStatus(details.status) ? details.status : "completed";
  const text = messageText(message.content);
  const createdAt = toMillis(details.createdAt);
  const completedAt = toMillis(details.completedAt);
  const durationMs = createdAt !== undefined && completedAt !== undefined && completedAt >= createdAt
    ? completedAt - createdAt
    : undefined;
  return {
    sessionId,
    label: subagentReportLabel(details, sessionId),
    profile: typeof details.profile === "string" ? details.profile : "",
    description: typeof details.description === "string" ? details.description : "",
    status,
    runInBackground: details.runInBackground === true,
    ...(createdAt !== undefined ? { createdAt } : {}),
    ...(completedAt !== undefined ? { completedAt } : {}),
    ...(durationMs !== undefined ? { durationMs } : {}),
    ...(typeof details.error === "string" && details.error.trim() ? { error: details.error.trim() } : {}),
    body: extractReportBody(text, sessionId),
    resumed: text.includes(RESUMED_MARKER),
  };
}
