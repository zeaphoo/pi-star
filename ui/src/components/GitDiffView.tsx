"use client";

import { useMemo } from "react";
import { diffGapLabel, parseUnifiedPatch, unifiedDiffLines, type SplitDiffFile } from "@/lib/patch";
import { buildDiffHighlightPlan, diffLineHtml } from "@/lib/diff-highlight";
import { languageForPath } from "@/lib/file-language";
import { useHighlightedSegments } from "@/hooks/useHighlightedSegments";
import { useI18n } from "@/hooks/useI18n";

interface Props {
  patch: string | null;
  emptyLabel: string;
}

const CODE_STYLE = {
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  lineHeight: "19px",
} as const;

function fileLabel(oldPath?: string, newPath?: string): string {
  if (oldPath && newPath && oldPath !== newPath) return `${oldPath} → ${newPath}`;
  return newPath ?? oldPath ?? "";
}

function gitDiffLineStyle(added: boolean, removed: boolean) {
  return {
    display: "flex",
    minWidth: "max-content",
    background: added
      ? "rgba(0, 200, 80, 0.10)"
      : removed
        ? "rgba(240, 60, 60, 0.12)"
        : "transparent",
  } as const;
}

function GitDiffFile({ file }: { file: SplitDiffFile }) {
  const { t } = useI18n();
  const rows = useMemo(() => unifiedDiffLines(file), [file]);
  const plan = useMemo(() => buildDiffHighlightPlan(rows), [rows]);
  const language = useMemo(
    () => languageForPath(file.newPath ?? file.oldPath ?? ""),
    [file.newPath, file.oldPath],
  );
  const highlighted = useHighlightedSegments(plan.segments, language, language !== "text");

  return (
    <>
      {rows.map((row, ri) => {
        if (row.type === "gap") {
          return (
            <div
              key={ri}
              style={{
                padding: "2px 12px",
                borderTop: "1px solid var(--border)",
                borderBottom: "1px solid var(--border)",
                background: "var(--code-gutter-bg)",
                color: "var(--code-punctuation)",
                fontSize: 11,
              }}
            >
              {row.count === null ? "⋯" : `⋯ ${diffGapLabel(row.count, t)}`}
            </div>
          );
        }
        const added = row.type === "added";
        const removed = row.type === "removed";
        const lineNo = removed ? row.oldLineNo : row.newLineNo;
        const html = diffLineHtml(row, ri, plan, highlighted);
        return (
          <div key={ri} style={gitDiffLineStyle(added, removed)}>
            <span
              style={{
                width: 46,
                minWidth: 46,
                padding: "0 8px",
                boxSizing: "border-box",
                borderRight: "1px solid var(--border)",
                background: "var(--code-gutter-bg)",
                color: "var(--code-punctuation)",
                textAlign: "right",
                fontSize: 10,
                userSelect: "none",
              }}
            >
              {lineNo ?? ""}
            </span>
            <span
              style={{
                width: 16,
                minWidth: 16,
                color: added ? "#4ade80" : removed ? "#f87171" : "var(--code-punctuation)",
                userSelect: "none",
                textAlign: "center",
              }}
            >
              {added ? "+" : removed ? "-" : " "}
            </span>
            {html !== null ? (
              <span
                style={{ padding: "0 10px 0 0", color: "var(--code-fg)", whiteSpace: "pre" }}
                dangerouslySetInnerHTML={{ __html: html || "\u00a0" }}
              />
            ) : (
              <span style={{ padding: "0 10px 0 0", color: "var(--code-fg)", whiteSpace: "pre" }}>
                {row.text || "\u00a0"}
              </span>
            )}
          </div>
        );
      })}
    </>
  );
}

/** A compact unified diff for the Git tab, using the same parser as the file viewer. */
export function GitDiffView({ patch, emptyLabel }: Props) {
  const files = patch ? parseUnifiedPatch(patch) : null;

  if (!patch || !files) {
    return (
      <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center", padding: 24, color: "var(--text-dim)", fontSize: 12, textAlign: "center" }}>
        {emptyLabel}
      </div>
    );
  }

  return (
    <div style={{ height: "100%", overflow: "auto", background: "var(--code-bg)", ...CODE_STYLE }}>
      {files.map((file, fi) => (
        <div key={`${file.newPath ?? file.oldPath ?? "diff"}:${fi}`}>
          <div
            style={{
              position: "sticky",
              top: 0,
              zIndex: 1,
              padding: "5px 12px",
              borderTop: fi ? "1px solid var(--border)" : undefined,
              borderBottom: "1px solid var(--border)",
              background: "var(--bg-panel)",
              color: "var(--text)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
            title={fileLabel(file.oldPath, file.newPath)}
          >
            {fileLabel(file.oldPath, file.newPath)}
          </div>
          <GitDiffFile file={file} />
        </div>
      ))}
    </div>
  );
}