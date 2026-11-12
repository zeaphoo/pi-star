// The code view's Prism tokenisation runs in a worker so a large file cannot
// block scrolling, selection or the rest of the workbench. The worker owns
// refractor/all; the main bundle only gets this small client and the fallback
// loader is dynamic.

interface PendingRequest {
  resolve: (lines: string[][]) => void;
  reject: (error: Error) => void;
}

interface HighlightResponse {
  id: number;
  lines?: string[][];
  error?: string;
}

interface CacheEntry {
  lines: string[][];
  chars: number;
}

const MAX_CACHE_ENTRIES = 8;
const MAX_CACHE_CHARS = 32_000_000;

const cache = new Map<string, CacheEntry>();
let cacheChars = 0;

let worker: Worker | null = null;
let workerBroken = false;
let nextRequestId = 1;
const pending = new Map<number, PendingRequest>();

function cacheKey(segments: readonly string[], language: string): string {
  return `${language}\u0000${segments.join("\u0000")}`;
}

function sizeOf(lines: string[][]): number {
  let size = 0;
  for (const segment of lines) {
    for (const line of segment) size += line.length + 8;
  }
  return size;
}

function readCache(key: string): string[][] | null {
  const entry = cache.get(key);
  if (!entry) return null;
  // Refresh insertion order so the least recently used entry is evicted first.
  cache.delete(key);
  cache.set(key, entry);
  return entry.lines;
}

function writeCache(key: string, lines: string[][]): void {
  const previous = cache.get(key);
  if (previous) {
    cacheChars -= previous.chars;
    cache.delete(key);
  }
  const chars = sizeOf(lines);
  cache.set(key, { lines, chars });
  cacheChars += chars;
  while (cache.size > MAX_CACHE_ENTRIES || cacheChars > MAX_CACHE_CHARS) {
    const oldestKey = cache.keys().next().value;
    if (oldestKey === undefined) break;
    const oldest = cache.get(oldestKey);
    cache.delete(oldestKey);
    if (oldest) cacheChars -= oldest.chars;
  }
}

function rejectPending(error: Error): void {
  for (const request of pending.values()) request.reject(error);
  pending.clear();
}

function ensureWorker(): Worker | null {
  if (workerBroken) return null;
  if (worker) return worker;
  if (typeof Worker === "undefined") {
    workerBroken = true;
    return null;
  }
  try {
    worker = new Worker(new URL("./highlight-worker.ts", import.meta.url), { type: "module" });
  } catch {
    workerBroken = true;
    return null;
  }
  worker.onmessage = (event: MessageEvent<HighlightResponse>) => {
    const request = pending.get(event.data.id);
    if (!request) return;
    pending.delete(event.data.id);
    if (event.data.error || !event.data.lines) {
      request.reject(new Error(event.data.error ?? "Highlight worker returned no lines"));
      return;
    }
    request.resolve(event.data.lines);
  };
  worker.onerror = () => {
    workerBroken = true;
    const broken = worker;
    worker = null;
    broken?.terminate();
    rejectPending(new Error("Highlight worker failed"));
  };
  return worker;
}

function requestWorker(
  activeWorker: Worker,
  segments: readonly string[],
  language: string,
): Promise<string[][]> {
  const id = nextRequestId++;
  return new Promise<string[][]>((resolve, reject) => {
    pending.set(id, { resolve, reject });
    activeWorker.postMessage({ id, segments, language });
  });
}

async function requestMainThread(
  segments: readonly string[],
  language: string,
): Promise<string[][]> {
  // Only reached when the browser has no worker or the worker failed. The
  // dynamic import keeps refractor/all out of the main chunk until then.
  const core = await import("./highlight-core");
  return core.highlightSegments(segments, language);
}

/** Cached result for a synchronous first paint, or null while it is computed. */
export function cachedHighlight(
  segments: readonly string[],
  language: string,
): string[][] | null {
  return readCache(cacheKey(segments, language));
}

/** Highlight independent code segments, newest request last. */
export async function highlightSegments(
  segments: readonly string[],
  language: string,
): Promise<string[][]> {
  const key = cacheKey(segments, language);
  const cached = readCache(key);
  if (cached) return cached;

  const activeWorker = ensureWorker();
  let lines: string[][];
  if (activeWorker) {
    try {
      lines = await requestWorker(activeWorker, segments, language);
    } catch {
      lines = await requestMainThread(segments, language);
    }
  } else {
    lines = await requestMainThread(segments, language);
  }
  writeCache(key, lines);
  return lines;
}