import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { resolvePiInvocation } from "./subagent.js";

const args = ["--mode", "rpc", "--session-id", "child"];

test("PI_STAR_PI_CLI wins over the bundled runtime", () => {
    const invocation = resolvePiInvocation(
        args,
        { PI_STAR_PI_CLI: "/opt/pi", PI_STAR_PI_NODE: "/runtime/node", PI_STAR_PI_JS: "/runtime/cli.js" },
        { execPath: "/usr/bin/node", script: "/runtime/cli.js" },
    );
    assert.deepEqual(invocation, { command: "/opt/pi", args });
});

test("the bundled runtime starts node with cli.js", () => {
    const invocation = resolvePiInvocation(
        args,
        { PI_STAR_PI_NODE: "/runtime/node", PI_STAR_PI_JS: "/runtime/pi/dist/bundle/cli.js" },
        { execPath: "/usr/bin/node", script: undefined },
    );
    assert.deepEqual(invocation, {
        command: "/runtime/node",
        args: ["/runtime/pi/dist/bundle/cli.js", ...args],
    });
});

test("a node parent reuses its own script", () => {
    const dir = mkdtempSync(join(tmpdir(), "pi-star-subagent-"));
    try {
        const script = join(dir, "cli.js");
        writeFileSync(script, "");
        const invocation = resolvePiInvocation(args, {}, { execPath: "/usr/bin/node", script });
        assert.deepEqual(invocation, { command: "/usr/bin/node", args: [script, ...args] });
    } finally {
        rmSync(dir, { recursive: true, force: true });
    }
});

test("a standalone pi binary re-runs itself", () => {
    const invocation = resolvePiInvocation(
        args,
        {},
        { execPath: "/usr/local/bin/pi", script: "/usr/local/bin/pi" },
    );
    assert.deepEqual(invocation, { command: "/usr/local/bin/pi", args });
});

test("a generic runtime without a script falls back to PATH", () => {
    const invocation = resolvePiInvocation(args, {}, { execPath: "/usr/bin/node", script: undefined });
    assert.deepEqual(invocation, { command: "pi", args });
});

test("a Bun virtual script falls back to PATH like pi's own subagent example", () => {
    const invocation = resolvePiInvocation(
        args,
        {},
        { execPath: "/usr/bin/bun", script: "/$bunfs/root/pi" },
    );
    assert.deepEqual(invocation, { command: "pi", args });
});
