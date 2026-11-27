import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url, { jsx: { runtime: "automatic" }, tsconfigPaths: true });
const React = await jiti.import("react");
const { renderToStaticMarkup } = await jiti.import("react-dom/server");
const { I18nProvider } = await jiti.import("@/hooks/useI18n");
const { SessionContextPanel } = await jiti.import("./SessionContextPanel.tsx");

const h = React.createElement;
const noop = () => {};

function render(props = {}) {
  return renderToStaticMarkup(h(I18nProvider, null, h(SessionContextPanel, {
    sessionId: "session-1",
    prompt: "You are Pi.",
    tools: [{ name: "read", description: "Read a file", active: true }],
    loading: false,
    onLoad: noop,
    ...props,
  })));
}

test("shows the selected session's prompt and tools as two tabs", () => {
  const html = render();
  assert.match(html, /role="tablist"/);
  assert.match(html, /role="tab" aria-selected="true"[^>]*>System prompt<\/button>/);
  assert.match(html, /role="tab" aria-selected="false"[^>]*>Tool definitions<\/button>/);
  assert.match(html, /You are Pi\./);
  assert.match(html, />Refresh<\/button>/);
});
