// Prism tokenisation for the code view, kept free of React so it can run in the
// highlight worker (lib/highlight-worker.ts).
//
// react-syntax-highlighter renders Prism's hast tree as React elements on the
// main thread. For a 10,000-line file that is both a long tokenisation and a
// large element tree, so the viewer only asks for per-line HTML: the worker
// tokenises off the main thread and the result is plain, escaped markup.

import { refractor } from "refractor/all";
import { vs, vscDarkPlus } from "react-syntax-highlighter/dist/esm/styles/prism";

export type HighlightTheme = "light" | "dark";

type ThemeStyle = Record<string, string | number>;
type Theme = Record<string, ThemeStyle>;
type HastNode = {
  type: string;
  tagName?: string;
  value?: string;
  properties?: { className?: string[] };
  children?: HastNode[];
};

const THEMES: Record<HighlightTheme, Theme> = {
  light: vs as Theme,
  dark: vscDarkPlus as Theme,
};

interface Leaf {
  classNames: string[];
  text: string;
}

const styleCache = new Map<string, string>();
const classCombinationCache = new Map<string, string[]>();

// react-syntax-highlighter merges a style for every ordered combination of a
// token's non-token classes (create-element.js). Keyword/string/comment tokens
// only need the single-class entries, but keeping the same lookup means a theme
// selector such as `.language-json .token.boolean` is not silently reordered.
function classCombinations(classNames: string[]): string[] {
  if (classNames.length === 0 || classNames.length === 1) return classNames;
  const key = classNames.join(".");
  const cached = classCombinationCache.get(key);
  if (cached) return cached;

  const names = classNames.slice(0, 4);
  const out: string[] = [...names];
  for (let i = 0; i < names.length; i++) {
    for (let j = 0; j < names.length; j++) {
      if (i !== j) out.push(`${names[i]}.${names[j]}`);
    }
  }
  if (names.length >= 3) {
    for (let i = 0; i < names.length; i++) {
      for (let j = 0; j < names.length; j++) {
        for (let k = 0; k < names.length; k++) {
          if (i !== j && j !== k && i !== k) out.push(`${names[i]}.${names[j]}.${names[k]}`);
        }
      }
    }
  }
  if (names.length === 4) {
    const [a, b, c, d] = names;
    out.push(`${a}.${b}.${c}.${d}`, `${a}.${b}.${d}.${c}`, `${a}.${c}.${b}.${d}`, `${a}.${c}.${d}.${b}`);
    out.push(`${a}.${d}.${b}.${c}`, `${a}.${d}.${c}.${b}`, `${b}.${a}.${c}.${d}`, `${b}.${a}.${d}.${c}`);
    out.push(`${b}.${c}.${a}.${d}`, `${b}.${c}.${d}.${a}`, `${b}.${d}.${a}.${c}`, `${b}.${d}.${c}.${a}`);
    out.push(`${c}.${a}.${b}.${d}`, `${c}.${a}.${d}.${b}`, `${c}.${b}.${a}.${d}`, `${c}.${b}.${d}.${a}`);
    out.push(`${c}.${d}.${a}.${b}`, `${c}.${d}.${b}.${a}`, `${d}.${a}.${b}.${c}`, `${d}.${a}.${c}.${b}`);
    out.push(`${d}.${b}.${a}.${c}`, `${d}.${b}.${c}.${a}`, `${d}.${c}.${a}.${b}`, `${d}.${c}.${b}.${a}`);
  }
  classCombinationCache.set(key, out);
  return out;
}

function hyphenate(property: string): string {
  return property.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`);
}

function styleFor(classNames: string[], themeName: HighlightTheme): string {
  const key = `${themeName}\u0000${classNames.join(" ")}`;
  const cached = styleCache.get(key);
  if (cached !== undefined) return cached;

  const theme = THEMES[themeName];
  const style: ThemeStyle = {};
  for (const name of classCombinations(classNames.filter((name) => name !== "token"))) {
    const value = theme[name];
    if (value) Object.assign(style, value);
  }
  const css = Object.entries(style)
    .map(([property, value]) => `${hyphenate(property)}:${String(value)}`)
    .join(";");
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
export function highlightCodeLines(code: string, language: string, theme: HighlightTheme): string[] {
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
    const style = classNames.length ? styleFor(classNames, theme) : "";
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
export function highlightSegments(
  segments: readonly string[],
  language: string,
  theme: HighlightTheme,
): string[][] {
  return segments.map((segment) => highlightCodeLines(segment, language, theme));
}