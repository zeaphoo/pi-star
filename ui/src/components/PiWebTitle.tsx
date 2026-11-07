"use client";

/**
 * The sidebar's brand with the files below the sessions (its toolbar row,
 * components/SessionSidebar.tsx; the tabs layout has its tabs there): the
 * panel's typographic anchor, as on the old sidebar. The versions it once
 * scrambled to on a click are in Settings › General now.
 */

export const BRAND_TEXT = "Pi Star";

export function PiWebTitle() {
  return (
    <span className="sidebar-brand">
      <svg className="sidebar-brand-logo" viewBox="0 0 512 512" aria-hidden="true">
        <rect width="512" height="512" rx="116" fill="#0e2925" />
        <rect x="16" y="16" width="480" height="480" rx="102" fill="none" stroke="#24423d" strokeWidth="4" />
        <path
          d="M 256.00 74.00 L 307.25 189.54 L 432.33 202.49 L 338.97 287.05 L 365.13 410.51 L 256.00 347.00 L 146.87 410.51 L 173.03 287.05 L 79.67 202.49 L 204.75 189.54 Z"
          fill="#f6e7bd"
          stroke="#f6e7bd"
          strokeWidth="20"
          strokeLinejoin="round"
        />
      </svg>
      {BRAND_TEXT}
    </span>
  );
}
