import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url, { jsx: { runtime: "automatic" }, tsconfigPaths: true });
const React = await jiti.import("react");
const { renderToStaticMarkup } = await jiti.import("react-dom/server");
const { I18nProvider } = await jiti.import("@/hooks/useI18n");
const { MainViewTabs } = await jiti.import("./MainViewTabs.tsx");

const h = React.createElement;
const noop = () => {};

function render(value = "git") {
  return renderToStaticMarkup(h(I18nProvider, null, h(MainViewTabs, { value, onChange: noop })));
}

test("the workspace's Git tab is a Diff tab with the file-diff icon", () => {
  const html = render();
  assert.match(html, />Diff</);
  assert.doesNotMatch(html, />Git</);
  // Lucide's file-diff (a file with plus and minus), not a Git branch: the
  // view will hold diffs from more than Git (revisions, comparisons).
  assert.match(html, /M6 22a2 2 0 0 1-2-2V4/);
  assert.doesNotMatch(html, /<circle/);
  // The view id stays "git": saved state and ?view= links keep working.
  assert.match(html, /<button[^>]*role="tab"[^>]*aria-selected="true"/);
  const selected = html.match(/<button[^>]*aria-selected="true"[^>]*>[\s\S]*?<\/button>/)?.[0] ?? "";
  assert.match(selected, />Diff</);
});
