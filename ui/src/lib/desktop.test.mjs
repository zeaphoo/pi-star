import assert from "node:assert/strict";
import test from "node:test";
import { desktopBridge, desktopPlatform, isDesktop } from "./desktop.ts";

function withWindow(value, run) {
  const previous = globalThis.window;
  globalThis.window = value;
  try {
    run();
  } finally {
    if (previous === undefined) delete globalThis.window;
    else globalThis.window = previous;
  }
}

test("browser without the marker is not desktop", () => {
  withWindow({}, () => {
    assert.equal(desktopBridge(), null);
    assert.equal(isDesktop(), false);
    assert.equal(desktopPlatform(), null);
  });
});

test("desktop marker is recognized", () => {
  withWindow(
    { __PI_STAR__: { version: 1, desktop: true, platform: "linux", shell: "tauri" } },
    () => {
      assert.equal(isDesktop(), true);
      assert.equal(desktopPlatform(), "linux");
      assert.equal(desktopBridge().shell, "tauri");
    },
  );
});

test("a malformed marker is ignored", () => {
  withWindow({ __PI_STAR__: { desktop: false } }, () => {
    assert.equal(isDesktop(), false);
  });
  withWindow({ __PI_STAR__: null }, () => {
    assert.equal(isDesktop(), false);
  });
});

test("missing window is safe", () => {
  const previous = globalThis.window;
  delete globalThis.window;
  try {
    assert.equal(desktopBridge(), null);
    assert.equal(isDesktop(), false);
  } finally {
    if (previous !== undefined) globalThis.window = previous;
  }
});
