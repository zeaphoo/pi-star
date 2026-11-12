import { highlightSegments } from "./highlight-core";

interface HighlightRequest {
  id: number;
  segments: string[];
  language: string;
}

interface HighlightResponse {
  id: number;
  lines?: string[][];
  error?: string;
}

const scope = self as unknown as {
  onmessage: ((event: MessageEvent<HighlightRequest>) => void) | null;
  postMessage: (message: HighlightResponse) => void;
};

scope.onmessage = (event) => {
  const { id, segments, language } = event.data;
  try {
    scope.postMessage({ id, lines: highlightSegments(segments, language) });
  } catch (error) {
    scope.postMessage({
      id,
      error: error instanceof Error ? error.message : String(error),
    });
  }
};