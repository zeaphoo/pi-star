import assert from "node:assert/strict";
import { readFile, stat } from "node:fs/promises";
import test from "node:test";

// The mono faces are bundled rather than looked up on the system, so the checks
// below keep each file, its @font-face, the default stack, the preload that
// starts the default face early and the licences in step.

const css = await readFile(new URL("../app/globals.css", import.meta.url), "utf8");
const indexHtml = await readFile(new URL("../../index.html", import.meta.url), "utf8");
const font = (name) => new URL(`../../public/fonts/${name}`, import.meta.url);

const FILES = [
  { name: "jetbrains-mono-var.woff2", min: 20_000, max: 400_000 },
  { name: "noto-sans-mono-var.woff2", min: 300_000, max: 700_000 },
  { name: "noto-sans-mono-cjk-sc-regular.woff2", min: 8_000_000, max: 14_000_000 },
  { name: "jetbrains-mono-nerd-regular.woff2", min: 700_000, max: 1_300_000 },
  { name: "jetbrains-mono-nerd-bold.woff2", min: 700_000, max: 1_300_000 },
  { name: "hack-nerd-regular.woff2", min: 800_000, max: 1_400_000 },
  { name: "hack-nerd-bold.woff2", min: 800_000, max: 1_400_000 },
];

test("every bundled face is a WOFF2 file with a licence beside it", async () => {
  for (const entry of FILES) {
    const info = await stat(font(entry.name));
    assert.ok(info.size > entry.min && info.size < entry.max, `${entry.name}: unexpected size ${info.size}`);
    const header = (await readFile(font(entry.name))).subarray(0, 4).toString("latin1");
    assert.equal(header, "wOF2", entry.name);
  }
  assert.match(await readFile(font("OFL.txt"), "utf8"), /SIL Open Font License/);
  assert.match(await readFile(font("HACK-LICENSE.txt"), "utf8"), /MIT License/);
  assert.match(await readFile(font("NERD-FONTS-LICENSE.txt"), "utf8"), /Nerd Fonts Licensing/);
});

test("each face is declared with its own family and weight", () => {
  const faces = [
    { family: "JetBrains Mono", url: "/fonts/jetbrains-mono-var.woff2", weight: "100 800" },
    { family: "Noto Sans Mono", url: "/fonts/noto-sans-mono-var.woff2", weight: "100 900" },
    { family: "Noto Sans Mono CJK SC", url: "/fonts/noto-sans-mono-cjk-sc-regular.woff2", weight: "400" },
    { family: "JetBrainsMono Nerd Font", url: "/fonts/jetbrains-mono-nerd-regular.woff2", weight: "400" },
    { family: "JetBrainsMono Nerd Font", url: "/fonts/jetbrains-mono-nerd-bold.woff2", weight: "700" },
    { family: "Hack Nerd Font", url: "/fonts/hack-nerd-regular.woff2", weight: "400" },
    { family: "Hack Nerd Font", url: "/fonts/hack-nerd-bold.woff2", weight: "700" },
  ];
  for (const face of faces) {
    const pattern = new RegExp(`@font-face \\{[^}]*url\\("${face.url.replaceAll("/", "\\/")}"\\)[^}]*\\}`);
    const match = css.match(pattern)?.[0];
    assert.ok(match, `${face.family} ${face.url} must have an @font-face`);
    assert.match(match, new RegExp(`font-family: "${face.family}";`));
    assert.match(match, new RegExp(`font-weight: ${face.weight};`));
    assert.match(match, /font-display: swap;/);
  }
  const cjk = css.match(/@font-face \{[^}]*font-family: "Noto Sans Mono CJK SC";[^}]*\}/)?.[0] ?? "";
  assert.match(cjk, /unicode-range:/);
  assert.match(cjk, /U\+4E00-9FFF/);
});

test("the code font is the bundled face, with icons and CJK behind it", () => {
  const stack = css.match(/--font-mono-default: ([^;]+);/)?.[1] ?? "";
  assert.ok(stack.startsWith("'JetBrains Mono',"), stack);
  // Everything after the bundled face is a fallback, icons and CJK included.
  for (const fallback of ["'JetBrainsMono Nerd Font'", "'Noto Sans Mono'", "'Noto Sans Mono CJK SC'", "'Consolas'", "ui-monospace", "'PingFang SC'"]) {
    assert.ok(stack.includes(fallback), `${fallback} must stay in the stack`);
  }
});

test("first paint starts the default font download before the app boots", () => {
  assert.match(indexHtml, /<link rel="preload" href="\/fonts\/jetbrains-mono-var\.woff2" as="font" type="font\/woff2" crossorigin \/>/);
  assert.doesNotMatch(indexHtml, /preload[^>]*noto-sans-mono-cjk/i);
});
