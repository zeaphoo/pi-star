import { useSyncExternalStore } from "react";

export type EnterSendMode = "enter" | "ctrlEnter";

export const ENTER_SEND_MODE_STORAGE_KEY = "pi-enter-send-mode";
export const ENTER_SEND_MODE_DEFAULT: EnterSendMode = "enter";

function readStoredMode(): EnterSendMode {
  try {
    const raw = window.localStorage.getItem(ENTER_SEND_MODE_STORAGE_KEY);
    return raw === "ctrlEnter" ? "ctrlEnter" : "enter";
  } catch {
    return ENTER_SEND_MODE_DEFAULT;
  }
}

let mode: EnterSendMode | null = null;
const listeners = new Set<() => void>();

function getSnapshot(): EnterSendMode {
  if (mode === null) mode = readStoredMode();
  return mode;
}

function notify(): void {
  listeners.forEach((listener) => listener());
}

/** Another tab wrote the choice: this tab sends Enter the same way. */
function onStorage(event: StorageEvent): void {
  if (event.key !== null && event.key !== ENTER_SEND_MODE_STORAGE_KEY) return;
  const next = readStoredMode();
  if (next === getSnapshot()) return;
  mode = next;
  notify();
}

function subscribe(listener: () => void): () => void {
  if (listeners.size === 0) window.addEventListener("storage", onStorage);
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) window.removeEventListener("storage", onStorage);
  };
}

export function setEnterSendMode(next: EnterSendMode): void {
  // Keep the choice in memory even when the browser refuses to save it: the
  // composer should still send the way the user just chose.
  mode = next;
  try {
    window.localStorage.setItem(ENTER_SEND_MODE_STORAGE_KEY, next);
  } catch {}
  notify();
}

export function useEnterSendMode(): EnterSendMode {
  return useSyncExternalStore(subscribe, getSnapshot, () => ENTER_SEND_MODE_DEFAULT);
}