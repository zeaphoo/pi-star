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

function GitIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="6" cy="6" r="2.5" />
      <circle cx="6" cy="18" r="2.5" />
      <circle cx="18" cy="6" r="2.5" />
      <path d="M6 8.5v7M8.5 6h4a5.5 5.5 0 0 1 5.5 5.5v.5" />
    </svg>
  );
}

/** The chat/code switch that sits in the centre of the chat toolbar. */
export function MainViewTabs({ value, onChange, compact = false }: Props) {
  const { t } = useI18n();
  const tabs: Array<{ id: MainView; label: string; icon: ReactNode }> = [
    { id: "chat", label: t("mainView.chat"), icon: <ChatIcon /> },
    { id: "code", label: t("mainView.code"), icon: <CodeIcon /> },
    { id: "git", label: t("mainView.git"), icon: <GitIcon /> },
  ];

  return (
    <div role="tablist" aria-label={t("mainView.label")} className="main-view-tabs">
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
            data-compact={compact ? "true" : undefined}
            className="main-view-tab"
            onClick={() => onChange(tab.id)}
          >
            {tab.icon}
            {!compact && <span>{tab.label}</span>}
          </button>
        );
      })}
    </div>

  );
}
