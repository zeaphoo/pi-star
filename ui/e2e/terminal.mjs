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
const artifacts = mkdtempSync(join(tmpdir(), "pi-web-terminal-e2e-"));
console.log(`Artifacts: ${artifacts}`);
const agentDir = join(artifacts, "agent");
const workspace = join(artifacts, "workspace-a");
const workspaceSubdir = join(workspace, "sub");
mkdirSync(join(agentDir, "sessions", "test"), { recursive: true });
mkdirSync(workspace);
mkdirSync(workspaceSubdir);
const timestamp = "2026-09-05T00:00:00.000Z";
for (const [id, cwd, name] of [
  ["terminal-a1", workspace, "Terminal session one"],
  ["terminal-a2", workspace, "Terminal session two"],
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
  for (const viewport of [{ width: 1440, height: 900 }, { width: 390, height: 844 }]) {
    const context = await browser.newContext({ viewport, locale: "en-US" });
    const page = await context.newPage();
    page.setDefaultTimeout(30_000);
    const errors = [];
    const created = new Set();
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("request", (request) => {
      if (new URL(request.url()).pathname === "/api/terminal" && request.method() === "POST") created.add(request.postDataJSON().id);
    });
    // The terminal is a main view: switching to it starts the project's first shell.
    const openTerminal = () => page.getByRole("tab", { name: "Terminal", exact: true }).click();
    const openChat = () => page.getByRole("tab", { name: "Chat", exact: true }).click();
    const ready = () => page.locator('.terminal-panel:visible[data-status="ready"]').waitFor();
    // The main view is remembered, so a reload may already show the terminal.
    const reopenTerminal = async () => {
      await page.getByRole("tab", { name: "Terminal", exact: true }).click();
      await ready();
    };
    const openSettings = async () => {
      const showSidebar = page.getByRole("button", { name: "Show sidebar", exact: true });
      if (await showSidebar.count()) await showSidebar.first().click().catch(() => {});
      await page.getByRole("button", { name: "Settings", exact: true }).click();
      const settings = page.getByRole("dialog", { name: "Settings", exact: true });
      await settings.waitFor();
      // A phone opens General as a list of sections; the select lives in its
      // Appearance detail.
      const select = settings.locator("#settings-terminal-theme");
      if (!(await select.isVisible())) {
        const appearance = settings.getByRole("button", { name: "Appearance", exact: true }).first();
        if (await appearance.count()) await appearance.click();
        await select.waitFor();
      }
      return select;
    };
    const closeSettings = async () => {
      const settings = page.getByRole("dialog", { name: "Settings", exact: true });
      await settings.getByRole("button", { name: "Close", exact: true }).click();
      await settings.waitFor({ state: "detached" });
    };
    const text = () => page.locator(".terminal-panel:visible .xterm-rows").innerText();
    const run = async (command) => {
      await page.locator(".terminal-panel:visible .xterm-helper-textarea").focus();
      await page.keyboard.type(command);
      await page.keyboard.press("Enter");
    };
    const waitOutput = (pattern) => page.waitForFunction((source) => {
      const panel = [...document.querySelectorAll(".terminal-panel")].find((element) => element.getBoundingClientRect().width > 0);
      return new RegExp(source).test(panel?.querySelector(".xterm-rows")?.textContent ?? "");
    }, pattern);
    /** Every terminal id the workspace remembers, across tabs and splits. */
    const savedTerminalIds = () => page.evaluate(() => {
      const all = JSON.parse(localStorage.getItem("pi-star:terminal-workspaces:v1") || "{}");
      const ids = [];
      const walk = (node) => {
        if (node.kind === "pane") ids.push(node.terminalId);
        else for (const child of node.children) walk(child);
      };
      for (const workspace of Object.values(all)) for (const tab of workspace.tabs) walk(tab.root);
      return ids;
    });
    try {
      await page.goto(`${base}/?session=terminal-a1`);
      await page.getByText("Terminal session one message", { exact: true }).waitFor();
      await openTerminal();
      await ready();
      await run("export E2E_TOKEN=alive; printf '\\nTOKEN:%s:%s\\n' \"$E2E_TOKEN\" \"$$\"");
      await waitOutput("TOKEN:alive:[0-9]+");
      const pid = (await text()).match(/TOKEN:alive:(\d+)/)[1];
      const [firstId] = created;
      assert.equal(created.size, 1);

      // Switching sessions of the same project keeps the shell: it is project-scoped.
      await openChat();
      await page.getByRole("button", { name: "Show sidebar", exact: true }).click().catch(() => {});
      await page.getByText("Terminal session two", { exact: true }).click();
      await page.getByText("Terminal session two message", { exact: true }).waitFor();
      await openTerminal();
      await ready();
      assert.equal(created.size, 1, "another session of the project must not spawn a second shell");
      await run("printf '\\nSESSION:%s:%s\\n' \"$E2E_TOKEN\" \"$$\"");
      await waitOutput(`SESSION:alive:${pid}`);

      // A reload reattaches to the shell that is still running.
      await page.reload();
      await reopenTerminal();
      assert.equal(created.size, 1, "a reload must reconnect, not create");
      await run("printf '\\nREFRESH:%s:%s\\n' \"$E2E_TOKEN\" \"$$\"");
      await waitOutput(`REFRESH:alive:${pid}`);
      const replayed = ((await text()).match(/REFRESH:alive:\d+/g) ?? []).length;
      assert.equal(replayed, 1, "a reconnect must not replay delivered output");

      // A restart ends that shell and starts a fresh one. The action lives in
      // the tab toolbar, beside split and close pane.
      await page.getByRole("button", { name: "Restart terminal", exact: true }).click();
      await page.waitForFunction((oldId) => {
        const all = JSON.parse(localStorage.getItem("pi-star:terminal-workspaces:v1") || "{}");
        const ids = [];
        const walk = (node) => {
          if (node.kind === "pane") ids.push(node.terminalId);
          else for (const child of node.children) walk(child);
        };
        for (const workspace of Object.values(all)) for (const tab of workspace.tabs) walk(tab.root);
        return ids.length === 1 && ids[0] !== oldId;
      }, firstId);
      await ready();
      assert.equal((await fetch(`${base}/api/terminal/${firstId}`)).status, 404);
      assert.equal(created.size, 2);
      const [restartedId] = [...created].filter((id) => id !== firstId);

      // The server follows the shell into a subdirectory, and the layout asks
      // for that same cwd when it is restored.
      await run("cd sub");
      const [cwdTerminalId] = await savedTerminalIds();
      await page.waitForFunction(async ({ id, expected }) => {
        const response = await fetch(`/api/terminal/${id}`);
        return response.ok && (await response.json()).cwd === expected;
      }, { id: cwdTerminalId, expected: workspaceSubdir });
      await page.waitForFunction((expected) => {
        const all = JSON.parse(localStorage.getItem("pi-star:terminal-workspaces:v1") || "{}");
        const panes = [];
        const walk = (node) => {
          if (node.kind === "pane") panes.push(node);
          else for (const child of node.children) walk(child);
        };
        for (const workspace of Object.values(all)) for (const tab of workspace.tabs) walk(tab.root);
        return panes.some((pane) => pane.cwd === expected);
      }, workspaceSubdir);
      await page.reload();
      await reopenTerminal();
      assert.ok((await page.locator('.terminal-tabstrip [role="tab"][aria-selected="true"]').getAttribute("title")).includes("sub"), "a reload keeps the restorable cwd");

      // If the server forgot the PTY, the pane is recreated in that cwd instead
      // of showing "Terminal expired or closed".
      const beforeRecovery = created.size;
      await fetch(`${base}/api/terminal/${cwdTerminalId}`, { method: "DELETE" });
      await page.reload();
      await reopenTerminal();
      await page.waitForFunction((oldId) => {
        const all = JSON.parse(localStorage.getItem("pi-star:terminal-workspaces:v1") || "{}");
        const ids = [];
        const walk = (node) => {
          if (node.kind === "pane") ids.push(node.terminalId);
          else for (const child of node.children) walk(child);
        };
        for (const workspace of Object.values(all)) for (const tab of workspace.tabs) walk(tab.root);
        return ids.length === 1 && ids[0] !== oldId;
      }, cwdTerminalId);
      const [recoveredTerminalId] = await savedTerminalIds();
      assert.notEqual(recoveredTerminalId, cwdTerminalId);
      assert.equal(created.size, beforeRecovery + 1, "recovery starts exactly one replacement shell");
      const recoveredInfo = await (await fetch(`${base}/api/terminal/${recoveredTerminalId}`)).json();
      assert.equal(recoveredInfo.cwd, workspaceSubdir);
      const finalTerminalId = recoveredTerminalId;

      // Switching tabs unmounts the xterm; coming back must reattach to the
      // same shell, not spawn a replacement.
      await run("export TAB_TOKEN=kept; printf '\\nTAB:%s:%s\\n' \"$TAB_TOKEN\" \"$$\"");
      await waitOutput("TAB:kept:[0-9]+");
      const tabPid = (await text()).match(/TAB:kept:(\d+)/)[1];
      await page.getByRole("button", { name: "New terminal tab", exact: true }).click();
      await ready();
      const tabs = page.locator(".terminal-tabstrip [role='tab']");
      assert.equal(await tabs.count(), 2);
      await tabs.first().click();
      await ready();
      await run("printf '\\nTAB2:%s:%s\\n' \"$TAB_TOKEN\" \"$$\"");
      await waitOutput(`TAB2:kept:${tabPid}`);
      await tabs.last().getByRole("button", { name: "Close terminal tab", exact: true }).click();
      await page.getByRole("dialog", { name: "Close terminal tab?" }).waitFor();
      await page.getByRole("dialog").getByRole("button", { name: "Close terminal tab", exact: true }).click();
      await page.waitForFunction(() => document.querySelectorAll(".terminal-tabstrip [role='tab']").length === 1);

      // The terminal theme is a global setting, not a per-pane button. The
      // phone's Settings button is behind the mobile sidebar, already covered
      // by the appearance suite; the desktop pass checks the terminal wiring.
      if (viewport.width >= 640) {
        const terminalBackground = () => page.locator(".terminal-panel:visible .xterm-viewport").evaluate((el) => getComputedStyle(el).backgroundColor);
        const terminalTheme = await openSettings();
        await terminalTheme.selectOption("light");
        await closeSettings();
        await page.locator('.terminal-panel:visible[data-terminal-theme="light"]').waitFor();
        assert.equal(await terminalBackground(), "rgb(251, 251, 252)");
        const terminalThemeAgain = await openSettings();
        await terminalThemeAgain.selectOption("dark");
        await closeSettings();
        await page.locator('.terminal-panel:visible[data-terminal-theme="dark"]').waitFor();
        assert.equal(await terminalBackground(), "rgb(17, 19, 24)");
      }

      // Closing the last tab asks first, then ends its shell.
      await page.getByRole("button", { name: "Close terminal tab", exact: true }).click();
      await page.getByRole("dialog", { name: "Close terminal tab?" }).waitFor();
      await page.getByRole("dialog").getByRole("button", { name: "Close terminal tab", exact: true }).click();
      await page.locator(".terminal-panel").waitFor({ state: "detached" });
      assert.equal((await fetch(`${base}/api/terminal/${finalTerminalId}`)).status, 404);
      assert.deepEqual(await savedTerminalIds(), [], "no shell is remembered after its tab closed");

      // A split tab recovers every pane. Both panes notice the lost PTY in the
      // same tick, so the later update must not write back the earlier pane's
      // dead id (one pane used to stay blank).
      await page.getByRole("button", { name: "Open workspace terminal", exact: true }).click();
      await ready();
      await run("cd sub");
      await page.waitForFunction((expected) => {
        const all = JSON.parse(localStorage.getItem("pi-star:terminal-workspaces:v1") || "{}");
        const panes = [];
        const walk = (node) => {
          if (node.kind === "pane") panes.push(node);
          else for (const child of node.children) walk(child);
        };
        for (const workspace of Object.values(all)) for (const tab of workspace.tabs) walk(tab.root);
        return panes.some((pane) => pane.cwd === expected);
      }, workspaceSubdir);
      await page.getByRole("button", { name: "Split pane right", exact: true }).click();
      await page.waitForFunction(() => document.querySelectorAll(".terminal-panel").length === 2);
      await page.waitForFunction(() => [...document.querySelectorAll(".terminal-panel")].every((element) => element.getAttribute("data-status") === "ready"));
      const beforeSplitRecovery = created.size;
      const splitTerminalIds = await savedTerminalIds();
      assert.equal(splitTerminalIds.length, 2, "a split tab remembers both shells");
      for (const oldId of splitTerminalIds) {
        await fetch(`${base}/api/terminal/${oldId}`, { method: "DELETE" });
      }
      await page.reload();
      await page.getByRole("tab", { name: "Terminal", exact: true }).click();
      await page.waitForFunction(() => document.querySelectorAll(".terminal-panel").length === 2);
      await page.waitForFunction(() => [...document.querySelectorAll(".terminal-panel")].every((element) => element.getAttribute("data-status") === "ready"));
      const recoveredSplitIds = await savedTerminalIds();
      assert.equal(recoveredSplitIds.length, 2, "both split panes keep their layout");
      for (const oldId of splitTerminalIds) {
        assert.ok(!recoveredSplitIds.includes(oldId), "no pane keeps a forgotten terminal id");
      }
      assert.equal(created.size, beforeSplitRecovery + 2, "each split pane starts one replacement shell");
      const recoveredCwds = await Promise.all(recoveredSplitIds.map(async (terminalId) => (await (await fetch(`${base}/api/terminal/${terminalId}`)).json()).cwd));
      assert.deepEqual(recoveredCwds, [workspaceSubdir, workspaceSubdir]);

      // `exit` closes that shell's pane, and the last pane takes its tab with
      // it, the way a terminal window behaves.
      await page.locator(".terminal-panel").nth(1).locator(".xterm-helper-textarea").focus();
      await page.keyboard.type("exit");
      await page.keyboard.press("Enter");
      await page.waitForFunction(() => document.querySelectorAll(".terminal-panel").length === 1);
      await ready();
      await run("exit");
      await page.locator(".terminal-panel").waitFor({ state: "detached" });
      assert.deepEqual(await savedTerminalIds(), [], "exit closes the last pane and its tab");

      assert.deepEqual(errors, []);
      console.log(`PASS: terminal at ${viewport.width}px (project-scoped shell, reload reattach, restart, theme)`);
    } catch (error) {
      await page.screenshot({ path: join(artifacts, `terminal-${viewport.width}-failure.png`) }).catch(() => {});
      throw error;
    } finally {
      await context.close();
    }
  }
} finally {
  await browser?.close();
  // Stop the server, then wait for it: killing only after awaiting its exit
  // would wait on the very process that is still running.
  if (server.exitCode === null) server.kill("SIGTERM");
  if (server.exitCode === null) await serverExit;
}