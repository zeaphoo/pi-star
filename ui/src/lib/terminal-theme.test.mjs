import assert from "node:assert/strict";
import test from "node:test";
import { parseTerminalThemePreference, resolveTerminalTheme, TERMINAL_XTERM_THEMES } from "./terminal-theme.ts";

test("a missing or unknown stored terminal theme falls back to dark", () => {
  assert.equal(parseTerminalThemePreference("light"), "light");
  assert.equal(parseTerminalThemePreference("dark"), "dark");
  assert.equal(parseTerminalThemePreference("auto"), "auto");
  assert.equal(parseTerminalThemePreference(null), "dark");
  assert.equal(parseTerminalThemePreference("pine"), "dark");
});

test("auto follows the app theme, explicit choices override it", () => {
  assert.equal(resolveTerminalTheme("auto", true), "dark");
  assert.equal(resolveTerminalTheme("auto", false), "light");
  assert.equal(resolveTerminalTheme("dark", false), "dark");
  assert.equal(resolveTerminalTheme("light", true), "light");
});

test("both terminal palettes define every ANSI color", () => {
  const names = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
  for (const theme of Object.values(TERMINAL_XTERM_THEMES)) {
    for (const name of names) {
      assert.match(theme[name], /^#[0-9a-f]{6}$/);
      assert.match(theme[`bright${name[0].toUpperCase()}${name.slice(1)}`], /^#[0-9a-f]{6}$/);
    }
  }
});
