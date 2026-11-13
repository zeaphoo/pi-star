import assert from "node:assert/strict";
import test from "node:test";
import {
  parseTerminalThemePreference,
  resolveTerminalTheme,
  TERMINAL_THEME_PRESETS,
  terminalThemePreset,
} from "./terminal-theme.ts";

test("old dark/light values migrate to the Pi presets", () => {
  assert.equal(parseTerminalThemePreference("light"), "pi-light");
  assert.equal(parseTerminalThemePreference("dark"), "pi-dark");
  assert.equal(parseTerminalThemePreference("auto"), "auto");
  assert.equal(parseTerminalThemePreference("dracula"), "dracula");
  assert.equal(parseTerminalThemePreference(null), "pi-dark");
  assert.equal(parseTerminalThemePreference("pine"), "pi-dark");
});

test("auto follows the app theme, explicit presets override it", () => {
  assert.equal(resolveTerminalTheme("auto", true).id, "pi-dark");
  assert.equal(resolveTerminalTheme("auto", false).id, "pi-light");
  assert.equal(resolveTerminalTheme("dracula", false).id, "dracula");
  assert.equal(resolveTerminalTheme("pi-light", true).id, "pi-light");
  assert.equal(resolveTerminalTheme("missing", true).id, "pi-dark");
});

test("every preset defines all ANSI colors, its chrome and a kind", () => {
  const names = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
  assert.ok(TERMINAL_THEME_PRESETS.length >= 10);
  for (const preset of TERMINAL_THEME_PRESETS) {
    assert.equal(preset.kind === "dark" || preset.kind === "light", true, preset.id);
    assert.match(preset.xterm.background ?? "", /^#[0-9a-f]{6}$/);
    assert.match(preset.xterm.foreground ?? "", /^#[0-9a-f]{6}$/);
    assert.match(preset.xterm.cursor ?? "", /^#[0-9a-f]{6}$/);
    assert.match(preset.xterm.selectionBackground ?? "", /^#[0-9a-f]{6}$/);
    for (const name of names) {
      assert.match(preset.xterm[name], /^#[0-9a-f]{6}$/, `${preset.id} ${name}`);
      assert.match(preset.xterm[`bright${name[0].toUpperCase()}${name.slice(1)}`], /^#[0-9a-f]{6}$/, `${preset.id} bright ${name}`);
    }
    for (const value of Object.values(preset.chrome)) {
      assert.match(value, /^(#[0-9a-f]{6}|\d+, \d+, \d+)$/, `${preset.id} chrome`);
    }
  }
});

test("preset lookup falls back to Pi Dark", () => {
  assert.equal(terminalThemePreset("nord").name, "Nord");
  assert.equal(terminalThemePreset("missing").id, "pi-dark");
});
