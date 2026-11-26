import assert from "node:assert/strict";
import test from "node:test";
import { keepsNativeMenuInDebug, shouldSuppressContextMenu } from "./policy.js";

const target = (overrides = {}) => ({
  editable: false,
  hasSelection: false,
  nativeOptOut: false,
  ...overrides,
});

test("app chrome loses the browser menu", () => {
  assert.equal(shouldSuppressContextMenu(target()), true);
});

test("editable fields keep their system menu", () => {
  assert.equal(shouldSuppressContextMenu(target({ editable: true })), false);
});

test("a text selection keeps copy and look-up available", () => {
  assert.equal(shouldSuppressContextMenu(target({ hasSelection: true })), false);
});

test("an opted-out subtree keeps its system menu", () => {
  assert.equal(shouldSuppressContextMenu(target({ nativeOptOut: true })), false);
});

test("debug builds keep the system menu for Inspect Element", () => {
  assert.equal(keepsNativeMenuInDebug({ debug: true }), true);
  assert.equal(keepsNativeMenuInDebug({ debug: false }), false);
  assert.equal(keepsNativeMenuInDebug(null), false);
});
