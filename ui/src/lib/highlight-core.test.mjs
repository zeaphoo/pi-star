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
  const lines = highlightCodeLines("package main\n\nimport \"fmt\"\n", "go");
  assert.equal(lines.length, 4, "a trailing newline ends one last empty line");
  assert.match(lines[0], /^<span class="token keyword" style="color:var\(--code-keyword\)">package<\/span> main$/);
  assert.equal(lines[1], "");
  // Quotes and angle brackets in code never reach the page as markup.
  assert.match(lines[2], /&quot;fmt&quot;/);
  assert.equal(plain(lines[2]), 'import "fmt"');
});

test("a token that spans lines keeps its class on every line", () => {
  const lines = highlightCodeLines("/* one\n   two */\nlet a = 1;", "rust");
  assert.equal(lines.length, 3);
  assert.match(lines[0], /token comment/);
  assert.match(lines[1], /token comment/);
  assert.equal(plain(lines[1]), "   two */");
  assert.match(lines[2], /token keyword/);
});

test("unknown languages and empty input render escaped plain text", () => {
  assert.deepEqual(highlightCodeLines("a < b", "not-a-language"), ["a &lt; b"]);
  assert.deepEqual(highlightCodeLines("a < b", "text"), ["a &lt; b"]);
  assert.deepEqual(highlightCodeLines("", "go"), [""]);
  // Escaping also covers the characters that could close an attribute.
  assert.deepEqual(highlightCodeLines("<a href=\"x\">&</a>", "text"), ["&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;"]);
});

test("tokens carry role variables, not palette colours", () => {
  // The same markup serves every theme: a palette only changes what the CSS
  // variables resolve to, so a theme switch never re-tokenises a file.
  const keyword = highlightCodeLines("fn main() {}", "rust")[0];
  assert.match(keyword, /color:var\(--code-keyword\)/);
  assert.doesNotMatch(keyword, /#[0-9a-fA-F]{6}/);

  // A function name is `function`, a class name is `type`: each role its own variable.
  const typed = highlightCodeLines("let x = bar(); class Foo {}", "typescript")[0];
  assert.match(typed, /color:var\(--code-function\)/);
  assert.match(typed, /color:var\(--code-type\)/);
  assert.match(highlightCodeLines("let x = 1;", "typescript")[0], /color:var\(--code-number\)/);
});

test("segments stay independent, so one hunk cannot leak state into the next", () => {
  // Each segment is tokenised from a fresh Prism state, as a diff hunk needs:
  // the segment before ends inside an unterminated block comment, and the next
  // one still tokenises from a clean state.
  const [first, second] = highlightSegments(["/* still open", "let a = 1;"], "rust");
  assert.equal(plain(first[0]), "/* still open");
  assert.match(second[0], /token keyword/);
  assert.equal(plain(second[0]), "let a = 1;");
});
