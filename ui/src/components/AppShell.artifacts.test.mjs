import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("./AppShell.tsx", import.meta.url), "utf8");

test("a session's file opens in its artifacts pane, and a sub-agent's too", () => {
  // ChatWindow's own file links and written-file chips go to the pane...
  assert.match(source, /onOpenFile=\{handleOpenArtifactFile\}/);
  assert.match(source, /onArtifactsChange=\{handleArtifactsChange\}/);
  // ...as does a sub-agent transcript, which the pane hosts beside the files.
  assert.match(source, /onOpenSubagent=\{handleOpenSubagentTab\}/);
  assert.match(source, /openAgentTab\(current\.tabs, sessionId, label\)/);
  // The old shared file panel is gone: nothing opens there any more.
  assert.doesNotMatch(source, /handleOpenLinkedFile|#file-panel|rightPanelOpen/);
});

test("the artifacts pane is its own panel: remembered open state, own width and per-session tabs", () => {
  assert.match(source, /const ARTIFACTS_OPEN_KEY = "pi-star:artifacts-open"/);
  assert.match(source, /localStorage\.getItem\(ARTIFACTS_OPEN_KEY\)/);
  assert.match(source, /localStorage\.setItem\(ARTIFACTS_OPEN_KEY/);
  assert.match(source, /storageKey: "pi-star:artifacts-width"/);
  assert.match(source, /ARTIFACT_PANES_KEY/);
  // The toggle is left of Debug, with the artifact count.
  assert.match(source, /aria-controls="session-artifacts"/);
  assert.match(source, /className="artifacts-button-count"/);
});

test("the pane scans the whole transcript through the chat's paging endpoint", () => {
  assert.match(source, /loadSessionArtifacts\(\{/);
  assert.match(source, /artifactScanCacheRef/);
  assert.match(source, /setArtifactsScanning\(true\)/);
  assert.match(source, /controller\.abort\(\)/);
});

test("the right dock is project-scoped, independent of the main view", () => {
  assert.match(source, /loadDockStates\(\)/);
  assert.match(source, /saveDockStates\(dockStates\)/);
  assert.match(source, /const dockProjectKey = gitProjectRoot;/);
  assert.match(source, /storageKey: "pi-star:dock-width"/);
  // The toggle only shows where a split makes sense: a wide enough window that
  // still leaves the chat its own column.
  assert.match(source, /setCanDock\(!isMobile && viewportWidth >= DOCK_MIN_VIEWPORT && room >= DOCK_MIN_WIDTH \+ DOCK_MIN_CHAT_WIDTH\)/);
  assert.match(source, /aria-controls="right-dock"/);
  // Reads through the dock's own state, never the main view's workspaces.
  assert.match(source, /<RightDock[\s\S]{0,400}state=\{dockState\}/);
  assert.doesNotMatch(source, /onChange=\{updateCodeWorkspace\}[\s\S]{0,80}<RightDock/);
});
test("the toolbar no longer opens its own system and tools panels", () => {
  assert.doesNotMatch(source, /handleSystemInfoToggle/);
  assert.doesNotMatch(source, /translate\("system\.label"\)|translate\("tools\.label"\)/);
  assert.doesNotMatch(source, /activeTopPanel === "system"|activeTopPanel === "tools"/);
  // Settings owns that view now, and the chat still provides the data.
  assert.match(source, /systemPrompt=\{systemPrompt\}/);
  assert.match(source, /onLoadSystemInfo=\{loadSystemInfo\}/);
  assert.match(source, /onSystemInfoLoaderChange=\{handleSystemInfoLoaderChange\}/);
});
