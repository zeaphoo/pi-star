import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const { highlightCodeLines, highlightSegments } = await createJiti(import.meta.url).import("./highlight-core.ts");

// Strip the token markup and undo the escaping, so a line compares as the
// source text a copy from the page yields.
const plain = (html) => html
  .replace(/<[^>]+>/g, "")
  .replace(/&quot;/g, '"')
  .replace(/&#39;/g, "'")
  .replace(/&lt;/g, "<")
  .replace(/&gt;/g, ">")
  .replace(/&amp;/g, "&");

test("one HTML string per source line, tokens escaped", () => {
  const lines = highlightCodeLines("package main\n\nimport \"fmt\"\n", "go", "dark");
  assert.equal(lines.length, 4, "a trailing newline ends one last empty line");
  assert.match(lines[0], /^<span class="token keyword" style="color:#569CD6">package<\/span> main$/);
  assert.equal(lines[1], "");
  // Quotes and angle brackets in code never reach the page as markup.
  assert.match(lines[2], /&quot;fmt&quot;/);
  assert.equal(plain(lines[2]), 'import "fmt"');
});

test("a token that spans lines keeps its class on every line", () => {
  const lines = highlightCodeLines("/* one\n   two */\nlet a = 1;", "rust", "dark");
  assert.equal(lines.length, 3);
  assert.match(lines[0], /token comment/);
  assert.match(lines[1], /token comment/);
  assert.equal(plain(lines[1]), "   two */");
  assert.match(lines[2], /token keyword/);
});

test("unknown languages and empty input render escaped plain text", () => {
  assert.deepEqual(highlightCodeLines("a < b", "not-a-language", "light"), ["a &lt; b"]);
  assert.deepEqual(highlightCodeLines("a < b", "text", "light"), ["a &lt; b"]);
  assert.deepEqual(highlightCodeLines("", "go", "dark"), [""]);
  // Escaping also covers the characters that could close an attribute.
  assert.deepEqual(highlightCodeLines("<a href=\"x\">&</a>", "text", "dark"), ["&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;"]);
});

test("the light and dark themes colour the same token differently", () => {
  const dark = highlightCodeLines("fn main() {}", "rust", "dark")[0];
  const light = highlightCodeLines("fn main() {}", "rust", "light")[0];
  assert.match(dark, /color:#569CD6/);
  assert.match(light, /color:#0000ff/);
});

test("segments stay independent, so one hunk cannot leak state into the next", () => {
  // Each segment is tokenised from a fresh Prism state, as a diff hunk needs:
  // the segment before ends inside an unterminated block comment, and the next
  // one still tokenises from a clean state.
  const [first, second] = highlightSegments(["/* still open", "let a = 1;"], "rust", "dark");
  assert.equal(plain(first[0]), "/* still open");
  assert.match(second[0], /token keyword/);
  assert.equal(plain(second[0]), "let a = 1;");
});
