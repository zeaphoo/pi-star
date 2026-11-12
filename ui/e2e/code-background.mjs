import assert from "node:assert/strict";

/** The fixture has one ordinary fence and one Mermaid diagram. */
export async function checkCodeBackground(page) {
  const warnings = [];
  const onConsole = (message) => {
    if (/conflicting property|style property during rerender/.test(message.text())) {
      warnings.push(message.text());
    }
  };
  page.on("console", onConsole);
  const source = page.getByRole("button", { name: "Source", exact: true });
  await source.click();
  const blocks = page.locator(".markdown-code-block pre");
  await blocks.nth(1).waitFor();
  try {
    for (const colorScheme of ["light", "dark", "light", "dark", "light"]) {
      await page.emulateMedia({ colorScheme });
      await page.waitForFunction((theme) => document.documentElement.dataset.theme === theme, colorScheme);
      await page.waitForFunction(() => {
        const probe = document.createElement("div");
        probe.style.backgroundColor = "color-mix(in srgb, var(--bg) 92%, var(--bg-panel))";
        document.body.append(probe);
        const expected = getComputedStyle(probe).backgroundColor;
        probe.remove();
        const nodes = [...document.querySelectorAll(".markdown-code-block pre")];
        return nodes.length === 2 && nodes.every((node) => (
          getComputedStyle(node).backgroundColor === expected
          && getComputedStyle(node).borderTopWidth === "0px"
          && getComputedStyle(node.querySelector("code")).backgroundColor === "rgba(0, 0, 0, 0)"
        ));
      });
    }
    assert.deepEqual(warnings, [], "Theme switches must not mix background shorthand and backgroundColor");

    // A palette chosen in Settings replaces the appearance theme's code
    // colours — background too — before first paint, with the same markup:
    // only the CSS variables the tokens name resolve differently.
    const themed = await page.context().newPage();
    try {
      const url = page.url();
      await themed.addInitScript(() => {
        try { localStorage.setItem("pi-code-theme", "github-dark"); } catch {}
      });
      await themed.goto(url, { waitUntil: "domcontentloaded" });
      await themed.locator(".markdown-code-block pre").first().waitFor();
      await themed.waitForFunction(() => document.documentElement.dataset.codeTheme === "github-dark");
      const applied = await themed.locator(".markdown-code-block pre").first().evaluate((pre) => ({
        background: getComputedStyle(pre).backgroundColor,
        keyword: getComputedStyle(pre.querySelector(".token")).color,
      }));
      assert.equal(applied.background, "rgb(13, 17, 23)", "GitHub Dark's own code background");
      assert.equal(applied.keyword, "rgb(255, 123, 114)", "GitHub Dark's keyword colour");
    } finally {
      await themed.evaluate(() => localStorage.removeItem("pi-code-theme")).catch(() => {});
      await themed.close();
    }
  } finally {
    page.off("console", onConsole);
    await page.emulateMedia({ colorScheme: "light" });
  }
  console.log("PASS: code and Mermaid source backgrounds survive repeated theme switches and a code palette");
}
