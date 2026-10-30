// @ts-nocheck
/**
 * Minimal agent-side subagent tools for the PiRpcBackend.
 *
 * A child is a real `pi --mode rpc` process with its own session, so it can be
 * inspected from the session list. The extension owns the child while the
 * parent run is alive; `get_subagent_result` and `steer_subagent` address it by
 * session id.
 */
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const KEY = Symbol.for("pi-star.subagent.runs");
const TERMINAL = new Set(["completed", "failed", "aborted", "interrupted"]);
const DEFAULT_WAIT_MS = 30 * 60 * 1000;

type Run = {
  id: string;
  profile: string;
  description: string;
  status: string;
  createdAt: number;
  completedAt?: number;
  result?: string;
  error?: string;
  runInBackground: boolean;
  proc: any;
  stdin: any;
  buffer: string;
  waiters: Array<() => void>;
};

function runs(): Map<string, Run> {
  const global = globalThis as Record<PropertyKey, any>;
  if (!global[KEY]) global[KEY] = new Map<string, Run>();
  return global[KEY];
}

function textOf(message: any): string {
  const content = message?.content;
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content.filter((block) => block?.type === "text" && typeof block.text === "string")
    .map((block) => block.text).join("\n");
}

function notify(run: Run) {
  for (const waiter of run.waiters.splice(0)) waiter();
}

function settle(run: Run, status: string, result?: string, error?: string) {
  if (TERMINAL.has(run.status)) return;
  run.status = status;
  run.completedAt = Date.now();
  if (result !== undefined) run.result = result;
  if (error !== undefined) run.error = error;
  notify(run);
}

function lineLoop(run: Run) {
  let buffer = "";
  run.proc.stdout.on("data", (chunk: Buffer) => {
    buffer += chunk.toString("utf8");
    while (true) {
      const index = buffer.indexOf("\n");
      if (index < 0) break;
      const line = buffer.slice(0, index).replace(/\r$/, "");
      buffer = buffer.slice(index + 1);
      if (!line.trim()) continue;
      let event: any;
      try { event = JSON.parse(line); } catch { continue; }
      if (event.type === "response" && event.success === false) {
        settle(run, "failed", undefined, event.error || "subagent command failed");
      } else if (event.type === "message_end" && event.message?.role === "assistant") {
        const text = textOf(event.message).trim();
        if (text) run.result = text;
      } else if (event.type === "agent_settled") {
        settle(run, "completed", run.result ?? "");
      } else if (event.type === "extension_error") {
        run.error = event.error;
      }
    }
  });
  run.proc.on("error", (error: Error) => settle(run, "failed", undefined, error.message));
  run.proc.on("close", (code: number | null) => {
    if (!TERMINAL.has(run.status)) {
      settle(run, code === 0 ? "completed" : "failed", run.result, code === 0 ? undefined : `subagent exited with code ${code}`);
    }
  });
}

function profiles(): Array<{ name: string; description: string; body: string }> {
  const roots = [
    join(homedir(), ".pi", "agent", "agents"),
    join(process.cwd(), ".pi", "agents"),
  ];
  const out: Array<{ name: string; description: string; body: string }> = [];
  for (const root of roots) {
    if (!existsSync(root)) continue;
    for (const name of readdirSync(root)) {
      if (!name.endsWith(".md")) continue;
      try {
        const text = readFileSync(join(root, name), "utf8");
        const match = text.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/);
        const meta = match?.[1] ?? "";
        const body = (match?.[2] ?? "").trim();
        const field = (key: string) => meta.match(new RegExp(`^${key}:\\s*(.+)$`, "m"))?.[1]?.trim().replace(/^["']|["']$/g, "");
        out.push({
          name: field("name") || name.slice(0, -3),
          description: field("description") || "",
          body,
        });
      } catch {
        // unreadable profiles are ignored
      }
    }
  }
  return out;
}

function profilePrompt(profile: string): { name: string; prefix: string } {
  const found = profiles().find((item) => item.name === profile);
  if (!found) return { name: profile || "general-purpose", prefix: "" };
  return {
    name: found.name,
    prefix: found.body ? `${found.body}\n\n---\n\n` : "",
  };
}

function details(run: Run) {
  return {
    kind: "pi-web-subagent",
    sessionId: run.id,
    profile: run.profile,
    description: run.description,
    status: run.status,
    runInBackground: run.runInBackground,
    createdAt: run.createdAt,
    ...(run.completedAt ? { completedAt: run.completedAt } : {}),
    ...(run.error ? { error: run.error } : {}),
  };
}

function finalText(run: Run): string {
  if (!TERMINAL.has(run.status)) return `Subagent ${run.id} is ${run.status}.`;
  if (run.status === "completed") {
    return run.result?.trim()
      ? `Subagent ${run.id} completed.\n\n${run.result.trim()}`
      : `Subagent ${run.id} completed without text output.`;
  }
  return `Subagent ${run.id} ${run.status}: ${run.error ?? "Unknown error"}`;
}

function startChild(pi: any, params: any, runInBackground: boolean): Run {
  const id = randomUUID();
  const profile = String(params.subagent_type || "general-purpose");
  const description = String(params.description || "Subagent");
  const prompt = profilePrompt(profile);
  const cli = process.env.PI_STAR_PI_CLI || "pi";
  const args = ["--mode", "rpc", "--session-id", id, "--name", description.slice(0, 80)];
  if (typeof params.model === "string" && params.model.trim()) {
    args.push("--model", params.model.trim());
  }
  const proc = spawn(cli, args, {
    cwd: process.cwd(),
    env: process.env,
    stdio: ["pipe", "pipe", "pipe"],
  });
  const run: Run = {
    id,
    profile: profile.name,
    description,
    status: "running",
    createdAt: Date.now(),
    runInBackground,
    proc,
    stdin: proc.stdin,
    buffer: "",
    waiters: [],
  };
  runs().set(id, run);
  lineLoop(run);
  proc.stderr?.on("data", () => {});
  try {
    proc.stdin.write(`${JSON.stringify({
      id: `prompt-${id}`,
      type: "prompt",
      message: `${prompt.prefix}${String(params.prompt ?? "")}`,
    })}\n`);
  } catch (error) {
    settle(run, "failed", undefined, error instanceof Error ? error.message : String(error));
  }
  if (runInBackground) {
    run.waiters.push(() => {
      try {
        pi.sendMessage?.({
          customType: "pi-web:subagent-notification",
          content: finalText(run),
          display: true,
          details: details(run),
        }, { deliverAs: "followUp" });
      } catch {
        // The parent may have shut down.
      }
    });
  }
  return run;
}

function waitFor(run: Run, timeoutMs: number): Promise<Run> {
  if (TERMINAL.has(run.status)) return Promise.resolve(run);
  return new Promise((resolve) => {
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      resolve(run);
    };
    const timer = setTimeout(finish, Math.max(1, timeoutMs));
    run.waiters.push(finish);
  });
}

export function registerSubagentTools(pi: any) {
  pi.registerTool({
    name: "Agent",
    label: "Agent",
    description: "Delegate a focused task to a subagent. Each subagent runs as a separate inspectable Pi session. Use background mode for independent work and foreground mode when the result is needed immediately.",
    parameters: {
      type: "object",
      properties: {
        subagent_type: { type: "string", description: "Configured agent profile name, if any." },
        prompt: { type: "string", description: "The complete task for the subagent." },
        description: { type: "string", description: "Short activity label shown in the UI." },
        run_in_background: { type: "boolean", description: "Return immediately and notify this session when complete." },
        wait_timeout: { type: "number", description: "Seconds to wait for a foreground subagent. Default: 1800." },
        model: { type: "string", description: "Optional provider/model override." },
      },
      required: ["prompt", "description"],
    },
    async execute(_toolCallId: string, params: any) {
      const background = params.run_in_background === true;
      const run = startChild(pi, params, background);
      if (background) {
        return {
          content: [{ type: "text", text: `Subagent started in background. Session ID: ${run.id}. Its report arrives when it completes.` }],
          details: details(run),
        };
      }
      const timeoutMs = Number.isFinite(params.wait_timeout) && params.wait_timeout > 0
        ? Number(params.wait_timeout) * 1000
        : DEFAULT_WAIT_MS;
      await waitFor(run, timeoutMs);
      return {
        content: [{ type: "text", text: finalText(run) }],
        details: details(run),
        ...(run.status === "failed" ? { isError: true } : {}),
      };
    },
  });

  pi.registerTool({
    name: "get_subagent_result",
    label: "Get agent result",
    description: "Retrieve a subagent session's latest result.",
    parameters: {
      type: "object",
      properties: {
        agent_id: { type: "string", description: "Subagent session ID." },
        wait: { type: "boolean", description: "Wait for completion." },
        wait_timeout: { type: "number", description: "Seconds to wait. Default: 1800." },
      },
      required: ["agent_id"],
    },
    async execute(_toolCallId: string, params: any) {
      const run = runs().get(String(params.agent_id));
      if (!run) return { content: [{ type: "text", text: `Subagent not found: ${params.agent_id}` }], details: undefined, isError: true };
      if (params.wait) {
        const timeoutMs = Number.isFinite(params.wait_timeout) && params.wait_timeout > 0
          ? Number(params.wait_timeout) * 1000
          : DEFAULT_WAIT_MS;
        await waitFor(run, timeoutMs);
      }
      return {
        content: [{ type: "text", text: finalText(run) }],
        details: details(run),
        ...(run.status === "failed" ? { isError: true } : {}),
      };
    },
  });

  pi.registerTool({
    name: "steer_subagent",
    label: "Steer agent",
    description: "Send a steering message to a currently running subagent.",
    parameters: {
      type: "object",
      properties: {
        agent_id: { type: "string", description: "Subagent session ID." },
        message: { type: "string", description: "Instruction to inject after the current tool execution." },
      },
      required: ["agent_id", "message"],
    },
    async execute(_toolCallId: string, params: any) {
      const run = runs().get(String(params.agent_id));
      if (!run || TERMINAL.has(run.status)) {
        return { content: [{ type: "text", text: `Subagent not running: ${params.agent_id}` }], details: undefined, isError: true };
      }
      try {
        run.stdin.write(`${JSON.stringify({ type: "steer", message: String(params.message) })}\n`);
        return { content: [{ type: "text", text: `Steering message sent to ${params.agent_id}.` }], details: undefined };
      } catch (error) {
        return { content: [{ type: "text", text: error instanceof Error ? error.message : String(error) }], details: undefined, isError: true };
      }
    },
  });
}
