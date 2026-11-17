"use client";

import { useI18n } from "@/hooks/useI18n";
import { getFileName } from "@/lib/file-paths";
import type { WrittenFile } from "@/lib/turn-written-files";
import { FileIcon } from "./FileIcons";

/**
 * Lists the files a turn actually wrote, as buttons that open each one in the
 * preview pane. Entries come from the turn's successful `write`/`edit` tool
 * calls — the reply text is never scanned for paths.
 */
export function TurnWrittenFiles({ files, missingPaths, onOpenFile }: {
  files: WrittenFile[];
  /** Files from this turn that no longer exist on disk. */
  missingPaths?: ReadonlySet<string>;
  onOpenFile?: (filePath: string) => void;
}) {
  const { t } = useI18n();
  if (files.length === 0) return null;

  return (
    <div aria-label={t("chat.filesWritten")} style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6, marginTop: 6 }}>
      {files.map(({ filePath }) => {
        const name = getFileName(filePath);
        const deleted = missingPaths?.has(filePath) ?? false;
        return (
          <button
            key={filePath}
            type="button"
            disabled={deleted}
            title={deleted ? t("artifacts.deletedHint") : filePath}
            aria-label={deleted
              ? `${t("chat.openWrittenFile", { name })} (${t("artifacts.deleted")})`
              : t("chat.openWrittenFile", { name })}
            onClick={() => { if (!deleted) onOpenFile?.(filePath); }}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 4,
              padding: "2px 8px",
              fontSize: 12,
              fontFamily: "var(--font-mono)",
              color: deleted ? "var(--text-dim)" : "var(--text)",
              background: deleted ? "transparent" : "var(--bg-subtle)",
              border: "1px solid var(--border)",
              borderRadius: 6,
              cursor: deleted ? "default" : "pointer",
              opacity: deleted ? 0.7 : 1,
            }}
          >
            <FileIcon name={name} size={12} />
            <span style={deleted ? { textDecoration: "line-through" } : undefined}>{name}</span>
            {deleted && <span style={{ color: "var(--text-dim)", fontSize: 10 }}>· {t("artifacts.deleted")}</span>}
          </button>
        );
      })}
    </div>
  );
}
