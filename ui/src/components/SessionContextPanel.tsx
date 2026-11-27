"use client";

import { useEffect, useRef, useState } from "react";
import { useI18n } from "@/hooks/useI18n";
import type { ToolEntry } from "@/lib/tool-presets";
import { SystemPromptPanel } from "./SystemPromptPanel";
import { ToolDefinitionsPanel } from "./ToolDefinitionsPanel";

interface Props {
  sessionId: string;
  /** The selected session's resolved prompt, as the chat last read it. */
  prompt: string | null;
  tools: ToolEntry[] | null;
  loading: boolean;
  /** Asks the chat to read the prompt and tools again. */
  onLoad: () => void;
}

type ContextView = "prompt" | "tools";

/**
 * The selected session's context, opened from its toolbar: the system prompt
 * and the tool definitions the agent is actually sent, side by side behind two
 * tabs. The chat owns the request; this panel only asks it to run.
 */
export function SessionContextPanel({ sessionId, prompt, tools, loading, onLoad }: Props) {
  const { t } = useI18n();
  const [view, setView] = useState<ContextView>("prompt");
  const requestedRef = useRef(false);

  useEffect(() => {
    if (requestedRef.current) return;
    requestedRef.current = true;
    if (sessionId && prompt === null && !loading) onLoad();
  }, [loading, onLoad, prompt, sessionId]);

  return (
    <div
      className="session-context-panel"
      role="dialog"
      aria-label={t("context.title")}
      style={{
        background: "var(--bg-panel)",
        borderLeft: "1px solid var(--border)",
        borderRight: "1px solid var(--border)",
        borderBottom: "1px solid var(--border)",
        borderRadius: "0 0 6px 6px",
        boxShadow: "0 10px 28px rgba(0,0,0,0.10)",
        overflow: "hidden",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px", borderBottom: "1px solid var(--border)" }}>
        <div className="session-context-tabs" role="tablist" aria-label={t("context.title")}>
          <button
            type="button"
            role="tab"
            aria-selected={view === "prompt"}
            className="session-context-tab"
            onClick={() => setView("prompt")}
          >
            {t("system.prompt")}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={view === "tools"}
            className="session-context-tab"
            onClick={() => setView("tools")}
          >
            {t("tools.title")}
          </button>
        </div>
        <span className="session-context-hint">{t("context.description")}</span>
        <button
          type="button"
          className="session-context-refresh"
          onClick={onLoad}
          disabled={loading}
        >
          {loading ? t("context.loading") : t("context.refresh")}
        </button>
      </div>

      {view === "prompt" ? (
        <SystemPromptPanel loading={loading} prompt={prompt} translate={t} />
      ) : (
        <ToolDefinitionsPanel loading={loading} tools={tools} translate={t} />
      )}
    </div>
  );
}
