"use client";

import { useState } from "react";
import { useI18n } from "@/hooks/useI18n";
import { copyText } from "@/lib/clipboard";
import { formatActiveDuration } from "@/lib/session-stats-format";
import { shortSubagentId, subagentReportPreview, type SubagentReport } from "@/lib/subagent-report";
import { StatusIcon, statusColor } from "./AgentSessionPanel";
import { SafeMarkdownBody } from "./SafeMarkdownBody";

interface Props {
  report: SubagentReport;
  cwd?: string;
  onOpenFile?: (filePath: string, page?: number) => void;
  onOpenSubagent?: (sessionId: string, label: string) => void;
}

/** A subagent's report renders as a card, not as the model-facing wrapper text. */
const EXPANDED_BODY_MAX_CHARS = 420;
const EXPANDED_BODY_MAX_LINES = 8;

function startsExpanded(report: SubagentReport): boolean {
  const body = report.body || report.error || "";
  return body.length <= EXPANDED_BODY_MAX_CHARS && body.split("\n").length <= EXPANDED_BODY_MAX_LINES;
}

/**
 * One background subagent's completion notice: a header naming the run and its
 * status, a collapsed preview for long reports, and the full markdown plus the
 * subagent's own session link once expanded.
 */
export function SubagentReportView({ report, cwd, onOpenFile, onOpenSubagent }: Props) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState(() => startsExpanded(report));
  const [copied, setCopied] = useState(false);
  const preview = subagentReportPreview(report.body);
  const failed = report.status === "failed" || report.status === "aborted" || report.status === "interrupted";
  const copyValue = report.body || report.error || "";
  const statusLabel = t(`agentSwitcher.status.${report.status}`);

  const copyReport = () => {
    if (!copyValue) return;
    void copyText(copyValue).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  return (
    <div style={{ marginBottom: 16 }}>
      <div
        data-subagent-report=""
        style={{
          border: failed ? "1px solid rgba(248,113,113,0.45)" : "1px solid var(--border)",
          borderRadius: 8,
          overflow: "hidden",
          background: failed ? "rgba(248,113,113,0.04)" : "var(--bg)",
        }}
      >
        <div style={{ display: "flex", alignItems: "stretch", minWidth: 0, background: "var(--bg-panel)" }}>
          <button
            type="button"
            data-subagent-report-toggle=""
            aria-expanded={expanded}
            onClick={() => setExpanded((value) => !value)}
            title={expanded ? t("i18n.collapse") : t("i18n.expand")}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 7,
              flex: 1,
              minWidth: 0,
              padding: "7px 10px",
              border: "none",
              borderBottom: expanded ? "1px solid var(--border)" : "none",
              background: "none",
              color: "var(--text-muted)",
              cursor: "pointer",
              fontSize: 12,
              textAlign: "left",
            }}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={{ flexShrink: 0 }}>
              <rect x="5" y="7" width="14" height="11" rx="2" />
              <path d="M9 11h.01M15 11h.01M9 15h6M12 7V4M10 4h4" />
            </svg>
            <span style={{ color: "var(--text-muted)", fontFamily: "var(--font-mono)", fontSize: 11, fontWeight: 650, flexShrink: 0 }}>
              {t("subagent.report.title")}
            </span>
            <span style={{ color: "var(--text)", fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", flex: 1, minWidth: 0 }}>
              {report.label}
            </span>
            {report.resumed && (
              <span style={{ flexShrink: 0, padding: "0 5px", borderRadius: 999, border: "1px solid var(--border)", color: "var(--text-dim)", fontSize: 10, lineHeight: "15px" }}>
                {t("subagent.report.resumed")}
              </span>
            )}
            <span style={{ display: "flex", alignItems: "center", gap: 4, flexShrink: 0, color: statusColor(report.status), fontSize: 11 }}>
              <StatusIcon status={report.status} />
              {statusLabel}
            </span>
            {report.durationMs !== undefined && (
              <span style={{ flexShrink: 0, color: "var(--text-dim)", fontSize: 11, fontVariantNumeric: "tabular-nums" }}>
                {formatActiveDuration(report.durationMs)}
              </span>
            )}
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="var(--text-dim)" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={{ flexShrink: 0, transform: expanded ? "rotate(180deg)" : "none", transition: "transform 0.15s" }}>
              <polyline points="2 3.5 5 6.5 8 3.5" />
            </svg>
          </button>
          {copyValue && (
            <button
              type="button"
              data-subagent-report-copy=""
              onClick={copyReport}
              title={copied ? t("i18n.copied") : t("i18n.copy")}
              aria-label={t("i18n.copy")}
              style={{ width: 32, display: "grid", placeItems: "center", border: "none", borderLeft: "1px solid var(--border)", background: "none", color: copied ? "var(--accent)" : "var(--text-muted)", cursor: "pointer", flexShrink: 0 }}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <rect x="9" y="9" width="11" height="11" rx="2" />
                <path d="M5 15V5a2 2 0 0 1 2-2h10" />
              </svg>
            </button>
          )}
          {onOpenSubagent && (
            <button
              type="button"
              data-subagent-report-open=""
              onClick={() => onOpenSubagent(report.sessionId, report.label)}
              title={t("subagent.openTab")}
              aria-label={t("subagent.openTab")}
              style={{ width: 32, display: "grid", placeItems: "center", border: "none", borderLeft: "1px solid var(--border)", background: "none", color: "var(--text-muted)", cursor: "pointer", flexShrink: 0 }}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M14 4h6v6M20 4l-9 9" />
                <path d="M18 14v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4" />
              </svg>
            </button>
          )}
        </div>

        {expanded ? (
          <>
            <div style={{ padding: "6px 10px" }}>
              {report.body ? (
                <SafeMarkdownBody className="markdown-custom-message" keepTextLayout cwd={cwd} onOpenFile={onOpenFile}>
                  {report.body}
                </SafeMarkdownBody>
              ) : report.error ? (
                <span style={{ color: "#dc2626", fontSize: 12, whiteSpace: "pre-wrap", wordBreak: "break-word" }}>{report.error}</span>
              ) : (
                <span style={{ color: "var(--text-dim)", fontSize: 12 }}>{t("subagent.report.noOutput")}</span>
              )}
            </div>
            {(report.profile || report.sessionId) && (
              <div
                data-subagent-report-footer=""
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                  padding: "4px 10px",
                  borderTop: "1px solid var(--border)",
                  background: "var(--bg-subtle)",
                  color: "var(--text-dim)",
                  fontSize: 11,
                  minWidth: 0,
                }}
              >
                {report.profile && <span style={{ flexShrink: 0 }}>@{report.profile}</span>}
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontFamily: "var(--font-mono)" }} title={report.sessionId}>
                  {shortSubagentId(report.sessionId)}
                </span>
              </div>
            )}
          </>
        ) : (
          (preview || report.error) && (
            <div
              data-subagent-report-preview=""
              style={{
                padding: "6px 10px 8px",
                color: report.error && !report.body ? "#dc2626" : "var(--text-dim)",
                fontSize: 12,
                lineHeight: 1.5,
                maxHeight: 40,
                overflow: "hidden",
                overflowWrap: "anywhere",
              }}
            >
              {preview || report.error}
            </div>
          )
        )}
      </div>
    </div>
  );
}
