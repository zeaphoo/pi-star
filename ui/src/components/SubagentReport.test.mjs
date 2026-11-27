import assert from "node:assert/strict";
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
const { parseSubagentNotification } = await jiti.import("@/lib/subagent-report");
const { SubagentReportView } = await jiti.import("./SubagentReport.tsx");

const SESSION = "3689737f-d4a9-4330-940b-06d4594c2802";

function notification(body, details = {}) {
  return {
    role: "custom",
    customType: "pi-star:subagent-notification",
    display: true,
    content: `Subagent ${SESSION} (explorer) completed:\n\n${body}`,
    details: {
      kind: "pi-star-subagent",
      sessionId: SESSION,
      profile: "explorer",
      description: "分析 Rust 后端功能",
      status: "completed",
      runInBackground: true,
      createdAt: "2026-11-27T03:17:05.119Z",
      completedAt: "2026-11-27T03:22:00.303Z",
      ...details,
    },
  };
}

function render(message, props = {}) {
  const report = parseSubagentNotification(message);
  assert.ok(report);
  return renderToStaticMarkup(
    React.createElement(
      I18nProvider,
      null,
      React.createElement(SubagentReportView, { report, ...props }),
    ),
  );
}

test("a long report starts collapsed behind a two-line preview", () => {
  const body = `# 报告标题\n\n${"很长的正文。".repeat(120)}\n\nUNIQUE_LONG_BODY_MARKER`;
  const html = render(notification(body));
  assert.match(html, /data-subagent-report=""/);
  assert.match(html, /data-subagent-report-preview=""/);
  assert.match(html, /Sub-agent report/);
  assert.match(html, /分析 Rust 后端功能/);
  assert.match(html, /Completed/);
  assert.match(html, /4m 55s/);
  assert.doesNotMatch(html, /UNIQUE_LONG_BODY_MARKER/);
});

test("a short report opens with its markdown body and the run's session id", () => {
  const html = render(notification("# 短报告\n\n正文 **重点**。"));
  assert.doesNotMatch(html, /data-subagent-report-preview=""/);
  assert.match(html, /短报告/);
  assert.match(html, /<strong>重点<\/strong>/);
  assert.match(html, new RegExp(`title="${SESSION}"`));
  assert.match(html, /3689737f/);
});

test("a failed run shows its error instead of an empty body", () => {
  const html = render(notification("", { status: "failed", error: "provider exploded" }));
  assert.match(html, /Failed/);
  assert.match(html, /provider exploded/);
});

test("the side-panel button appears only when the chat offers one", () => {
  const message = notification("短报告。");
  assert.doesNotMatch(render(message), /data-subagent-report-open=""/);
  assert.match(render(message, { onOpenSubagent() {} }), /data-subagent-report-open=""/);
});
