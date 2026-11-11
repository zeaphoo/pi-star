// Bridge between pi-star's SSE login protocol and pi-ai's provider OAuth
// implementations. Run from the pi package directory so
// `@earendil-works/pi-ai/providers/all` resolves to the same runtime the
// standard pi backend uses.
//
// stdout carries one JSON event per line:
//   {type:"auth"|"device_code"|"progress", ...}
//   {type:"prompt_request"|"select_request", ..., token}
//   {type:"success", credential}
//   {type:"error", message}
// stdin carries `{token, code}` replies for the pending prompt.

import { createInterface } from "node:readline";

const providerId = process.argv[2] ?? process.argv[1];
const send = (value) => process.stdout.write(`${JSON.stringify(value)}\n`);

let builtinProviders;
try {
  ({ builtinProviders } = await import("@earendil-works/pi-ai/providers/all"));
} catch (error) {
  send({ type: "error", message: `OAuth requires the bundled pi runtime: ${error instanceof Error ? error.message : error}` });
  process.exit(0);
}

const provider = builtinProviders().find((entry) => entry.id === providerId);
const oauth = provider?.auth?.oauth;
if (!oauth) {
  send({ type: "error", message: `OAuth is not available for ${providerId}` });
  process.exit(0);
}

const pending = new Map();
let sequence = 0;

createInterface({ input: process.stdin }).on("line", (line) => {
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    return;
  }
  const settle = pending.get(message.token);
  if (!settle) return;
  pending.delete(message.token);
  settle(typeof message.code === "string" ? message.code : "");
});

function promptUser(prompt) {
  return new Promise((resolve, reject) => {
    const token = `oauth-${providerId}-${++sequence}`;
    if (prompt.signal?.aborted) {
      reject(new Error("Login cancelled"));
      return;
    }
    const onAbort = () => {
      if (pending.delete(token)) reject(new Error("Login cancelled"));
    };
    prompt.signal?.addEventListener("abort", onAbort, { once: true });
    pending.set(token, (value) => {
      prompt.signal?.removeEventListener("abort", onAbort);
      resolve(value);
    });
    if (prompt.type === "select") {
      send({ type: "select_request", message: prompt.message, options: prompt.options, token });
    } else {
      send({ type: "prompt_request", message: prompt.message, placeholder: prompt.placeholder, token });
    }
  });
}

const interaction = {
  // The Rust side kills this process when the browser closes the SSE stream.
  signal: new AbortController().signal,
  prompt: promptUser,
  notify(event) {
    if (event.type === "auth_url") {
      send({ type: "auth", url: event.url, instructions: event.instructions ?? null, token: "" });
    } else if (event.type === "device_code") {
      send({ type: "device_code", ...event });
    } else if (event.type === "progress") {
      send({ type: "progress", message: event.message });
    } else if (event.type === "info") {
      const links = (event.links ?? [])
        .map((link) => (link.label ? `${link.label}: ${link.url}` : link.url))
        .join("\n");
      send({ type: "progress", message: [event.message, links].filter(Boolean).join("\n") });
    }
  },
};

try {
  const credential = await oauth.login(interaction, {
    getDeviceId: process.env.PI_STAR_DEVICE_ID || undefined,
  });
  send({ type: "success", credential });
} catch (error) {
  send({ type: "error", message: error instanceof Error ? error.message : String(error) });
}