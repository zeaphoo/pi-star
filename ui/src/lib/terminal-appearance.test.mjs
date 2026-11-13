import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url);
const {
  DEFAULT_TERMINAL_APPEARANCE,
  resolveTerminalFontSize,
  resolveTerminalFontWeight,
  sanitizeTerminalAppearance,
  TERMINAL_FONT_SIZE_MAX,
  TERMINAL_FONT_SIZE_MIN,
  TERMINAL_SCROLLBACK_MAX,
  terminalFontStack,
} = await jiti.import("./terminal-appearance.ts");

test("missing appearance keeps the documented defaults", () => {
  assert.deepEqual(sanitizeTerminalAppearance(null), DEFAULT_TERMINAL_APPEARANCE);
  assert.deepEqual(sanitizeTerminalAppearance([]), DEFAULT_TERMINAL_APPEARANCE);
  assert.deepEqual(sanitizeTerminalAppearance({}), DEFAULT_TERMINAL_APPEARANCE);
});

test("stored numbers are clamped to the terminal's own ranges", () => {
  const appearance = sanitizeTerminalAppearance({
    font: "hack-nerd",
    customFont: "Fira Code, monospace",
    fontWeight: "500",
    fontSize: 999,
    lineHeight: 0,
    letterSpacing: 9,
    cursorStyle: "bar",
    cursorBlink: false,
    minimumContrastRatio: 100,
    scrollback: 1_000_000,
  });
  assert.equal(appearance.font, "hack-nerd");
  assert.equal(appearance.customFont, "Fira Code, monospace");
  assert.equal(appearance.fontWeight, 500);
  assert.equal(appearance.fontSize, TERMINAL_FONT_SIZE_MAX);
  assert.equal(appearance.lineHeight, 1);
  assert.equal(appearance.letterSpacing, 2);
  assert.equal(appearance.cursorStyle, "bar");
  assert.equal(appearance.cursorBlink, false);
  assert.equal(appearance.minimumContrastRatio, 21);
  assert.equal(appearance.scrollback, TERMINAL_SCROLLBACK_MAX);

  assert.equal(sanitizeTerminalAppearance({ fontSize: 1 }).fontSize, TERMINAL_FONT_SIZE_MIN);
  assert.equal(sanitizeTerminalAppearance({ font: "comic-sans" }).font, "inherit");
  assert.equal(sanitizeTerminalAppearance({ cursorStyle: "beam" }).cursorStyle, "block");
});

test("null size and weight mean follow the editor", () => {
  assert.equal(resolveTerminalFontSize(null, 13), 13);
  assert.equal(resolveTerminalFontSize(17, 13), 17);
  assert.equal(resolveTerminalFontWeight(null, 400), 400);
  assert.equal(resolveTerminalFontWeight(600, 400), 600);
});

test("every terminal font stack keeps a bundled face, icons and CJK behind it", () => {
  for (const font of ["jetbrains-mono", "jetbrains-mono-nerd", "hack-nerd", "noto-sans-mono"]) {
    const stack = terminalFontStack(font, "");
    assert.ok(stack, font);
    assert.match(stack, /Noto Sans Mono/);
    assert.match(stack, /Noto Sans Mono CJK SC/);
  }
  assert.match(terminalFontStack("jetbrains-mono", ""), /'JetBrainsMono Nerd Font'/);
  assert.match(terminalFontStack("hack-nerd", ""), /'Hack Nerd Font'/);
  assert.match(terminalFontStack("noto-sans-mono", ""), /'JetBrainsMono Nerd Font'/);
  assert.match(terminalFontStack("custom", '"Fira Code", monospace'), /"Fira Code", monospace, 'Noto Sans Mono'/);
  assert.equal(terminalFontStack("inherit", ""), null);
  assert.equal(terminalFontStack("custom", ""), null);
});
