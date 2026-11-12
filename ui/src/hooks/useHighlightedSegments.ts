"use client";

import { useEffect, useState } from "react";
import { cachedHighlight, highlightSegments } from "@/lib/highlight-client";

interface Result {
  segments: readonly string[];
  language: string;
  lines: string[][];
}

/**
 * Highlight independent code segments in the worker (lib/highlight-worker.ts).
 *
 * `segments` must be memoised by the caller. Returns null until the result for
 * the current input is ready; the cached result is returned synchronously when
 * the same file is opened again.
 *
 * The result does not depend on the palette: tokens carry CSS variables
 * (lib/code-roles.ts), so switching the appearance or code theme never
 * re-tokenises a file.
 */
export function useHighlightedSegments(
  segments: readonly string[],
  language: string,
  enabled = true,
): string[][] | null {
  const cached = enabled ? cachedHighlight(segments, language) : null;
  const [result, setResult] = useState<Result | null>(() => {
    const initial = enabled ? cachedHighlight(segments, language) : null;
    return initial ? { segments, language, lines: initial } : null;
  });

  const current = cached
    ?? (result && result.segments === segments && result.language === language
      ? result.lines
      : null);

  useEffect(() => {
    if (!enabled || current) return;
    let active = true;
    highlightSegments(segments, language).then(
      (lines) => {
        if (active) setResult({ segments, language, lines });
      },
      () => {
        // The viewer keeps its plain rows; the next mount retries.
      },
    );
    return () => { active = false; };
  }, [current, enabled, language, segments]);

  return current;
}