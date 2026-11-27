// One browser profile, one /api/stream socket: every tab and every terminal
// pane shares it, and no per-feature SSE endpoint is used.
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
const artifacts = mkdtempSync(join(tmpdir(), "pi-star-bus-e2e-"));
console.log(`Artifacts: ${artifacts}`);
const agentDir = join(artifacts, "agent");
const workspace = join(artifacts, "project");
mkdirSync(join(agentDir, "sessions", "test"), { recursive: true });
mkdirSync(workspace);
const timestamp = "2026-11-20T00:00:00.000Z";
writeFileSync(join(agentDir, "sessions", "test", "bus-1.jsonl"), [
  { type: "session", version: 3, id: "bus-1", timestamp, cwd: workspace },
  { type: "session_info", id: "name", parentId: null, timestamp, name: "Bus session" },
  { type: "message", id: "message", parentId: "name", timestamp, message: { role: "user", content: "hello" } },
].map((entry) => JSON.stringify(entry)).join("\n") + "\n");

const probe = createServer();
probe.listen(0, "127.0.0.1");
await once(probe, "listening");
const port = probe.address().port;
await new Promise((resolve) => probe.close(resolve));
const base = `http://127.0.0.1:${port}`;
const { child: server, exit: serverExit } = await startServer({ root, uiRoot, agentDir, port, artifacts });
let browser;
try {
  for (let i = 0; ; i++) {
    const response = await fetch(`${base}/api/sessions`).catch(() => null);
    if (response?.ok) break;
    assert.ok(i < 120 && server.exitCode === null, "Server did not become ready");
    await delay(500);
  }
  browser = await chromium.launch(chromiumOptions());
  const context = await browser.newContext();
  const page = await context.newPage();
  const cdp = await context.newCDPSession(page);
  const sse = [];
  cdp.on("Network.requestWillBeSent", ({ request }) => {
    const path = new URL(request.url).pathname;
    if (path.endsWith("/events")) sse.push(path);
  });
  await cdp.send("Network.enable");

  await page.goto(`${base}/?session=bus-1`);
  await page.waitForFunction(() => window.__piStarBus?.().mode === "bus");
  await page.waitForFunction(() => window.__piStarBus().channels.some((channel) => channel.channel.startsWith("agent:")));
  await page.waitForFunction(() => window.__piStarBus().state === "open");
  const initial = await page.evaluate(() => window.__piStarBus());
  assert.equal(initial.sockets, 1, "the page holds one bus socket");
  // The main-view terminal is a channel on the same socket. The UI follows the
  // browser's language, so match the tab by its position, not its label.
  await page.getByRole("tab", { name: /^(Terminal|终端|終端)$/ }).click();
  await page.locator('.terminal-panel:visible[data-status="ready"]').waitFor();
  const first = await page.evaluate(() => window.__piStarBus());
  assert.equal(first.sockets, 1);
  assert.ok(first.channels.some((channel) => channel.kind === "terminal"), JSON.stringify(first.channels));

  // Switching terminal tabs remounts the pane: the fresh channel must answer
  // with a snapshot, not a silent cursor replay (that left the pane blank).
  await page.getByRole("button", { name: /New terminal tab|新建终端|新增終端/ }).click();
  await page.locator('.terminal-panel:visible[data-status="ready"]').waitFor();
  const tabs = page.locator('.terminal-tabstrip [role="tab"]');
  assert.ok((await tabs.count()) >= 2, "a second terminal tab");
  await tabs.first().click();
  await page.locator('.terminal-panel:visible[data-status="ready"]').waitFor();
  await page.waitForFunction(() => {
    const pane = [...document.querySelectorAll(".terminal-panel")].find((element) => element.getBoundingClientRect().width > 0);
    return (pane?.querySelector(".xterm-rows")?.textContent ?? "").trim().length > 0;
  }, null, { timeout: 10_000 });

  // A second tab shares the same profile socket instead of opening another.
  const second = await context.newPage();
  await second.goto(`${base}/?session=bus-1`);
  await second.waitForFunction(() => window.__piStarBus?.().mode === "bus");
  await second.waitForFunction(() => window.__piStarBus().channels.some((channel) => channel.channel.startsWith("agent:")));

  // The browser goes offline: the socket drops and the relay reconnects on
  // its own, resubscribing the same channels with their cursors.
  await context.setOffline(true);
  await delay(500);
  await context.setOffline(false);
  await page.waitForFunction(() => window.__piStarBus().state === "open", null, { timeout: 30_000 });
  await page.waitForFunction(() => window.__piStarBus().channels.some((channel) => channel.kind === "terminal"));
  assert.equal((await page.evaluate(() => window.__piStarBus())).sockets, 1, "one socket after a reconnect");

  const debug = await (await fetch(`${base}/api/agent/debug`)).json();
  assert.equal(debug.hub.connections, 1, "the server sees one connection for both tabs");
  assert.ok(debug.hub.channels >= 2, JSON.stringify(debug.hub.channelsByKind));
  assert.ok(debug.hub.channelsByKind.terminal >= 1, JSON.stringify(debug.hub.channelsByKind));
  assert.deepEqual(sse, [], "no per-feature SSE endpoint was requested");
  assert.equal(debug.server.sseTotal, 0, "the server has no SSE streams");
  assert.ok(debug.hub.subscribers >= 2, JSON.stringify(debug.hub.streamsByKind));
  assert.ok(debug.server.busTotal >= 2, JSON.stringify(debug.server));
  assert.equal(debug.server.streamTotal, debug.server.sseTotal + debug.server.busTotal);
  assert.ok(
    debug.server.pages.some((page) => page.agent >= 1 && page.terminal >= 1),
    JSON.stringify(debug.server.pages),
  );
  const detail = await (await fetch(`${base}/api/agent/debug?session=bus-1`)).json();
  const busStream = detail.session.eventStreams.find((stream) => stream.transport === "bus");
  assert.ok(busStream, JSON.stringify(detail.session.eventStreams));
  assert.equal(typeof busStream.openedAt, "number");
  assert.equal(typeof busStream.connectedAt, "number");
  assert.ok(busStream.holder, "the bus stream names the page that holds it");
  console.log("stream bus e2e: ok", JSON.stringify(debug.hub.channelsByKind));
} finally {
  await browser?.close().catch(() => {});
  server.kill("SIGTERM");
  await Promise.race([serverExit, delay(5_000)]);
}
