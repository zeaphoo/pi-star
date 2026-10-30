// Minimal replacement for the three next/navigation hooks pi-web uses.
// AppShell owns its own state; these hooks only keep the URL in sync.
export function useSearchParams(): URLSearchParams {
  if (typeof window === "undefined") return new URLSearchParams();
  return new URLSearchParams(window.location.search);
}

type NavigateOptions = { scroll?: boolean };

function navigate(url: string, replace: boolean, options?: NavigateOptions): void {
  if (typeof window === "undefined") return;
  if (replace) window.history.replaceState(null, "", url);
  else window.history.pushState(null, "", url);
  if (options?.scroll !== false) window.scrollTo(0, 0);
}

export function useRouter() {
  return {
    push: (url: string, options?: NavigateOptions) => navigate(url, false, options),
    replace: (url: string, options?: NavigateOptions) => navigate(url, true, options),
    back: () => window.history.back(),
    forward: () => window.history.forward(),
    refresh: () => window.location.reload(),
    prefetch: () => undefined,
  };
}

export function usePathname(): string {
  if (typeof window === "undefined") return "/";
  return window.location.pathname;
}
