import assert from "node:assert/strict";
import { readFile, stat } from "node:fs/promises";
import test from "node:test";

// The mono face is bundled rather than looked up on the system, so the checks
// below keep the file, the @font-face that loads it, the default stack that
// names it, the preload that starts it early and its licence in step.

const fontUrl = new URL("../../public/fonts/jetbrains-mono-var.woff2", import.meta.url);
const licenceUrl = new URL("../../public/fonts/OFL.txt", import.meta.url);
const css = await readFile(new URL("../app/globals.css", import.meta.url), "utf8");
const indexHtml = await readFile(new URL("../../index.html", import.meta.url), "utf8");

test("the bundled mono font is a WOFF2 file with a licence beside it", async () => {
  const info = await stat(fontUrl);
  assert.ok(info.size > 20_000 && info.size < 400_000, `unexpected size ${info.size}`);
  const header = (await readFile(fontUrl)).subarray(0, 4).toString("latin1");
  assert.equal(header, "wOF2");
  assert.match(await readFile(licenceUrl, "utf8"), /SIL Open Font License/);
});

test("the code font is the bundled face, with the system stack behind it", () => {
  const face = css.match(/@font-face \{[^}]*font-family: "JetBrains Mono";[^}]*\}/)?.[0];
  assert.ok(face, "a @font-face must declare JetBrains Mono");
  assert.match(face, /url\("\/fonts\/jetbrains-mono-var\.woff2"\)/);
  assert.match(face, /font-weight: 100 800;/);
  assert.match(face, /font-display: swap;/);

  const stack = css.match(/--font-mono-default: ([^;]+);/)?.[1] ?? "";
  assert.ok(stack.startsWith("'JetBrains Mono',"), stack);
  // Everything after the bundled face is a fallback, CJK included.
  for (const fallback of ["'Noto Sans Mono'", "'Consolas'", "ui-monospace", "'PingFang SC'"]) {
    assert.ok(stack.includes(fallback), `${fallback} must stay in the stack`);
  }
});

test("first paint starts the font download before the app boots", () => {
  assert.match(indexHtml, /<link rel="preload" href="\/fonts\/jetbrains-mono-var\.woff2" as="font" type="font\/woff2" crossorigin \/>/);
});