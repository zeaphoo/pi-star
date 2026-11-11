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

/** Lucide's file-diff: a file with a plus and a minus, not a Git branch:
 *  the tab will hold diffs from more than Git (revisions, comparisons). */
function DiffIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z" />
      <path d="M9 10h6" />
      <path d="M12 13V7" />
      <path d="M9 17h6" />
    </svg>
  );
}

function TerminalIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <polyline points="7 9 10 12 7 15" />
      <line x1="13" y1="15" x2="17" y2="15" />
    </svg>
  );
}

/** The chat/code switch that sits in the centre of the chat toolbar. */
export function MainViewTabs({ value, onChange, compact = false }: Props) {
  const { t } = useI18n();
  const tabs: Array<{ id: MainView; label: string; icon: ReactNode }> = [
    { id: "chat", label: t("mainView.chat"), icon: <ChatIcon /> },
    { id: "code", label: t("mainView.code"), icon: <CodeIcon /> },
    { id: "git", label: t("mainView.diff"), icon: <DiffIcon /> },
    { id: "terminal", label: t("mainView.terminal"), icon: <TerminalIcon /> },
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
