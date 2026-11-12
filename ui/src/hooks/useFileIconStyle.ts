"use client";

import { useSyncExternalStore } from "react";

/** Whether the tree draws Catppuccin's own colors or the app's restrained text tone. */
export type FileIconStyle = "mono" | "colorful";

export const FILE_ICON_STYLE_STORAGE_KEY = "pi-file-icon-style";
export const FILE_ICON_MONO_CLASS = "file-icons-mono";

// Catppuccin's language colors are the default; the monochrome look is the
// opt-out, so a fresh install shows the icons as their SVGs draw them.
const DEFAULT_STYLE: FileIconStyle = "colorful";
let style: FileIconStyle | null = null;
const listeners = new Set<() => void>();

function isFileIconStyle(value: unknown): value is FileIconStyle {
  return value === "mono" || value === "colorful";
}

function readStoredStyle(): FileIconStyle {
  try {
    const stored = window.localStorage.getItem(FILE_ICON_STYLE_STORAGE_KEY);
    return isFileIconStyle(stored) ? stored : DEFAULT_STYLE;
  } catch {
    return DEFAULT_STYLE;
  }
}

function applyStyle(value: FileIconStyle): void {
  if (typeof document === "undefined") return;
  document.documentElement.classList.toggle(FILE_ICON_MONO_CLASS, value === "mono");
}

function getSnapshot(): FileIconStyle {
  if (typeof window === "undefined") return DEFAULT_STYLE;
  if (!style) {
    style = readStoredStyle();
    applyStyle(style);
  }
  return style;
}

function notify(): void {
  listeners.forEach((listener) => listener());
}

function onStorage(event: StorageEvent): void {
  if (event.key !== null && event.key !== FILE_ICON_STYLE_STORAGE_KEY) return;
  const next = readStoredStyle();
  if (next === getSnapshot()) return;
  style = next;
  applyStyle(next);
  notify();
}

function subscribe(listener: () => void): () => void {
  if (listeners.size === 0) window.addEventListener("storage", onStorage);
  listeners.add(listener);
  // Apply the stored choice even when Settings has never been opened.
  getSnapshot();
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) window.removeEventListener("storage", onStorage);
  };
}

export function setFileIconStyle(value: FileIconStyle): void {
  if (!isFileIconStyle(value) || value === getSnapshot()) return;
  style = value;
  applyStyle(value);
  try {
    if (value === DEFAULT_STYLE) window.localStorage.removeItem(FILE_ICON_STYLE_STORAGE_KEY);
    else window.localStorage.setItem(FILE_ICON_STYLE_STORAGE_KEY, value);
  } catch {
    // Apply in memory even if the browser cannot save the preference.
  }
  notify();
}

export function useFileIconStyle() {
  const snapshot = useSyncExternalStore(subscribe, getSnapshot, () => DEFAULT_STYLE);
  return {
    style: snapshot,
    colorful: snapshot === "colorful",
    setColorful: (colorful: boolean) => setFileIconStyle(colorful ? "colorful" : "mono"),
  };
}