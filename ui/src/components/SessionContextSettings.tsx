"use client";

import { useEffect, useRef, useState } from "react";
import { useI18n } from "@/hooks/useI18n";
import type { ToolEntry } from "@/lib/tool-presets";
import { SystemPromptPanel } from "./SystemPromptPanel";
import { ToolDefinitionsPanel } from "./ToolDefinitionsPanel";
import {
  ConfigButton,
  ConfigDetail,
  ConfigDetailActions,
  ConfigDetailHeader,
  ConfigDetailHeaderInfo,
  ConfigDetailStack,
  ConfigDetailTitle,
  ConfigSidebar,
  ConfigSidebarGroupLabel,
  ConfigSidebarItem,
  ConfigSidebarList,
  ConfigSidebarText,
  ConfigSplitView,
} from "./SettingsUi";

interface Props {
  sessionId: string | null;
  /** The selected session's resolved prompt, as the chat last read it. */
  prompt: string | null;
  tools: ToolEntry[] | null;
  loading: boolean;
  /** Asks the chat to read the prompt and tools again. */
  onLoad: () => void;
}

type ContextView = "prompt" | "tools";

/**
 * Settings › Session context: what the agent is actually sent for this session.
 * It is the two toolbar panels it replaced — "System" and "Tools" — read-only
 * and side by side in one section: the sidebar picks which half to show.
 */
export function SessionContextSettings({ sessionId, prompt, tools, loading, onLoad }: Props) {
  const { t } = useI18n();
  const [view, setView] = useState<ContextView>("prompt");
  // Opening the section reads once, as the old panels did when they opened.
  const requestedRef = useRef(false);
  useEffect(() => {
    if (requestedRef.current) return;
    requestedRef.current = true;
    if (sessionId && prompt === null && !loading) onLoad();
  }, [loading, onLoad, prompt, sessionId]);

  if (!sessionId) {
    return (
      <div className="settings-context-empty">
        <div className="settings-context-empty-title">{t("context.noSession")}</div>
        <div className="settings-context-empty-hint">{t("context.noSessionHint")}</div>
      </div>
    );
  }

  return (
    <div className="settings-context-root">
      <ConfigSplitView>
        <ConfigSidebar>
          <ConfigSidebarList>
            <ConfigSidebarGroupLabel>{t("context.group")}</ConfigSidebarGroupLabel>
            <ConfigSidebarItem
              active={view === "prompt"}
              onClick={() => setView("prompt")}
            >
              <ConfigSidebarText className="is-grow">{t("system.prompt")}</ConfigSidebarText>
            </ConfigSidebarItem>
            <ConfigSidebarItem
              active={view === "tools"}
              onClick={() => setView("tools")}
            >
              <ConfigSidebarText className="is-grow">{t("tools.title")}</ConfigSidebarText>
            </ConfigSidebarItem>
          </ConfigSidebarList>
        </ConfigSidebar>

        <ConfigDetail>
          <ConfigDetailStack className="is-full-height" style={{ gap: 12 }}>
            <ConfigDetailHeader>
              <ConfigDetailHeaderInfo>
                <div style={{ minWidth: 0 }}>
                  <ConfigDetailTitle>
                    {view === "prompt" ? t("system.prompt") : t("tools.title")}
                  </ConfigDetailTitle>
                  <div className="settings-context-hint">
                    {t("context.description")}
                  </div>
                </div>
              </ConfigDetailHeaderInfo>
              <ConfigDetailActions>
                <ConfigButton size="small" onClick={onLoad} disabled={loading}>
                  {loading ? t("context.loading") : t("context.refresh")}
                </ConfigButton>
              </ConfigDetailActions>
            </ConfigDetailHeader>

            {view === "prompt" ? (
              <SystemPromptPanel embedded loading={loading} prompt={prompt} translate={t} />
            ) : (
              <ToolDefinitionsPanel embedded loading={loading} tools={tools} translate={t} />
            )}
          </ConfigDetailStack>
        </ConfigDetail>
      </ConfigSplitView>

      <style>{`
        .settings-context-root {
          display: flex;
          flex-direction: column;
          height: 100%;
          min-height: 0;
        }
        .settings-context-root .config-detail {
          display: flex;
          flex-direction: column;
        }
        .settings-context-root .config-detail-stack.is-full-height > .system-prompt-panel,
        .settings-context-root .config-detail-stack.is-full-height > .tool-definitions-panel {
          flex: 1;
          min-height: 0;
        }
        .settings-context-hint {
          margin-top: 2px;
          color: var(--text-muted);
          font-size: 11px;
          line-height: 1.5;
        }
        @media (max-width: 640px) {
          /* A phone's dialog header already names the detail; the hint would wrap it. */
          .settings-context-hint {
            display: none;
          }
        }
        .settings-context-empty {
          display: flex;
          flex-direction: column;
          gap: 6px;
          align-items: center;
          justify-content: center;
          height: 100%;
          padding: 28px 8px;
          color: var(--text-muted);
          text-align: center;
        }
        .settings-context-empty-title {
          color: var(--text);
          font-size: 13px;
        }
        .settings-context-empty-hint {
          font-size: 11px;
        }
      `}</style>
    </div>
  );
}