import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url, { jsx: { runtime: "automatic" }, tsconfigPaths: true });
const React = await jiti.import("react");
const { renderToStaticMarkup } = await jiti.import("react-dom/server");
const { I18nProvider } = await jiti.import("@/hooks/useI18n");
const { EMPTY_ARTIFACT_PANE, SessionArtifactsPane } = await jiti.import("./SessionArtifactsPane.tsx");

const h = React.createElement;
const noop = () => {};

function render(props = {}) {
  return renderToStaticMarkup(h(I18nProvider, null, h(SessionArtifactsPane, {
    sessionId: "s1",
    cwd: "/work/app",
    artifacts: [],
    scanning: false,
    pane: EMPTY_ARTIFACT_PANE,
    onChange: noop,
    onOpenFile: noop,
    onOpenInCode: noop,
    onOpenCodeView: noop,
    onViewerStateChange: noop,
    onClose: noop,
    ...props,
  })));
}

test("the artifacts pane lists what the session wrote, with its count and path", () => {
  const html = render({
    artifacts: [
      { filePath: "/work/app/src/a.ts", relativePath: "src/a.ts", lastWriteAt: Date.now(), writes: 2 },
      { filePath: "/work/app/docs/spec.md", relativePath: "docs/spec.md", writes: 1 },
    ],
  });
  assert.match(html, /id="session-artifacts"/);
  assert.match(html, /class="artifacts-count"[^>]*>2</);
  assert.match(html, />a\.ts</);
  assert.match(html, />src\/a\.ts</);
  assert.match(html, />spec\.md</);
  assert.match(html, />docs\/spec\.md</);
  // The first tab is the artifacts list, marked as the current view.
  assert.match(html, /class="artifacts-home is-active"/);
});

test("an empty session says so, and a scan in progress says that", () => {
  const empty = render();
  assert.match(empty, /No artifacts in this session/);
  assert.match(empty, /Files the agent writes or edits appear here\./);
  assert.match(empty, /Open Code view/);

  const scanning = render({ scanning: true });
  assert.match(scanning, /Scanning history…/);
});

test("the open file tab replaces the list and can go back to it", () => {
  const html = render({
    pane: {
      tabs: [{ id: "file:/work/app/src/a.ts", filePath: "/work/app/src/a.ts", label: "a.ts", sourceSessionId: "s1" }],
      activeTabId: "file:/work/app/src/a.ts",
    },
  });
  assert.doesNotMatch(html, /class="artifacts-home is-active"/);
  assert.match(html, /role="tab"/);
  assert.match(html, />a\.ts</);
});
