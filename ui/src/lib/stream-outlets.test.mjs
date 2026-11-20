import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

// One socket per browser profile is a guarantee, not a convention: only the
// stream bus modules may construct one. A new feature subscribes to a channel.

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const allowed = new Set(["lib/stream-bus.ts", "lib/stream-relay.ts", "lib/stream-worker.ts"]);
const constructors = [/new\s+EventSource\s*\(/, /new\s+WebSocket\s*\(/, /new\s+SharedWorker\s*\(/];

function sourceFiles(dir) {
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "node_modules" || entry.name === "__fixtures__") continue;
      files.push(...sourceFiles(full));
    } else if (/\.(ts|tsx)$/.test(entry.name)) {
      files.push(full);
    }
  }
  return files;
}

test("long-lived connection constructors live only in the stream bus", () => {
  const offenders = [];
  for (const file of sourceFiles(root)) {
    const relative = path.relative(root, file).split(path.sep).join("/");
    if (allowed.has(relative)) continue;
    const text = readFileSync(file, "utf8");
    for (const pattern of constructors) {
      if (pattern.test(text)) offenders.push(`${relative} matches ${pattern}`);
    }
  }
  assert.deepEqual(offenders, [], offenders.join("\n"));
});
