import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("./CodeWorkspace.tsx", import.meta.url), "utf8");

test("the Code view's tree keeps the files' keys the sidebar had: upload, ignored files, search", () => {
  // The tree header's three actions, before the hide-tree key.
  for (const key of ["sidebar.uploadFilesTitle", "sidebar.showIgnoredFiles", "sidebar.searchFiles"]) {
    assert.match(source, new RegExp(`label=\\{t\\("${key.replaceAll(".", "\\.")}"\\)\\}`), key);
  }
  assert.match(source, /explorerRef\.current\?\.openUploadPicker\(\)/);
  // The explorer gets the state and the ref those keys drive.
  assert.match(source, /ref=\{explorerRef\}/);
  assert.match(source, /fileSearchOpen=\{fileSearchOpen\}/);
  assert.match(source, /onFileSearchOpenChange=\{setFileSearchOpen\}/);
  assert.match(source, /showHidden=\{showHidden\}/);
  assert.match(source, /onUploadBusyChange=\{setUploadBusy\}/);
  // Ignored files are a saved choice, as they were in the sidebar.
  assert.match(source, /useState\(loadShowIgnoredFiles\)/);
  assert.match(source, /saveShowIgnoredFiles\(next\)/);
});
