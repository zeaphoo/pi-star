import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("./AppShell.tsx", import.meta.url), "utf8");

test("a session's file opens in its artifacts pane, not the shared file panel", () => {
  // ChatWindow's own file chips and links go to the artifacts handler...
  assert.match(source, /onOpenFile=\{handleOpenArtifactFile\}/);
  assert.match(source, /onArtifactsChange=\{handleArtifactsChange\}/);
  // ...while the shared panel and the subagent viewer keep theirs.
  assert.match(source, /onOpenFile=\{handleOpenLinkedFile\}/);
  assert.match(source, /onOpenFile=\{\(filePath, page\) => handleOpenFile\(/);
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
