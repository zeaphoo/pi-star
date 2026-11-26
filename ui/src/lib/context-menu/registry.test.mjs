import assert from "node:assert/strict";
import test from "node:test";
import { clearContextMenus, registerContextMenu, resolveContextMenu } from "./registry.js";

test("a registered id builds entries from its element", () => {
  clearContextMenus();
  registerContextMenu("file", (element) => [{ id: "copy", label: element.dataset.path }]);
  const element = { dataset: { contextMenu: "file", path: "/tmp/a" } };
  assert.deepEqual(resolveContextMenu(element), [{ id: "copy", label: "/tmp/a" }]);
});

test("unknown ids resolve to null", () => {
  clearContextMenus();
  assert.equal(resolveContextMenu({ dataset: { contextMenu: "missing" } }), null);
  assert.equal(resolveContextMenu({ dataset: {} }), null);
  assert.equal(resolveContextMenu(null), null);
});

test("re-registering replaces the builder and the cleanup is idempotent", () => {
  clearContextMenus();
  const first = () => [{ id: "a", label: "a" }];
  const second = () => [{ id: "b", label: "b" }];
  const dispose = registerContextMenu("x", first);
  registerContextMenu("x", second);
  assert.equal(resolveContextMenu({ dataset: { contextMenu: "x" } })[0].id, "b");
  dispose();
  assert.equal(resolveContextMenu({ dataset: { contextMenu: "x" } })[0].id, "b");
});
