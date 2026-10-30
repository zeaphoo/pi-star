// Minimal replacement for the three next/navigation hooks pi-web uses.
// AppShell owns its own state; these hooks only keep the URL in sync.
//
// Next returns the same router object for the life of a mount; effects such as
// AppShell's `?cwd=` validation depend on that identity. A fresh object per
// render would abort and restart the validation on every one of its own state
// updates (a request loop).
type NavigateOptions = { scroll?: boolean };

function navigate(url: string, replace: boolean, options?: NavigateOptions): void {
  if (typeof window === "undefined") return;
  if (replace) window.history.replaceState(null, "", url);
  else window.history.pushState(null, "", url);
  if (options?.scroll !== false) window.scrollTo(0, 0);
}

const ROUTER = {
  push: (url: string, options?: NavigateOptions) => navigate(url, false, options),
  replace: (url: string, options?: NavigateOptions) => navigate(url, true, options),
  back: () => window.history.back(),
  forward: () => window.history.forward(),
  refresh: () => window.location.reload(),
  prefetch: () => undefined,
};

export function useRouter() {
  return ROUTER;
}

let cachedSearch: string | null = null;
let cachedParams = new URLSearchParams();

export function useSearchParams(): URLSearchParams {
  if (typeof window === "undefined") return new URLSearchParams();
  const search = window.location.search;
  if (search !== cachedSearch) {
    cachedSearch = search;
    cachedParams = new URLSearchParams(search);
  }
  return cachedParams;
}

export function usePathname(): string {
  if (typeof window === "undefined") return "/";
  return window.location.pathname;
}
