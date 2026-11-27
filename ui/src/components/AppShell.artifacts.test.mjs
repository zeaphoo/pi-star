import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("./AppShell.tsx", import.meta.url), "utf8");

test("a session's file opens in its artifacts pane; a sub-agent's transcript does not", () => {
  // ChatWindow's own file links and written-file chips go to the pane...
  assert.match(source, /onOpenFile=\{handleOpenArtifactFile\}/);
  assert.match(source, /onArtifactsChange=\{handleArtifactsChange\}/);
  // ...while a sub-agent transcript opens in its own dialog, never beside the files.
  assert.match(source, /onOpenSubagent=\{openSubagentDialog\}/);
  assert.match(source, /<SubagentDialog/);
  assert.doesNotMatch(source, /openAgentTab\(/);
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
test("trust during a run is saved now and applied after the folder goes idle", () => {
  assert.match(source, /if \(projectTrustCwdRunning\)/);
  assert.match(source, /setProjectTrustReloadCwd\(projectTrustCwd\)/);
  assert.match(source, /const stillRunning = sessionsWithSelection\.some/);
  assert.match(source, /if \(projectTrustCwd === projectTrustReloadCwd\)/);
  // The waiting status replaces the restricted-mode banner where it stood.
  assert.match(source, /data-mobile-trust-pending/);
  assert.doesNotMatch(source, /projectTrustToast/);
});

test("the restricted-mode warning has no side divider of its own", () => {
  // The warning button only draws its mobile bottom edge; other buttons keep theirs.
  assert.doesNotMatch(source, /borderRight: mobileBanner \?/);
});

test("the selected session's toolbar opens its context panel, not Settings", () => {
  assert.doesNotMatch(source, /handleSystemInfoToggle/);
  assert.doesNotMatch(source, /activeTopPanel === "system"|activeTopPanel === "tools"/);
  // One context button in the chat's action group, for a selected session.
  assert.match(source, /selectedSession && \(/);
  assert.match(source, /data-mobile-toolbar-action="context"/);
  assert.match(source, /activeTopPanel === "context" && selectedSession/);
  assert.match(source, /<SessionContextPanel/);
  assert.match(source, /onLoad=\{loadSystemInfo\}/);
  // The chat still provides the prompt, tools and loader behind the panel.
  assert.match(source, /onSystemInfoLoaderChange=\{handleSystemInfoLoaderChange\}/);
});
