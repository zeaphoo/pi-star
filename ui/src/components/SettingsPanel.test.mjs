import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const cssSource = await readFile(new URL("../app/settings.css", import.meta.url), "utf8");
const panelSource = await readFile(new URL("./SettingsPanel.tsx", import.meta.url), "utf8");
const contextSource = await readFile(new URL("./SessionContextSettings.tsx", import.meta.url), "utf8");
const terminalSource = await readFile(new URL("./TerminalSettings.tsx", import.meta.url), "utf8");
const loginSource = await readFile(new URL("../app/login/page.tsx", import.meta.url), "utf8");

test("the trust dialog Settings › MCP opens stacks above Settings", () => {
  const zIndex = (selector) => Number(cssSource.match(new RegExp(`\\${selector} \\{[^}]*z-index: (\\d+);`))?.[1]);
  assert.ok(zIndex(".project-trust-backdrop") > zIndex(".settings-dialog-backdrop"));
});

test("the login page returns only to a destination safeLoginDestination() accepts", () => {
  assert.match(loginSource, /safeLoginDestination\(destination, window\.location\.origin\)/);
});

test("Session context lives in Settings, not on the toolbar", () => {
  // Listed between the other sections and hosted by the panel.
  assert.match(panelSource, /\{ id: "context", label: t\("settings\.context"\) \}/);
  assert.match(panelSource, /sectionHost\("context", \(/);
  assert.match(panelSource, /<SessionContextSettings/);
  // It is a split view like the other sections: a sidebar picks which half of
  // what the two retired panels showed, and the detail renders it read-only.
  assert.match(contextSource, /<ConfigSplitView>/);
  assert.match(contextSource, /<ConfigSidebar>/);
  assert.match(contextSource, /<ConfigSidebarItem/);
  assert.match(contextSource, /<ConfigDetail>/);
  assert.match(contextSource, /<SystemPromptPanel embedded/);
  assert.match(contextSource, /<ToolDefinitionsPanel embedded/);
  assert.match(contextSource, /onClick=\{onLoad\}/);
  // The sections that need a project say so once, in settings-navigation.
  assert.doesNotMatch(panelSource, /requiresProject: true/);
});

test("Terminal sits under General › Appearance in the sidebar", () => {
  assert.doesNotMatch(panelSource, /\{ id: "terminal", label:/);
  assert.match(panelSource, /const \[generalView, setGeneralView\] = useState<"appearance" \| "terminal" \| "code" \| "chat">\("appearance"\)/);
  assert.match(panelSource, /generalView === "terminal"/);
  assert.match(panelSource, /<TerminalSettings \/>/);
  assert.match(panelSource, /setGeneralView\("terminal"\)/);
  assert.match(panelSource, /active=\{generalView === "terminal"\}/);
  assert.match(terminalSource, /<TerminalThemePicker \/>/);
  assert.match(terminalSource, /<TerminalTypographySettings \/>/);
  assert.doesNotMatch(terminalSource, /settings\.typography/);
});

test("General is a split view too: Appearance, Terminal, Code and Chat", () => {
  assert.match(panelSource, /<ConfigSplitView>/);
  assert.match(panelSource, /<TerminalSettings \/>/);
  assert.match(panelSource, /generalView === "appearance"/);
  assert.match(panelSource, /generalView === "terminal"/);
  assert.match(panelSource, /generalView === "code" \? code : conversation/);
  assert.match(panelSource, /setGeneralView\("code"\)/);
  assert.match(panelSource, /active=\{generalView === "code"\}/);

  const appearance = panelSource.slice(panelSource.indexOf("const appearance = ("), panelSource.indexOf("const conversation = ("));
  const conversation = panelSource.slice(panelSource.indexOf("const conversation = ("), panelSource.indexOf("const account = ("));
  const account = panelSource.slice(panelSource.indexOf("const account = ("), panelSource.indexOf("const code = ("));
  const code = panelSource.slice(panelSource.indexOf("const code = ("), panelSource.indexOf("return (\n    <div className=\"settings-general-root\">"));

  // Looking: theme, fonts and layout, then language; the build's versions live
  // at the bottom of the sidebar.
  assert.match(appearance, /settings\.typography/);
  assert.match(appearance, /settings\.chatContentFontSize/);
  assert.match(account, /common\.language/);
  assert.doesNotMatch(account, /settings\.version/);
  assert.match(panelSource, /settings-sidebar-version/);
  assert.match(panelSource, /settings\.appVersion/);
  assert.match(panelSource, /settings\.piVersion/);
  assert.doesNotMatch(appearance, /settings\.thinkingExpandedDefault/);
  // Code is its own view: syntax colours and file icons are not in Appearance.
  assert.match(code, /settings\.codeTheme/);
  assert.match(code, /settings\.fileIcons/);
  assert.match(code, /settings\.colorfulFileIcons/);
  assert.doesNotMatch(appearance, /settings\.codeTheme/);
  assert.doesNotMatch(appearance, /settings\.fileIcons/);

  // Behaving: how a conversation sends, quotes and reports.
  assert.match(conversation, /settings\.thinkingExpandedDefault/);
  assert.match(conversation, /settings\.quoteSelection/);
  assert.match(conversation, /settings\.enterSendMode/);
  assert.match(conversation, /settings\.cacheMissNotices/);
  assert.match(conversation, /settings\.usePowerShell/);
  assert.doesNotMatch(conversation, /common\.language/);
});
