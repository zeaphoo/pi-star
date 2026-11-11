"use client";

import { useEffect, useState } from "react";
import { cachedHighlight, highlightSegments } from "@/lib/highlight-client";
import type { HighlightTheme } from "@/lib/highlight-core";

interface Result {
  segments: readonly string[];
  language: string;
  theme: HighlightTheme;
  lines: string[][];
}

/**
 * Highlight independent code segments in the worker (lib/highlight-worker.ts).
 *
 * `segments` must be memoised by the caller. Returns null until the result for
 * the current input is ready; the cached result is returned synchronously when
 * the same file is opened again.
 */
export function useHighlightedSegments(
  segments: readonly string[],
  language: string,
  isDark: boolean,
  enabled = true,
): string[][] | null {
  const theme: HighlightTheme = isDark ? "dark" : "light";
  const cached = enabled ? cachedHighlight(segments, language, theme) : null;
  const [result, setResult] = useState<Result | null>(() => {
    const initial = enabled ? cachedHighlight(segments, language, theme) : null;
    return initial ? { segments, language, theme, lines: initial } : null;
  });

  const current = cached
    ?? (result && result.segments === segments && result.language === language && result.theme === theme
      ? result.lines
      : null);

  useEffect(() => {
    if (!enabled || current) return;
    let active = true;
    highlightSegments(segments, language, theme).then(
      (lines) => {
        if (active) setResult({ segments, language, theme, lines });
      },
      () => {
        // The viewer keeps its plain rows; the next mount retries.
      },
    );
    return () => { active = false; };
  }, [current, enabled, language, segments, theme]);

  return current;
}