// Prism tokenisation for the code view, kept free of React so it can run in the
// highlight worker (lib/highlight-worker.ts).
//
// react-syntax-highlighter renders Prism's hast tree as React elements on the
// main thread. For a 10,000-line file that is both a long tokenisation and a
// large element tree, so the viewer only asks for per-line HTML: the worker
// tokenises off the main thread and the result is plain, escaped markup.
//
// Tokens are coloured through the CSS variables in lib/code-roles.ts, not
// through a Prism theme object, so the markup does not depend on the palette:
// a theme switch re-resolves the variables and never re-tokenises a file.

import { refractor } from "refractor/all";
import { codeStyleFor } from "./code-roles";

type HastNode = {
  type: string;
  tagName?: string;
  value?: string;
  properties?: { className?: string[] };
  children?: HastNode[];
};

interface Leaf {
  classNames: string[];
  text: string;
}

const styleCache = new Map<string, string>();

// Styles depend only on which classes a token carries, so the same handful of
// class lists (`token keyword`, `token string …`) recur across a whole file.
function styleFor(classNames: string[]): string {
  const key = classNames.join(" ");
  const cached = styleCache.get(key);
  if (cached !== undefined) return cached;
  const css = codeStyleFor(classNames);
  styleCache.set(key, css);
  return css;
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function flatten(node: HastNode | undefined, classNames: string[], out: Leaf[]): void {
  if (!node) return;
  if (node.type === "text") {
    if (node.value) out.push({ classNames, text: node.value });
    return;
  }
  if (node.type !== "element") {
    // The root (and any non-element wrapper) only contributes its children.
    for (const child of node.children ?? []) flatten(child, classNames, out);
    return;
  }
  const ownClassNames = Array.isArray(node.properties?.className) ? node.properties.className : [];
  const nextClassNames = ownClassNames.length ? classNames.concat(ownClassNames) : classNames;
  for (const child of node.children ?? []) flatten(child, nextClassNames, out);
}

/** Escaped, unhighlighted lines. */
function plainLines(code: string): string[] {
  return code.split("\n").map(escapeHtml);
}

/**
 * Highlight one piece of code and return one HTML string per source line.
 *
 * Every piece is tokenised with a fresh Prism state. Callers that split a file
 * into independent hunks must pass each hunk as its own segment, as px0 does:
 * a block comment that opens in one hunk may not continue in the next.
 */
export function highlightCodeLines(code: string, language: string): string[] {
  if (!language || language === "text" || language === "plaintext" || !refractor.registered(language)) {
    return plainLines(code);
  }

  let tree: HastNode;
  try {
    tree = refractor.highlight(code, language) as HastNode;
  } catch {
    return plainLines(code);
  }

  const leaves: Leaf[] = [];
  flatten(tree, [], leaves);

  const lines: string[] = [""];
  let lineIndex = 0;
  for (const leaf of leaves) {
    const classNames = leaf.classNames;
    const classAttribute = classNames.length
      ? ` class="${escapeHtml(classNames.join(" "))}"`
      : "";
    const style = classNames.length ? styleFor(classNames) : "";
    const styleAttribute = style ? ` style="${escapeHtml(style)}"` : "";
    const parts = leaf.text.split("\n");
    for (let index = 0; index < parts.length; index++) {
      const part = parts[index];
      if (part) {
        lines[lineIndex] += classNames.length || style
          ? `<span${classAttribute}${styleAttribute}>${escapeHtml(part)}</span>`
          : escapeHtml(part);
      }
      if (index < parts.length - 1) {
        lines.push("");
        lineIndex++;
      }
    }
  }
  return lines;
}

/** Highlight several independent pieces, preserving segment order. */
export function highlightSegments(segments: readonly string[], language: string): string[][] {
  return segments.map((segment) => highlightCodeLines(segment, language));
}