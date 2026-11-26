import assert from "node:assert/strict";
import test from "node:test";
import { fileManagerTarget, revealMenuLabelKey } from "./file-manager.js";

test("the desktop shell's platform picks the wording", () => {
  assert.equal(revealMenuLabelKey({ platform: "macos" }), "menu.revealInFinder");
  assert.equal(revealMenuLabelKey({ platform: "windows" }), "menu.revealInExplorer");
  assert.equal(revealMenuLabelKey({ platform: "linux" }), "menu.revealInFileManager");
});

test("a plain browser falls back to the user agent", () => {
  assert.equal(fileManagerTarget({ platform: null, userAgent: "Mozilla/5.0 (Macintosh)" }), "finder");
  assert.equal(fileManagerTarget({ platform: null, userAgent: "Mozilla/5.0 (Windows NT 10.0)" }), "explorer");
  assert.equal(revealMenuLabelKey({ platform: null, userAgent: "Mozilla/5.0 (X11; Linux)" }), "menu.revealInFileManager");
});
