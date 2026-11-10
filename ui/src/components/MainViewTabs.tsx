"use client";

import { useI18n } from "@/hooks/useI18n";
import type { ReactNode } from "react";
import type { MainView } from "@/lib/code-workspace";

interface Props {
  value: MainView;
  onChange: (view: MainView) => void;
  /** Icon-only presentation for narrow phone toolbars. */
  compact?: boolean;
}

function ChatIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M21 15a2 2 0 0 1-2 2H8l-5 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
    </svg>
  );
}

function CodeIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="m8 6-6 6 6 6" />
      <path d="m16 6 6 6-6 6" />
    </svg>
  );
}

/** The chat/code switch that sits in the centre of the chat toolbar. */
export function MainViewTabs({ value, onChange, compact = false }: Props) {
  const { t } = useI18n();
  const tabs: Array<{ id: MainView; label: string; icon: ReactNode }> = [
    { id: "chat", label: t("mainView.chat"), icon: <ChatIcon /> },
    { id: "code", label: t("mainView.code"), icon: <CodeIcon /> },
  ];

  return (
    <div
      role="tablist"
      aria-label={t("mainView.label")}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 2,
        height: 26,
        padding: 2,
        boxSizing: "border-box",
        border: "1px solid var(--border)",
        borderRadius: 8,
        background: "var(--bg)",
        flexShrink: 0,
      }}
    >
      {tabs.map((tab) => {
        const selected = tab.id === value;
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={selected}
            title={compact ? tab.label : undefined}
            aria-label={compact ? tab.label : undefined}
            onClick={() => onChange(tab.id)}
            style={{
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              gap: 5,
              height: 20,
              padding: compact ? "0 7px" : "0 10px",
              border: "none",
              borderRadius: 6,
              background: selected ? "var(--bg-selected)" : "transparent",
              color: selected ? "var(--text)" : "var(--text-muted)",
              cursor: "pointer",
              fontSize: 11,
              fontWeight: selected ? 600 : 500,
              lineHeight: 1,
              whiteSpace: "nowrap",
              transition: "background 0.12s, color 0.12s",
            }}
            onMouseEnter={(event) => {
              if (!selected) event.currentTarget.style.color = "var(--text)";
            }}
            onMouseLeave={(event) => {
              event.currentTarget.style.color = selected ? "var(--text)" : "var(--text-muted)";
            }}
          >
            {tab.icon}
            {!compact && <span>{tab.label}</span>}
          </button>
        );
      })}
    </div>
  );
}
