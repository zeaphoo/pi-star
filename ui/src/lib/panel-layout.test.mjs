import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url);
const {
  clampPanelWidth,
  getSidebarMaxWidth,
} = await jiti.import("./panel-layout.ts");

test("clamps panel widths to finite bounds", () => {
  assert.equal(clampPanelWidth(420.4, 180, 480), 420);
  assert.equal(clampPanelWidth(120, 180, 480), 180);
  assert.equal(clampPanelWidth(600, 180, 480), 480);
  assert.equal(clampPanelWidth(Number.NaN, 180, 480), 180);
  assert.equal(clampPanelWidth(200, 300, 250), 300);
});

test("the sidebar gives way to the chat and to an open side panel", () => {
  assert.equal(getSidebarMaxWidth({
    viewportWidth: 700,
    panelOpen: true,
    panelWidth: 560,
  }), 380);
  assert.equal(getSidebarMaxWidth({
    viewportWidth: 1366,
    panelOpen: true,
    panelWidth: 686,
  }), 260);
  assert.equal(getSidebarMaxWidth({
    viewportWidth: 1366,
    panelOpen: false,
    panelWidth: 686,
  }), 480, "a closed panel gives the room back, up to the sidebar's own cap");
  assert.equal(getSidebarMaxWidth({
    viewportWidth: 500,
    panelOpen: true,
    panelWidth: 400,
  }), 480, "a phone keeps the sidebar at its cap");
});
