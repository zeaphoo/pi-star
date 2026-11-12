"use client";

import { useSyncExternalStore } from "react";
import { ICON_THEMES, type IconThemeId } from "@/lib/icon-themes";

export const FILE_ICON_THEME_STORAGE_KEY = "pi-file-icon-theme";
const DEFAULT_THEME: IconThemeId = "catppuccin";
const THEME_IDS = ICON_THEMES.map((theme) => theme.id);
let theme: IconThemeId | null = null;
const listeners = new Set<() => void>();

function isIconTheme(value: unknown): value is IconThemeId {
  return typeof value === "string" && (THEME_IDS as string[]).includes(value);
}

function readStoredTheme(): IconThemeId {
  try {
    const stored = window.localStorage.getItem(FILE_ICON_THEME_STORAGE_KEY);
    return isIconTheme(stored) ? stored : DEFAULT_THEME;
  } catch {
    return DEFAULT_THEME;
  }
}

function getSnapshot(): IconThemeId {
  if (typeof window === "undefined") return DEFAULT_THEME;
  if (!theme) theme = readStoredTheme();
  return theme;
}

function notify(): void {
  listeners.forEach((listener) => listener());
}

function onStorage(event: StorageEvent): void {
  if (event.key !== null && event.key !== FILE_ICON_THEME_STORAGE_KEY) return;
  const next = readStoredTheme();
  if (next === getSnapshot()) return;
  theme = next;
  notify();
}

function subscribe(listener: () => void): () => void {
  if (listeners.size === 0) window.addEventListener("storage", onStorage);
  listeners.add(listener);
  getSnapshot();
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) window.removeEventListener("storage", onStorage);
  };
}

export function setFileIconTheme(value: IconThemeId): void {
  if (!isIconTheme(value) || value === getSnapshot()) return;
  theme = value;
  try {
    if (value === DEFAULT_THEME) window.localStorage.removeItem(FILE_ICON_THEME_STORAGE_KEY);
    else window.localStorage.setItem(FILE_ICON_THEME_STORAGE_KEY, value);
  } catch {
    // Apply in memory even if the browser cannot save the preference.
  }
  notify();
}

export function useFileIconTheme() {
  const snapshot = useSyncExternalStore(subscribe, getSnapshot, () => DEFAULT_THEME);
  return { theme: snapshot, setTheme: setFileIconTheme };
}