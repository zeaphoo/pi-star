// Starts the pi-star binary for the browser suites. pi-web's runner spawned
// `next dev`/`next start`; pi-star serves the already-built `ui/dist` itself.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createWriteStream, existsSync } from "node:fs";
import { join } from "node:path";

/** The repository root that holds `target/` and `ui/`. */
export function repoRoot(uiRoot) {
  return join(uiRoot, "..");
}

/** Playwright launch options; `PI_STAR_CHROMIUM` points at a system browser. */
export function chromiumOptions() {
  return {
    ...(process.env.PI_STAR_CHROMIUM ? { executablePath: process.env.PI_STAR_CHROMIUM } : {}),
    args: ["--no-sandbox"],
  };
}

export function resolveServerBinary(root) {
  const candidates = [
    process.env.PI_STAR_BIN,
    join(root, "target/release/pi-star"),
    join(root, "target/debug/pi-star"),
  ].filter(Boolean);
  const binary = candidates.find((candidate) => existsSync(candidate));
  assert.ok(binary, `pi-star binary not found (tried ${candidates.join(", ")}); run cargo build first`);
  return binary;
}

/**
 * Spawns pi-star on `port` with the fixture agent directory and waits for
 * `/api/sessions`. Returns the child, its log stream, and its exit promise.
 */
export async function startServer({ root, uiRoot, agentDir, port, artifacts, env = {} }) {
  const binary = resolveServerBinary(root);
  const log = createWriteStream(join(artifacts, "server.log"));
  const child = spawn(binary, [
    "--hostname", "127.0.0.1",
    "--port", String(port),
    "--ui-dir", join(uiRoot, "dist"),
    "--agent-dir", agentDir,
    "--no-open",
  ], {
    cwd: root,
    env: {
      ...process.env,
      PI_WEB_PASSWORD: "",
      ...env,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  child.stdout.pipe(log, { end: false });
  child.stderr.pipe(log, { end: false });
  const exit = once(child, "exit");
  return { child, log, exit, binary };
}
