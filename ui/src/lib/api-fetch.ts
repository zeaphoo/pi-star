/**
 * Every ordinary request goes through here: a timeout, a priority class and a
 * readable error. The stream bus keeps long-lived traffic off the browser's
 * per-origin connection budget, but a request still must fail in bounded time
 * instead of spinning forever.
 */

export type ApiErrorCode = "timeout" | "offline" | "network" | "http";

export class ApiError extends Error {
  constructor(
    message: string,
    public readonly code: ApiErrorCode,
    public readonly status?: number,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

export interface ApiFetchInit extends Omit<RequestInit, "priority"> {
  /** Defaults to 30 s. Long reads may pass more; never `Infinity`. */
  timeoutMs?: number;
  /** `background` work is capped so it cannot queue in front of interaction. */
  lane?: "interactive" | "background";
}

const BACKGROUND_LIMIT = 2;
let backgroundActive = 0;
const backgroundQueue: (() => void)[] = [];

function acquireBackground(): Promise<() => void> {
  return new Promise((resolve) => {
    const grant = () => {
      backgroundActive += 1;
      let released = false;
      resolve(() => {
        if (released) return;
        released = true;
        backgroundActive -= 1;
        const next = backgroundQueue.shift();
        if (next) next();
      });
    };
    if (backgroundActive < BACKGROUND_LIMIT) grant();
    else backgroundQueue.push(grant);
  });
}

export async function apiFetch(input: string, init: ApiFetchInit = {}): Promise<Response> {
  const { timeoutMs = 30_000, lane = "interactive", signal, ...rest } = init;
  const release = lane === "background" ? await acquireBackground() : () => {};
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  const forwardAbort = () => controller.abort();
  signal?.addEventListener("abort", forwardAbort, { once: true });
  try {
    return await fetch(input, { ...rest, signal: controller.signal });
  } catch (error) {
    if (signal?.aborted) throw error;
    if (controller.signal.aborted) {
      throw new ApiError("Request timed out. Please retry.", "timeout");
    }
    if (typeof navigator !== "undefined" && navigator.onLine === false) {
      throw new ApiError("The network is offline.", "offline");
    }
    throw new ApiError(error instanceof Error ? error.message : String(error), "network");
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener("abort", forwardAbort);
    release();
  }
}

type PolicyFetch = typeof fetch & { __piStarPolicy?: boolean };

const nativeFetch: typeof fetch | null = typeof fetch === "function" ? fetch.bind(globalThis) : null;

/**
 * Installs a default timeout on `window.fetch` so a request with no call-site
 * timeout still fails instead of waiting forever. `apiFetch` passes its own
 * signal and is left alone; a caller that wants no timeout passes
 * `timeoutMs: 0` through `apiFetch`.
 */
export function installFetchPolicy(defaultTimeoutMs = 60_000): void {
  if (!nativeFetch) return;
  const current = globalThis.fetch as PolicyFetch;
  if (current.__piStarPolicy) return;
  const wrapped: PolicyFetch = async (input, init) => {
    const request = init ?? {};
    if (request.signal) return nativeFetch(input, request);
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), defaultTimeoutMs);
    try {
      return await nativeFetch(input, { ...request, signal: controller.signal });
    } catch (error) {
      if (controller.signal.aborted) {
        throw new ApiError("Request timed out. Please retry.", "timeout");
      }
      throw error;
    } finally {
      clearTimeout(timer);
    }
  };
  wrapped.__piStarPolicy = true;
  globalThis.fetch = wrapped;
}

export async function apiJson<T>(input: string, init: ApiFetchInit = {}): Promise<T> {
  const response = await apiFetch(input, init);
  const body = (await response.json().catch(() => null)) as T | null;
  if (!response.ok) {
    throw new ApiError(
      (body as { error?: string } | null)?.error ?? `HTTP ${response.status}`,
      "http",
      response.status,
    );
  }
  return body as T;
}
