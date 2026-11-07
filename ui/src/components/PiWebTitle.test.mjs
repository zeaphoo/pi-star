import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url, { jsx: { runtime: "automatic" }, tsconfigPaths: true });
const React = await jiti.import("react");
const { renderToStaticMarkup } = await jiti.import("react-dom/server");
const { BRAND_TEXT, PiWebTitle } = await jiti.import("./PiWebTitle.tsx");
const { SIDEBAR_MIN_WIDTH } = await jiti.import("@/lib/panel-layout.ts");

const css = await readFile(new URL("../app/sidebar.css", import.meta.url), "utf8");

/** A rule's declarations (its first rule for the selector). */
function rule(selector) {
  const start = css.indexOf(`\n${selector} {`);
  assert.notEqual(start, -1, `${selector} not found`);
  return css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
}

test("the brand is the logo and its name, with nothing to click", () => {
  const html = renderToStaticMarkup(React.createElement(PiWebTitle));
  assert.match(
    html,
    /^<span class="sidebar-brand"><svg class="sidebar-brand-logo" viewBox="0 0 512 512" aria-hidden="true">.*<\/svg>Pi Star<\/span>$/,
  );
  assert.equal(BRAND_TEXT, "Pi Star");
  assert.doesNotMatch(html, /<button|is-version/, "the versions are in Settings › General, not behind a click");
  assert.doesNotMatch(html, /<link|preload/, "the inline mark needs no image preload");
  assert.doesNotMatch(html, /style=/, "no inline styles: its look is app/sidebar.css's");
});

test("the brand's text sits in the chevron column and is never cut at the sidebar's minimum", () => {
  const brand = rule(".sidebar-brand");
  // Its text in the chevron column: the tree's 6px inset and a toggle's 5px.
  const [marginLeft] = brand.match(/margin: 0 0 0 (\d+)px;/).slice(1).map(Number);
  const [paddingLeft] = brand.match(/padding: \d+px (\d+)px;/).slice(1).map(Number);
  assert.equal(marginLeft + paddingLeft, 6 + 5);

  // At the sidebar's minimum (its line off), beside New's + and the search
  // (36px cells each), "Pi Star" fits in its 15px code type (0.6em a
  // character), and in the 11px type the narrowest header drops it to.
  assert.match(brand, /(?:^|\s)font-family: var\(--font-mono\);/);
  assert.match(brand, /(?:^|\s)font-size: 15px;/);
  assert.match(css, /\.sidebar-header\[data-fit="2"\] \.sidebar-brand \{\s*font-size: 11px;\s*\}/);
  assert.doesNotMatch(css, /\.sidebar-brand\.is-version/, "the version state is gone");
  assert.match(css, /\.sidebar-brand-logo \{[^}]*width: 16px;[^}]*height: 16px;/);
  const room = SIDEBAR_MIN_WIDTH - 1 - 36 - 36;
  const logo = 16 + 6;
  const box = (text, fontSize) => marginLeft + 2 * paddingLeft + logo + text.length * 0.6 * fontSize;
  assert.ok(box(BRAND_TEXT, 15) <= room, "the brand and logo at 15px");
  assert.ok(box(BRAND_TEXT, 11) <= room, "the brand at the narrowest header");
});
