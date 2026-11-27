import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { registerHooks } from "node:module";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createJiti } from "jiti";

registerHooks({
  load(url, context, nextLoad) {
    if (!url.endsWith(".module.css")) return nextLoad(url, context);
    return {
      format: "module",
      shortCircuit: true,
      source: "export default new Proxy({}, { get: (_, key) => String(key) });",
    };
  },
});

const jiti = createJiti(import.meta.url, {
  jsx: { runtime: "automatic" },
  tsconfigPaths: true,
});
const { I18nProvider } = await jiti.import("@/hooks/useI18n");
const { SubagentDialog } = await jiti.import("./SubagentDialog.tsx");

function render(props = {}) {
  return renderToStaticMarkup(
    React.createElement(
      I18nProvider,
      null,
      React.createElement(SubagentDialog, {
        sessionId: "child-1",
        label: "分析 Rust 后端功能",
        running: false,
        onClose() {},
        onOpenFile() {},
        ...props,
      }),
    ),
  );
}

test("a sub-agent transcript opens as a modal over the chat", () => {
  const html = render();
  assert.match(html, /data-subagent-dialog-backdrop=""/);
  assert.match(html, /class="subagent-dialog"/);
  assert.match(html, /role="dialog"/);
  assert.match(html, /aria-modal="true"/);
  assert.match(html, /aria-label="分析 Rust 后端功能"/);
  // The viewer keeps its own header, title and close control.
  assert.match(html, />分析 Rust 后端功能</);
  assert.match(html, /title="Close sub-agent transcript"/);
  assert.match(html, /No messages yet/);
});

test("the dialog is a phone's whole screen and stays below Settings", async () => {
  const css = await readFile(new URL("../app/settings.css", import.meta.url), "utf8");
  assert.match(css, /\.subagent-dialog-backdrop \{[^}]*z-index: 950;/);
  assert.match(css, /@media \(max-width: 640px\) \{[\s\S]*?\.subagent-dialog \{[^}]*height: 100dvh;/);
});
