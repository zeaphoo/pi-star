"use client";

import type { DockView } from "@/lib/dock-state";

function Icon({ size, children }: { size: number; children: React.ReactNode }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {children}
    </svg>
  );
}

export function DockCodeIcon({ size = 14 }: { size?: number }) {
  return <Icon size={size}><path d="m8 6-6 6 6 6" /><path d="m16 6 6 6-6 6" /></Icon>;
}

/** Lucide's file-diff: the Diff tab and the dock share one picture. */
export function DockDiffIcon({ size = 14 }: { size?: number }) {
  return (
    <Icon size={size}>
      <path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z" />
      <path d="M9 10h6" /><path d="M12 13V7" /><path d="M9 17h6" />
    </Icon>
  );
}

export function DockTerminalIcon({ size = 14 }: { size?: number }) {
  return (
    <Icon size={size}>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <polyline points="7 9 10 12 7 15" /><line x1="13" y1="15" x2="17" y2="15" />
    </Icon>
  );
}

export function dockViewIcon(view: DockView, size = 14) {
  if (view === "code") return <DockCodeIcon size={size} />;
  if (view === "diff") return <DockDiffIcon size={size} />;
  return <DockTerminalIcon size={size} />;
}

/** A square split down the middle: two views side by side. */
export function SplitRowIcon({ size = 13 }: { size?: number }) {
  return (
    <Icon size={size}>
      <rect x="3" y="4" width="18" height="16" rx="1.5" />
      <line x1="12" y1="4" x2="12" y2="20" />
    </Icon>
  );
}

/** A square split across the middle: two views stacked. */
export function SplitColumnIcon({ size = 13 }: { size?: number }) {
  return (
    <Icon size={size}>
      <rect x="4" y="3" width="16" height="18" rx="1.5" />
      <line x1="4" y1="12" x2="20" y2="12" />
    </Icon>
  );
}

export function DockCloseIcon({ size = 12 }: { size?: number }) {
  return <Icon size={size}><path d="M18 6 6 18M6 6l12 12" /></Icon>;
}

export function DockPlusIcon({ size = 14 }: { size?: number }) {
  return <Icon size={size}><line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" /></Icon>;
}

export function DockChevronDownIcon({ size = 12 }: { size?: number }) {
  return <Icon size={size}><path d="m6 9 6 6 6-6" /></Icon>;
}

export function DockExpandIcon({ size = 16, expanded = false }: { size?: number; expanded?: boolean }) {
  return (
    <Icon size={size}>
      <path d={expanded
        ? "M9 3v6H3m12-6v6h6M9 21v-6H3m12 6v-6h6M3 3l6 6m12-6-6 6M3 21l6-6m12 6-6-6"
        : "M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5M3 3l6 6m12-6-6 6M3 21l6-6m12 6-6-6"} />
    </Icon>
  );
}

/** Lucide's panel-right: the toolbar's dock toggle. */
export function DockPanelIcon({ size = 16 }: { size?: number }) {
  return (
    <Icon size={size}>
      <rect x="3" y="3" width="18" height="18" rx="2" /><line x1="15" y1="3" x2="15" y2="21" />
    </Icon>
  );
}