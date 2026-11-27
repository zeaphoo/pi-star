import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { resolvePiInvocation } from "./subagent.js";
import piStarSubagentChild from "./subagent-child.js";

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

test("project profiles load only when the server says the folder is trusted", async () => {
    const { mkdirSync, writeFileSync } = await import("node:fs");
    const { readProfiles } = await import("./subagent.js");
    const agentDir = mkdtempSync(join(tmpdir(), "pi-star-agents-"));
    const project = mkdtempSync(join(tmpdir(), "pi-star-project-"));
    const previousCwd = process.cwd();
    const previousAgentDir = process.env.PI_CODING_AGENT_DIR;
    const previousTrust = process.env.PI_STAR_PROJECT_TRUSTED;
    try {
        mkdirSync(join(project, ".pi", "agents"), { recursive: true });
        writeFileSync(
            join(project, ".pi", "agents", "review.md"),
            "---\nname: review\ndescription: review\n---\n\nReview the change.\n",
        );
        process.env.PI_CODING_AGENT_DIR = agentDir;
        process.chdir(project);

        process.env.PI_STAR_PROJECT_TRUSTED = "0";
        assert.equal(readProfiles().some((profile) => profile.name === "review"), false);

        process.env.PI_STAR_PROJECT_TRUSTED = "1";
        assert.equal(readProfiles().some((profile) => profile.name === "review"), true);
    } finally {
        process.chdir(previousCwd);
        if (previousAgentDir === undefined) delete process.env.PI_CODING_AGENT_DIR;
        else process.env.PI_CODING_AGENT_DIR = previousAgentDir;
        if (previousTrust === undefined) delete process.env.PI_STAR_PROJECT_TRUSTED;
        else process.env.PI_STAR_PROJECT_TRUSTED = previousTrust;
        rmSync(agentDir, { recursive: true, force: true });
        rmSync(project, { recursive: true, force: true });
    }
});

test("profile resources are read from frontmatter and default off", async () => {
    const { mkdirSync, writeFileSync } = await import("node:fs");
    const { readProfiles } = await import("./subagent.js");
    const agentDir = mkdtempSync(join(tmpdir(), "pi-star-agents-"));
    const project = mkdtempSync(join(tmpdir(), "pi-star-project-"));
    const previousCwd = process.cwd();
    const previousAgentDir = process.env.PI_CODING_AGENT_DIR;
    const previousTrust = process.env.PI_STAR_PROJECT_TRUSTED;
    try {
        mkdirSync(join(project, ".pi", "agents"), { recursive: true });
        writeFileSync(join(project, ".pi", "agents", "res.md"), [
            "---",
            "name: res",
            "description: resources",
            "load_skills: true",
            "skills: [review, testing]",
            "load_extensions: false",
            "mcp: on",
            "codemode: inherit",
            "prompt_mode: replace",
            "inherit_context: true",
            "run_in_background: true",
            "---",
            "",
            "Body.",
        ].join("\n"));
        process.env.PI_CODING_AGENT_DIR = agentDir;
        process.env.PI_STAR_PROJECT_TRUSTED = "1";
        process.chdir(project);

        const res = readProfiles().find((profile) => profile.name === "res");
        assert.ok(res);
        assert.equal(res.loadSkills, true);
        assert.deepEqual(res.skills, ["review", "testing"]);
        assert.equal(res.loadExtensions, false);
        assert.equal(res.mcp, "on");
        assert.equal(res.codemode, "inherit");
        assert.equal(res.promptMode, "replace");
        assert.equal(res.inheritContext, true);
        assert.equal(res.runInBackground, true);

        const builtin = readProfiles().find((profile) => profile.name === "general-purpose");
        assert.equal(builtin.loadSkills, false);
        assert.equal(builtin.loadExtensions, false);
        assert.equal(builtin.mcp, "off");
        assert.equal(builtin.codemode, "off");
        assert.equal(builtin.promptMode, "append");
        assert.equal(builtin.runInBackground, false);
    } finally {
        process.chdir(previousCwd);
        if (previousAgentDir === undefined) delete process.env.PI_CODING_AGENT_DIR;
        else process.env.PI_CODING_AGENT_DIR = previousAgentDir;
        if (previousTrust === undefined) delete process.env.PI_STAR_PROJECT_TRUSTED;
        else process.env.PI_STAR_PROJECT_TRUSTED = previousTrust;
        rmSync(agentDir, { recursive: true, force: true });
        rmSync(project, { recursive: true, force: true });
    }
});

function childHarness() {
    const handlers = new Map();
    const entries = [];
    let loading = true;
    const pi = {
        on(event, handler) {
            handlers.set(event, handler);
        },
        appendEntry(customType, data) {
            // pi rejects session-bound calls made while the factory is loading.
            if (loading) throw new Error("Extension runtime not initialized. Action methods cannot be called during extension loading.");
            entries.push({ customType, data });
        },
    };
    return { pi, handlers, entries, doneLoading: () => { loading = false; } };
}

function withSubagentEnv(meta, queued, run) {
    const previousMeta = process.env.PI_STAR_SUBAGENT_META;
    const previousQueued = process.env.PI_STAR_SUBAGENT_START_QUEUED;
    process.env.PI_STAR_SUBAGENT_META = JSON.stringify(meta);
    if (queued) process.env.PI_STAR_SUBAGENT_START_QUEUED = "1";
    try {
        run();
    } finally {
        if (previousMeta === undefined) delete process.env.PI_STAR_SUBAGENT_META;
        else process.env.PI_STAR_SUBAGENT_META = previousMeta;
        if (previousQueued === undefined) delete process.env.PI_STAR_SUBAGENT_START_QUEUED;
        else process.env.PI_STAR_SUBAGENT_START_QUEUED = previousQueued;
    }
}

test("the child writes its metadata on session_start, not during extension loading", () => {
    withSubagentEnv({ version: 1, parentSessionId: "parent" }, false, () => {
        const { pi, handlers, entries, doneLoading } = childHarness();
        piStarSubagentChild(pi);
        assert.deepEqual(entries, [], "appendEntry during loading aborts the RPC child");
        doneLoading();
        handlers.get("session_start")();
        assert.deepEqual(entries.map((entry) => entry.customType), ["pi-star:subagent", "pi-star:subagent-status"]);
        assert.equal(entries[1].data.status, "running");
        // A second session_start (reload) must not duplicate the header.
        handlers.get("session_start")();
        assert.equal(entries.length, 2);
    });
});

test("a queued child flips to running on its first agent_start", () => {
    withSubagentEnv({ version: 1, parentSessionId: "parent" }, true, () => {
        const { pi, handlers, entries, doneLoading } = childHarness();
        piStarSubagentChild(pi);
        doneLoading();
        handlers.get("session_start")();
        assert.equal(entries[1].data.status, "queued");
        handlers.get("agent_start")();
        assert.equal(entries[2].data.status, "running");
        handlers.get("agent_start")();
        assert.equal(entries.length, 3);
    });
});
