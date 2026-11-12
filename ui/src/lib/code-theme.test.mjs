import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  CODE_THEME_DEFAULT,
  CODE_THEME_IDS,
  CODE_THEME_OPTIONS,
  CODE_THEME_STORAGE_KEY,
  codeThemeScheme,
  isCodeThemeId,
  parseCodeThemePreference,
} from "./code-theme.ts";
import { THEME_OPTIONS } from "./theme.ts";

const css = await readFile(new URL("../app/globals.css", import.meta.url), "utf8");
const indexHtml = await readFile(new URL("../../index.html", import.meta.url), "utf8");

const ROLE_VARS = ["keyword", "string", "number", "function", "type", "property", "tag"];
const PALETTE_VARS = ["bg", "fg", "gutter-bg", "comment", "meta", "punctuation", ...ROLE_VARS];

/** The custom-property declarations of the first rule whose selector matches. */
function declarations(selector) {
  const start = css.search(selector);
  assert.ok(start >= 0, `missing rule ${selector}`);
  const open = css.indexOf("{", start);
  const close = css.indexOf("}", open);
  const found = new Map();
  for (const line of css.slice(open + 1, close).split("\n")) {
    const match = line.match(/^\s*--([\w-]+):\s*([^;]+);/);
    if (match) found.set(match[1], match[2].trim());
  }
  return found;
}

function luminance(hex) {
  const channels = [1, 3, 5]
    .map((offset) => parseInt(hex.slice(offset, offset + 2), 16) / 255)
    .map((value) => (value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4));
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
}

function contrast(a, b) {
  const [lighter, darker] = [luminance(a), luminance(b)].sort((left, right) => right - left);
  return (lighter + 0.05) / (darker + 0.05);
}

test("a missing or unknown stored palette follows the appearance theme", () => {
  assert.equal(parseCodeThemePreference(null), "auto");
  assert.equal(parseCodeThemePreference(""), "auto");
  assert.equal(parseCodeThemePreference("pine"), "auto");
  assert.equal(parseCodeThemePreference("zenburn"), "zenburn");
  assert.equal(isCodeThemeId("contrast-dark"), true);
  assert.equal(isCodeThemeId("auto"), false);
  assert.equal(CODE_THEME_DEFAULT, "auto");
});

test("each palette reports its own brightness", () => {
  assert.equal(codeThemeScheme("auto"), null);
  assert.equal(codeThemeScheme("vscode-dark"), "dark");
  assert.equal(codeThemeScheme("solarized-light"), "light");
  for (const option of CODE_THEME_OPTIONS) {
    assert.equal(codeThemeScheme(option.id), option.scheme);
  }
});

test("first paint restores the code palette before the app renders", () => {
  // An unknown stored value is harmless: it matches no rule and the appearance
  // theme's roles apply, so the script can pass it through unvalidated.
  assert.match(indexHtml, new RegExp(`localStorage\\.getItem\\("${CODE_THEME_STORAGE_KEY}"\\)`));
  assert.match(indexHtml, /r\.dataset\.codeTheme ?= ?c/);
});

test("every appearance theme defines a readable colour for every code role", () => {
  for (const { id } of THEME_OPTIONS) {
    if (id === "auto") continue;
    const theme = declarations(new RegExp(`\\[data-theme="${id}"\\]`));
    for (const role of ROLE_VARS) {
      const colour = theme.get(`code-${role}`);
      assert.match(colour ?? "", /^#[0-9a-f]{6}$/, `${id} --code-${role}`);
      // Both surfaces code can sit on: the theme background and its panel.
      for (const surface of ["bg", "bg-panel"]) {
        const ratio = contrast(colour, theme.get(surface));
        assert.ok(ratio >= 4.5, `${id} --code-${role} on --${surface} is ${ratio.toFixed(2)}:1`);
      }
    }
  }
});

test("every code palette defines its own background and every role", () => {
  const paletteIds = new Set([...css.matchAll(/\[data-code-theme="([\w-]+)"\]/g)].map((match) => match[1]));
  assert.deepEqual([...paletteIds].sort(), [...CODE_THEME_IDS].sort());

  for (const id of CODE_THEME_IDS) {
    const palette = declarations(new RegExp(`\\[data-code-theme="${id}"\\]`));
    for (const name of PALETTE_VARS) {
      assert.match(palette.get(`code-${name}`) ?? "", /^#[0-9a-f]{6}$/, `${id} --code-${name}`);
    }
  }
});