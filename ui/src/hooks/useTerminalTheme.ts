import { useSyncExternalStore } from "react";
import { useTheme } from "@/hooks/useTheme";
import {
  parseTerminalThemePreference,
  resolveTerminalTheme,
  TERMINAL_THEME_DEFAULT,
  TERMINAL_THEME_STORAGE_KEY,
  type TerminalTheme,
  type TerminalThemePreference,
} from "@/lib/terminal-theme";

function readStoredTheme(): TerminalThemePreference {
  try {
    return parseTerminalThemePreference(window.localStorage.getItem(TERMINAL_THEME_STORAGE_KEY));
  } catch {
    return TERMINAL_THEME_DEFAULT;
  }
}

let preference: TerminalThemePreference | null = null;
const listeners = new Set<() => void>();

function getSnapshot(): TerminalThemePreference {
  if (preference === null) preference = readStoredTheme();
  return preference;
}

function notify(): void {
  listeners.forEach((listener) => listener());
}

// Another tab chose; `key` is null when that tab cleared its storage.
function onStorage(event: StorageEvent): void {
  if (event.key !== null && event.key !== TERMINAL_THEME_STORAGE_KEY) return;
  const next = readStoredTheme();
  if (next === preference) return;
  preference = next;
  notify();
}

function subscribe(listener: () => void): () => void {
  if (listeners.size === 0) {
    window.addEventListener("storage", onStorage);
    // Nobody listened for another tab's choice meanwhile.
    preference = readStoredTheme();
  }
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) window.removeEventListener("storage", onStorage);
  };
}

export function setTerminalThemePreference(next: TerminalThemePreference): void {
  if (next === getSnapshot()) return;
  preference = next;
  try {
    if (next === TERMINAL_THEME_DEFAULT) window.localStorage.removeItem(TERMINAL_THEME_STORAGE_KEY);
    else window.localStorage.setItem(TERMINAL_THEME_STORAGE_KEY, next);
  } catch {
    // Applies in memory even if the browser cannot save it.
  }
  notify();
}

/** The stored preference, for the Settings selector. */
export function useTerminalThemePreference() {
  const snapshot = useSyncExternalStore(subscribe, getSnapshot, () => TERMINAL_THEME_DEFAULT);
  return { preference: snapshot, setPreference: setTerminalThemePreference };
}

/** Every open terminal shares one choice; `auto` follows the app's light/dark. */
export function useTerminalTheme(): TerminalTheme {
  const { preference: stored } = useTerminalThemePreference();
  const { isDark } = useTheme();
  return resolveTerminalTheme(stored, isDark);
}
