import assert from "node:assert/strict";
import test from "node:test";
import { CODE_THEME_STYLE, codeRoleFor, codeStyleFor } from "./code-roles.ts";

test("the most specific class decides a token's role", () => {
  assert.equal(codeRoleFor(["token", "keyword"]), "keyword");
  assert.equal(codeRoleFor(["token", "string"]), "string");
  // Prism emits both for a function name; the class declared later in its
  // stylesheet wins, exactly as the stylesheet would.
  assert.equal(codeRoleFor(["token", "function", "class-name"]), "type");
  assert.equal(codeRoleFor(["token", "boolean"]), "number");
  // Classes from the language wrapper do not take part in the lookup.
  assert.equal(codeRoleFor(["token", "language-json", "property"]), "property");
  assert.equal(codeRoleFor(["token", "markdown-code-line-text"]), null);
  assert.equal(codeRoleFor([]), null);
});

test("a style names a variable, never a colour", () => {
  assert.equal(codeStyleFor(["token", "keyword"]), "color:var(--code-keyword)");
  assert.equal(codeStyleFor(["token", "punctuation", "bold"]), "color:var(--code-punctuation);font-weight:600");
  assert.equal(codeStyleFor(["token", "italic"]), "font-style:italic");
  assert.equal(codeStyleFor(["token"]), "");
});

test("the react-syntax-highlighter stylesheet covers the roles and the pre", () => {
  assert.equal(CODE_THEME_STYLE['pre[class*="language-"]'].background, undefined);
  assert.equal(CODE_THEME_STYLE['code[class*="language-"]'].color, "var(--code-fg)");
  for (const name of ["comment", "keyword", "string", "number", "function", "class-name", "attr-name", "tag", "punctuation", "important"]) {
    assert.match(CODE_THEME_STYLE[name]?.color ?? "", /^var\(--code-[\w-]+\)$/, name);
  }
});