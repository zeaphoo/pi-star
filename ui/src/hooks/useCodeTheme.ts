"use client";

import { useSyncExternalStore } from "react";
import {
  CODE_THEME_DEFAULT,
  CODE_THEME_STORAGE_KEY,
  parseCodeThemePreference,
  type CodeThemePreference,
} from "@/lib/code-theme";

/**
 * The chosen code palette reaches the page as the `data-code-theme` attribute
 * on <html>: app/globals.css turns it into `--code-*` values that out-specify
 * the appearance theme's. `auto` removes it, so the appearance theme's own
 * roles apply.
 */
function applyCodeTheme(preference: CodeThemePreference): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  if (preference === CODE_THEME_DEFAULT) delete root.dataset.codeTheme;
  else root.dataset.codeTheme = preference;
}

function readStoredTheme(): CodeThemePreference {
  try {
    return parseCodeThemePreference(window.localStorage.getItem(CODE_THEME_STORAGE_KEY));
  } catch {
    return CODE_THEME_DEFAULT;
  }
}

let preference: CodeThemePreference | null = null;
const listeners = new Set<() => void>();

function getSnapshot(): CodeThemePreference {
  if (preference === null) {
    preference = readStoredTheme();
    // index.html restores the attribute before first paint; this also covers
    // a document that was not bootstrapped by it (tests, embedded views).
    applyCodeTheme(preference);
  }
  return preference;
}

function notify(): void {
  listeners.forEach((listener) => listener());
}

// Another tab chose; `key` is null when that tab cleared its storage.
function onStorage(event: StorageEvent): void {
  if (event.key !== null && event.key !== CODE_THEME_STORAGE_KEY) return;
  const next = readStoredTheme();
  if (next === preference) return;
  preference = next;
  applyCodeTheme(next);
  notify();
}

function subscribe(listener: () => void): () => void {
  if (listeners.size === 0) {
    window.addEventListener("storage", onStorage);
    // Nobody listened for another tab's choice meanwhile.
    preference = readStoredTheme();
    applyCodeTheme(preference);
  }
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) window.removeEventListener("storage", onStorage);
  };
}

export function setCodeThemePreference(next: CodeThemePreference): void {
  if (next === getSnapshot()) return;
  preference = next;
  try {
    if (next === CODE_THEME_DEFAULT) window.localStorage.removeItem(CODE_THEME_STORAGE_KEY);
    else window.localStorage.setItem(CODE_THEME_STORAGE_KEY, next);
  } catch {
    // Applies in memory even if the browser cannot save it.
  }
  applyCodeTheme(next);
  notify();
}

/** The stored preference, for the Settings selector. */
export function useCodeThemePreference() {
  const snapshot = useSyncExternalStore(subscribe, getSnapshot, () => CODE_THEME_DEFAULT);
  return { preference: snapshot, setPreference: setCodeThemePreference };
}