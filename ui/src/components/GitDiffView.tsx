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
  /** When set, a gap row is a button that asks for more context lines. */
  onExpandContext?: () => void;
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

// GitHub draws two number columns (old | new) and tints the gutter with the
// same row color as the code, a shade stronger: the gutter then reads as the
// row's own band instead of a separate stripe that only carries digits.
const GUTTER_CELL = {
  width: 44,
  minWidth: 44,
  padding: "0 8px",
  boxSizing: "border-box",
  borderRight: "1px solid var(--border)",
  color: "var(--code-punctuation)",
  textAlign: "right",
  fontSize: 11,
  fontVariantNumeric: "tabular-nums",
  userSelect: "none",
  flexShrink: 0,
} as const;

function gutterBackground(added: boolean, removed: boolean): string {
  if (added) return "rgba(0, 200, 80, 0.16)";
  if (removed) return "rgba(240, 60, 60, 0.18)";
  return "var(--code-gutter-bg)";
}

function GitDiffFile({ file, onExpandContext }: { file: SplitDiffFile; onExpandContext?: () => void }) {
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
        if (row.type === "hunk") {
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
                whiteSpace: "pre",
                overflow: "hidden",
                textOverflow: "ellipsis",
              }}
            >
              {row.text}
            </div>
          );
        }
        if (row.type === "gap") {
          const label = row.count === null ? "⋯" : `⋯ ${diffGapLabel(row.count, t)}`;
          const gapStyle = {
            padding: "2px 12px",
            borderTop: "1px solid var(--border)",
            borderBottom: "1px solid var(--border)",
            background: "var(--code-gutter-bg)",
            color: "var(--code-punctuation)",
            fontSize: 11,
          } as const;
          if (!onExpandContext) {
            return (
              <div key={ri} style={gapStyle}>
                {label}
              </div>
            );
          }
          return (
            <button
              key={ri}
              type="button"
              onClick={onExpandContext}
              title={t("diff.expandContext")}
              onMouseEnter={(event) => { event.currentTarget.style.background = "var(--bg-hover)"; }}
              onMouseLeave={(event) => { event.currentTarget.style.background = gapStyle.background; }}
              style={{
                ...gapStyle,
                display: "block",
                width: "100%",
                border: "none",
                borderTop: gapStyle.borderTop,
                borderBottom: gapStyle.borderBottom,
                textAlign: "left",
                cursor: "pointer",
                font: "inherit",
                fontSize: 11,
              }}
            >
              {label}
            </button>
          );
        }
        const added = row.type === "added";
        const removed = row.type === "removed";
        const gutterBg = gutterBackground(added, removed);
        const html = diffLineHtml(row, ri, plan, highlighted);
        return (
          <div key={ri} style={gitDiffLineStyle(added, removed)}>
            <span style={{ ...GUTTER_CELL, background: gutterBg }}>
              {row.oldLineNo ?? ""}
            </span>
            <span style={{ ...GUTTER_CELL, background: gutterBg }}>
              {row.newLineNo ?? ""}
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
export function GitDiffView({ patch, emptyLabel, onExpandContext }: Props) {
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
          <GitDiffFile file={file} onExpandContext={onExpandContext} />
        </div>
      ))}
    </div>
  );
}