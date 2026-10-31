"use client";

import { useEffect, useRef, useState } from "react";
import { useI18n } from "@/hooks/useI18n";
import { useIsMobile } from "@/hooks/useIsMobile";
import { SelectorRow } from "./SelectorRow";

export interface AgentBackendOption {
  id: string;
  name: string;
}

interface Props {
  options: AgentBackendOption[];
  value: string;
  onChange: (backend: string) => void;
  /** Switching is refused while the session has work under way. */
  disabled?: boolean;
}

/**
 * The session's agent backend: the standard Node.js pi (`rpc`) or the
 * in-process Rust harness (`quick`). The server persists the pick in the
 * session file; the label comes from `GET /api/agent/backends`.
 */
export function AgentBackendSelector({
  options,
  value,
  onChange,
  disabled = false,
}: Props) {
  const { t } = useI18n();
  const isMobile = useIsMobile();
  const rootRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  useEffect(() => {
    if (!open) return;
    const handleOutsideClick = (event: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", handleOutsideClick);
    return () => document.removeEventListener("mousedown", handleOutsideClick);
  }, [open]);

  const selected = options.find((option) => option.id === value);
  const currentName = selected?.name
    ?? (value === "quick" ? "极速 PI (QuickPI)" : "标准 PI (Node.js)");

  return (
    <div ref={rootRef} style={{ position: "relative", flexShrink: 0 }}>
      <button
        type="button"
        onClick={() => {
          if (!disabled) setOpen((current) => !current);
        }}
        disabled={disabled}
        title={disabled
          ? t("chat.backendLocked")
          : t("chat.changeBackend", { name: currentName })}
        aria-label={t("chat.changeBackend", { name: currentName })}
        aria-expanded={open}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 5,
          height: 32,
          padding: isMobile ? "0 6px" : "8px 10px",
          maxWidth: isMobile ? 32 : 150,
          background: open ? "var(--bg-hover)" : "none",
          border: "none",
          borderRadius: 9,
          color: "var(--text-muted)",
          cursor: disabled ? "not-allowed" : "pointer",
          fontSize: 12,
          opacity: disabled ? 0.5 : 1,
          transition: "background 0.12s, color 0.12s",
        }}
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round">
          <rect x="4" y="4" width="16" height="16" rx="3" />
          <path d="M9 9h6v6H9z" />
          <path d="M9 2v2M15 2v2M9 20v2M15 20v2M2 9h2M2 15h2M20 9h2M20 15h2" />
        </svg>
        {!isMobile && (
          <span style={{
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}>
            {currentName}
          </span>
        )}
      </button>
      {open && (
        <div style={{
          position: "absolute",
          bottom: "calc(100% + 6px)",
          left: 0,
          zIndex: 100,
          minWidth: 210,
          background: "var(--bg)",
          border: "1px solid var(--border)",
          borderRadius: 8,
          boxShadow: "0 -4px 16px rgba(0,0,0,0.10)",
          overflow: "hidden",
        }}>
          {options.map((option) => (
            <SelectorRow
              key={option.id}
              active={option.id === value}
              onSelect={() => {
                setOpen(false);
                if (option.id !== value) onChange(option.id);
              }}
            >
              <span style={{ flex: 1 }}>{option.name}</span>
              {option.id === value && (
                <span style={{ fontSize: 11, color: "var(--text-dim)", marginLeft: 8 }}>
                  {t("chat.backendActive")}
                </span>
              )}
            </SelectorRow>
          ))}
        </div>
      )}
    </div>
  );
}
