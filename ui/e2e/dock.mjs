/**
 * The right dock: a project's own workbench beside the chat. Every leaf is an
 * independent instance, so this suite checks that a dock terminal and the main
 * view's terminal are two different shells, that the layout survives a reload
 * without spawning new ones, and that narrow windows keep the dock away.
 */
import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { chromiumOptions, repoRoot, startServer } from "./server.mjs";

const uiRoot = fileURLToPath(new URL("../", import.meta.url));
const root = repoRoot(uiRoot);
const artifacts = mkdtempSync(join(tmpdir(), "pi-web-dock-e2e-"));
console.log(`Artifacts: ${artifacts}`);
const agentDir = join(artifacts, "agent");
const projectA = join(artifacts, "project-a");
const projectB = join(artifacts, "project-b");
mkdirSync(join(agentDir, "sessions", "test"), { recursive: true });
mkdirSync(projectA);
mkdirSync(projectB);
writeFileSync(join(projectA, "readme.md"), "# Project A\n");
const timestamp = "2026-10-01T00:00:00.000Z";
for (const [id, cwd, name] of [
  ["dock-a1", projectA, "Dock session A"],
  ["dock-b1", projectB, "Dock session B"],
]) {
  writeFileSync(join(agentDir, "sessions", "test", `${id}.jsonl`), [
    { type: "session", version: 3, id, timestamp, cwd },
    { type: "session_info", id: "name", parentId: null, timestamp, name },
    { type: "message", id: "message", parentId: "name", timestamp, message: { role: "user", content: `${name} message` } },
  ].map((entry) => JSON.stringify(entry)).join("\n") + "\n");
}

const probe = createServer();
probe.listen(0, "127.0.0.1");
await once(probe, "listening");
const port = probe.address().port;
await new Promise((resolve) => probe.close(resolve));
const base = `http://127.0.0.1:${port}`;
const { child: server, exit: serverExit } = await startServer({
  root,
  uiRoot,
  agentDir,
  port,
  artifacts,
  env: {
    HISTFILE: process.platform === "win32" ? "NUL" : "/dev/null",
    BASH_SILENCE_DEPRECATION_WARNING: "1",
    SHELL: process.platform === "win32" ? process.env.SHELL : "/bin/bash",
  },
});

let browser;
try {
  for (let i = 0; ; i++) {
    const response = await fetch(`${base}/api/sessions`).catch(() => null);
    if (response?.ok) break;
    assert.ok(i < 120 && server.exitCode === null, "Server did not become ready");
    await delay(500);
  }
  browser = await chromium.launch(chromiumOptions());

  // ── Narrow windows do not offer the dock at all ──────────────────────────
  for (const viewport of [{ width: 900, height: 700 }, { width: 390, height: 844 }]) {
    const context = await browser.newContext({ viewport, locale: "en-US" });
    const page = await context.newPage();
    page.setDefaultTimeout(30_000);
    try {
      await page.goto(`${base}/?session=dock-a1`);
      await page.getByText("Dock session A message", { exact: true }).waitFor();
      assert.equal(await page.locator('[data-dock-toggle="true"]').count(), 0, "no dock toggle below the breakpoint");
      assert.equal(await page.locator("#right-dock").count(), 0);
      console.log(`PASS: the dock stays away at ${viewport.width}px`);
    } finally {
      await context.close();
    }
  }

  // ── A desktop: independent leaves, splits, reload, per-project memory ────
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, locale: "en-US" });
  const page = await context.newPage();
  page.setDefaultTimeout(30_000);
  const errors = [];
  const created = new Set();
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (new URL(request.url()).pathname === "/api/terminal" && request.method() === "POST") created.add(request.postDataJSON().id);
  });

  const dock = page.locator("#right-dock");
  const leaves = dock.locator("[data-dock-leaf]");
  const toggle = page.locator('[data-dock-toggle="true"]');
  const splitPane = () => dock.locator('[data-dock-split="true"]').click();
  const addView = async (view) => {
    const choice = dock.locator(`[data-dock-add="${view}"]`).first();
    if (await choice.count()) {
      await choice.click();
      return;
    }
    await splitPane();
    await dock.locator(`[data-dock-add="${view}"]`).first().click();
  };
  const dockReady = () => dock.locator('.terminal-panel[data-status="ready"]').first().waitFor();
  const savedDock = () => page.evaluate(() => JSON.parse(localStorage.getItem("pi-star:dock:v1") || "{}"));
  const dockTerminalIds = async () => {
    const all = await savedDock();
    const ids = [];
    const walk = (node) => {
      if (node.kind === "pane") ids.push(node.terminalId);
      else for (const child of node.children) walk(child);
    };
    for (const dock of Object.values(all)) for (const ws of Object.values(dock.terminal ?? {})) for (const tab of ws.tabs) walk(tab.root);
    return ids;
  };

  try {
    await page.goto(`${base}/?session=dock-a1`);
    await page.getByText("Dock session A message", { exact: true }).waitFor();
    assert.equal(await toggle.count(), 1);
    await toggle.click();
    await dock.waitFor();
    // An empty dock offers the three views.
    assert.equal(await dock.locator(".dock-empty-choice").count(), 3);

    // Code first: it becomes the root leaf, with the project's tree and toolbar.
    await addView("code");
    assert.equal(await leaves.count(), 1);
    assert.equal(await leaves.first().getAttribute("data-dock-view"), "code");
    assert.equal(await dock.locator(".code-workspace-tree").count(), 1, "an empty code pane opens the file tree");

    // The arrow beside the split button chooses the direction explicitly.
    await dock.getByRole("button", { name: "Split direction", exact: true }).click();
    await dock.getByRole("menuitem", { name: "Split below", exact: true }).click();
    assert.equal(await leaves.count(), 2);
    const explicitEmpty = dock.locator('[data-dock-leaf]:not([data-dock-view])');
    assert.equal(await explicitEmpty.count(), 1);
    await explicitEmpty.getByRole("button", { name: "Close pane", exact: true }).click();
    assert.equal(await leaves.count(), 1);

    await dock.getByText("readme.md", { exact: true }).first().click();
    await dock.locator('[role="tab"]').first().waitFor();
    await dock.getByRole("button", { name: "Close readme.md", exact: true }).click();
    await dock.locator('[role="tab"]').first().waitFor({ state: "detached" });
    assert.equal(await dock.getByText("No file open", { exact: true }).count(), 1, "closing the last file tab leaves the pane empty");

    // A terminal beside it: a second, independent shell.
    await addView("terminal");
    assert.equal(await leaves.count(), 2);
    await dockReady();
    const firstTerminalIds = await dockTerminalIds();
    assert.equal(firstTerminalIds.length, 1);
    assert.equal(created.size, 1, "only the dock asked for a shell so far");

    // The main view's terminal is its own instance, not the dock's. Both are
    // visible at once, so wait for the shell it asks for rather than for "a"
    // ready panel (the dock's already is).
    const mainShell = page.waitForRequest((request) => (
      new URL(request.url()).pathname === "/api/terminal" && request.method() === "POST"
    ));
    await page.getByRole("tab", { name: "Terminal", exact: true }).click();
    await mainShell;
    await page.locator('.terminal-panel:visible[data-status="ready"]').first().waitFor();
    assert.equal(created.size, 2, "the main view started a second shell");
    const mainId = [...created].find((id) => id !== firstTerminalIds[0]);
    assert.equal((await fetch(`${base}/api/terminal/${mainId}`)).status, 200);
    assert.equal((await fetch(`${base}/api/terminal/${firstTerminalIds[0]}`)).status, 200);
    // Back to the chat: the dock is still there, the main terminal stays mounted
    // and hidden, exactly like any other main view.
    await page.getByRole("tab", { name: "Chat", exact: true }).click();
    await dock.waitFor();

    // Splitting the terminal leaf creates an empty pane; choosing Terminal makes it a shell.
    const terminalLeaf = dock.locator('[data-dock-leaf][data-dock-view="terminal"]');
    await terminalLeaf.click();
    await splitPane();
    assert.equal(await leaves.count(), 3);
    await dock.locator('[data-dock-add="terminal"]').first().click();
    await page.waitForFunction(() => Object.keys(JSON.parse(localStorage.getItem("pi-star:dock:v1") || "{}")).length > 0);
    assert.equal((await dockTerminalIds()).length, 2, "two dock shells now");

    // The terminal budget is two leaves: a fresh empty pane cannot pick Terminal.
    await splitPane();
    const terminalAdd = dock.locator('[data-dock-add="terminal"]');
    assert.equal(await terminalAdd.isDisabled(), true, "the terminal budget is two leaves");
    await dock.locator('[data-dock-leaf]:not([data-dock-view])').getByRole("button", { name: "Close pane", exact: true }).click();
    assert.equal(await leaves.count(), 3);

    // Closing a filled pane empties it; closing the empty pane removes it.
    const codeLeaf = dock.locator('[data-dock-leaf][data-dock-view="code"]');
    const codeLeafId = await codeLeaf.getAttribute("data-dock-leaf");
    await codeLeaf.getByText("readme.md", { exact: true }).first().click();
    await dock.locator('[role="tab"]').first().waitFor();
    await codeLeaf.getByRole("button", { name: "Close view content", exact: true }).click();
    assert.equal(await leaves.count(), 3, "the pane slot stays");
    assert.equal(await dock.getByText("Choose a view", { exact: true }).count(), 1, "an open file does not bring the code pane back");
    const emptiedCode = dock.locator(`[data-dock-leaf="${codeLeafId}"]`);
    await emptiedCode.getByRole("button", { name: "Close pane", exact: true }).click();
    assert.equal(await leaves.count(), 2);
    await page.waitForFunction(() => document.querySelectorAll("#right-dock [data-dock-leaf]").length === 2);

    // A reload restores the layout, the shells and the dock's open state.
    const savedBeforeReload = await dockTerminalIds();
    const shellsBeforeReload = created.size;
    await page.reload();
    await page.getByText("Dock session A message", { exact: true }).waitFor();
    await dock.waitFor();
    assert.equal(await leaves.count(), 2, "the layout came back");
    assert.deepEqual(await dockTerminalIds(), savedBeforeReload, "the same shells, not new ones");
    assert.equal(created.size, shellsBeforeReload, "a reload must reconnect, not create");
    await dockReady();

    // The dock is per project: another project starts empty, this one returns.
    await page.goto(`${base}/?session=dock-b1`);
    await page.getByText("Dock session B message", { exact: true }).waitFor();
    await dock.waitFor();
    assert.equal(await leaves.count(), 0, "the other project has no dock layout yet");
    assert.equal(await dock.locator(".dock-empty-choice").count(), 3);
    await page.goto(`${base}/?session=dock-a1`);
    await page.getByText("Dock session A message", { exact: true }).waitFor();
    await dock.waitFor();
    assert.equal(await leaves.count(), 2, "this project's own layout returned");
    await dockReady();

    // Closing the dock keeps its layout for next time.
    await dock.getByRole("button", { name: "Hide dock", exact: true }).click();
    await page.locator("#right-dock").waitFor({ state: "detached" });
    await toggle.click();
    await dock.waitFor();
    assert.equal(await leaves.count(), 2);

    assert.deepEqual(errors, []);
    console.log("PASS: dock leaves are independent, split, survive a reload and stay per project");
  } catch (error) {
    await page.screenshot({ path: join(artifacts, "dock-failure.png") }).catch(() => {});
    throw error;
  } finally {
    await context.close();
  }
} finally {
  await browser?.close();
  // Stop the server, then wait for it: killing only after awaiting its exit
  // would wait on the very process that is still running.
  if (server.exitCode === null) server.kill("SIGTERM");
  if (server.exitCode === null) await serverExit;
}