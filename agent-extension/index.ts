// @ts-nocheck
/**
 * pi-star agent-side host extension.
 *
 * Loaded by `pi --mode rpc --extension <this file>`. It adds the few host
 * operations pi's JSONL RPC does not expose (get/set active tools, tree
 * navigation, branch fork, reload) and reports their results to the Rust
 * control channel.
 */
import net from "node:net";
import { registerSubagentTools } from "./subagent.ts";

type Pending = { id: string; method: string };
const KEY = Symbol.for("pi-star.agent-extension.control");

function state() {
  const global = globalThis as Record<PropertyKey, any>;
  if (!global[KEY]) {
    global[KEY] = { socket: null as net.Socket | null, buffer: "", connecting: false };
  }
  return global[KEY];
}

function connect() {
  const store = state();
  if (store.socket && !store.socket.destroyed) return;
  if (store.connecting) return;
  const address = process.env.PI_STAR_CONTROL_ADDR;
  const token = process.env.PI_STAR_CONTROL_TOKEN;
  if (!address || !token) return;
  const split = address.lastIndexOf(":");
  if (split < 0) return;
  const host = address.slice(0, split);
  const port = Number(address.slice(split + 1));
  if (!Number.isFinite(port)) return;
  store.connecting = true;
  const socket = net.createConnection({ host, port }, () => {
    store.connecting = false;
    socket.write(`${JSON.stringify({ type: "hello", token })}\n`);
  });
  store.socket = socket;
  socket.setNoDelay(true);
  socket.on("data", (chunk: Buffer) => {
    store.buffer += chunk.toString("utf8");
    while (true) {
      const index = store.buffer.indexOf("\n");
      if (index < 0) break;
      const line = store.buffer.slice(0, index).replace(/\r$/, "");
      store.buffer = store.buffer.slice(index + 1);
      if (line.trim()) {
        try {
          JSON.parse(line);
        } catch {
          // The control channel only carries JSON; ignore malformed records.
        }
      }
    }
  });
  socket.on("error", () => {
    store.connecting = false;
  });
  socket.on("close", () => {
    store.connecting = false;
    store.socket = null;
    // A reload tears down the extension runtime; reconnect for the next one.
    setTimeout(() => connect(), 250).unref?.();
  });
}

function respond(id: string, success: boolean, data?: unknown, error?: string) {
  connect();
  const store = state();
  if (!store.socket || store.socket.destroyed) return;
  store.socket.write(`${JSON.stringify({ type: "response", id, success, ...(success ? { data } : { error }) })}\n`);
}

function toolList(pi: any) {
  const active = new Set(pi.getActiveTools?.() ?? []);
  return (pi.getAllTools?.() ?? []).map((tool: any) => ({
    name: tool.name,
    label: tool.label ?? tool.name,
    description: tool.description ?? "",
    active: active.has(tool.name),
    parameters: tool.parameters ?? { type: "object", properties: {} },
    exposure: tool.exposure,
    namespace: tool.namespace,
    annotations: tool.annotations,
    sourceInfo: tool.sourceInfo,
  }));
}

function parseArgs(args: string): any {
  const start = args.indexOf("{");
  if (start < 0) throw new Error("Expected a JSON request");
  return JSON.parse(args.slice(start));
}

export default function piStarHost(pi: any) {
  connect();
  registerSubagentTools(pi);
  let compacting = false;
  pi.on("session_before_compact", () => { compacting = true; });
  pi.on("session_compact", () => { compacting = false; });
  pi.on("session_compact_failed", () => { compacting = false; });
  pi.registerCommand("pi-star-host", {
    description: "Internal: pi-star host control bridge",
    handler: async (args: string, ctx: any) => {
      let request: any;
      try {
        request = parseArgs(args);
      } catch (error) {
        return;
      }
      const id = String(request.id ?? "");
      const method = String(request.method ?? "");
      const params = request.params ?? {};
      try {
        let data: unknown = {};
        switch (method) {
          case "get_tools":
            data = toolList(pi);
            break;
          case "set_tools":
            pi.setActiveTools(Array.isArray(params.toolNames) ? params.toolNames : []);
            data = toolList(pi);
            break;
          case "navigate_tree":
            data = await ctx.navigateTree(String(params.targetId), {
              ...(params.summarize === undefined ? {} : { summarize: params.summarize }),
              ...(params.customInstructions === undefined ? {} : { customInstructions: params.customInstructions }),
              ...(params.replaceInstructions === undefined ? {} : { replaceInstructions: params.replaceInstructions }),
              ...(params.label === undefined ? {} : { label: params.label }),
            });
            break;
          case "fork_branch":
            data = await ctx.fork(String(params.entryId), params.position === "before" || params.position === "at"
              ? { position: params.position }
              : {});
            break;
          case "reload":
            respond(id, true, {});
            await ctx.reload();
            return;
          case "abort_compaction":
            // The SDK exposes abortCompaction() to its own TUI, but not to
            // extensions. Abort only while a compaction is actually running;
            // otherwise leave the current agent run alone.
            if (compacting) ctx.abort();
            data = {};
            break;
          default:
            throw new Error(`Unknown pi-star host method: ${method}`);
        }
        respond(id, true, data);
      } catch (error) {
        respond(id, false, undefined, error instanceof Error ? error.message : String(error));
      }
    },
  });
}
