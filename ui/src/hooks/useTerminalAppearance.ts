import { useSyncExternalStore } from "react";
import {
  DEFAULT_TERMINAL_APPEARANCE,
  sanitizeTerminalAppearance,
  TERMINAL_APPEARANCE_STORAGE_KEY,
  type TerminalAppearance,
} from "@/lib/terminal-appearance";

let appearance: TerminalAppearance | null = null;
const listeners = new Set<() => void>();

function readStoredAppearance(): TerminalAppearance {
  try {
    const raw = window.localStorage.getItem(TERMINAL_APPEARANCE_STORAGE_KEY);
    if (!raw) return { ...DEFAULT_TERMINAL_APPEARANCE };
    return sanitizeTerminalAppearance(JSON.parse(raw));
  } catch {
    return { ...DEFAULT_TERMINAL_APPEARANCE };
  }
}

function getSnapshot(): TerminalAppearance {
  if (appearance === null) appearance = readStoredAppearance();
  return appearance;
}

function notify(): void {
  listeners.forEach((listener) => listener());
}

// Another tab changed a value; `key` is null when that tab cleared its storage.
function onStorage(event: StorageEvent): void {
  if (event.key !== null && event.key !== TERMINAL_APPEARANCE_STORAGE_KEY) return;
  const next = readStoredAppearance();
  if (JSON.stringify(next) === JSON.stringify(getSnapshot())) return;
  appearance = next;
  notify();
}

function subscribe(listener: () => void): () => void {
  if (listeners.size === 0) {
    window.addEventListener("storage", onStorage);
    // Nobody listened for another tab's change meanwhile.
    appearance = readStoredAppearance();
  }
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) window.removeEventListener("storage", onStorage);
  };
}

function persist(next: TerminalAppearance): void {
  appearance = next;
  try {
    if (JSON.stringify(next) === JSON.stringify(DEFAULT_TERMINAL_APPEARANCE)) {
      window.localStorage.removeItem(TERMINAL_APPEARANCE_STORAGE_KEY);
    } else {
      window.localStorage.setItem(TERMINAL_APPEARANCE_STORAGE_KEY, JSON.stringify(next));
    }
  } catch {
    // Applies in memory even if the browser cannot save it.
  }
  notify();
}

export function setTerminalAppearance(patch: Partial<TerminalAppearance>): void {
  persist(sanitizeTerminalAppearance({ ...getSnapshot(), ...patch }));
}

export function resetTerminalAppearance(): void {
  persist({ ...DEFAULT_TERMINAL_APPEARANCE });
}

/** Terminal-only typography; changing it never recreates the shell or stream. */
export function useTerminalAppearance() {
  const snapshot = useSyncExternalStore(subscribe, getSnapshot, () => DEFAULT_TERMINAL_APPEARANCE);
  return {
    ...snapshot,
    setAppearance: setTerminalAppearance,
    resetAppearance: resetTerminalAppearance,
  };
}
