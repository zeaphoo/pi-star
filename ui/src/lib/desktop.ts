/**
 * Desktop shell marker injected by `app/` before page scripts run.
 *
 * The object is read-only and intentionally contains no Tauri invoke surface:
 * the main webview has no desktop command permissions. Keep this module free of
 * `@tauri-apps/api` imports so the browser bundle stays shell-agnostic.
 */

export interface PiStarDesktopBridge {
  version: number;
  desktop: true;
  platform: string;
  shell: "tauri";
}

declare global {
  interface Window {
    __PI_STAR__?: PiStarDesktopBridge;
  }
}

export function desktopBridge(): PiStarDesktopBridge | null {
  if (typeof window === "undefined") return null;
  const bridge = window.__PI_STAR__;
  if (!bridge || bridge.desktop !== true) return null;
  return bridge;
}

export function isDesktop(): boolean {
  return desktopBridge() !== null;
}

export function desktopPlatform(): string | null {
  return desktopBridge()?.platform ?? null;
}
