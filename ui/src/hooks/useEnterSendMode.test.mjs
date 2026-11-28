import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { Script } from "node:vm";
import ts from "typescript";

const STORAGE_KEY = "pi-enter-send-mode";
const source = readFileSync(new URL("./useEnterSendMode.ts", import.meta.url), "utf8");
const script = new Script(ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS },
}).outputText);

function fixture({ initial, refusesWrites = false } = {}) {
  const stored = new Map(initial === undefined ? [] : [[STORAGE_KEY, initial]]);
  const handlers = new Map();
  const exported = {};
  script.runInNewContext({
    exports: exported,
    require(id) {
      // As React does: subscribe on mount, re-read the snapshot.
      if (id === "react") {
        return { useSyncExternalStore: (subscribe, getSnapshot) => { subscribe(() => {}); return getSnapshot(); } };
      }
      throw new Error(`Unexpected import ${id}`);
    },
    window: {
      localStorage: {
        getItem(key) { return stored.get(key) ?? null; },
        setItem(key, value) {
          // A browser can refuse storage (private mode, quota); the choice
          // still applies in memory.
          if (refusesWrites) throw new Error("storage disabled");
          stored.set(key, value);
        },
        removeItem(key) { stored.delete(key); },
      },
      addEventListener(type, handler) { handlers.set(type, handler); },
      removeEventListener(type) { handlers.delete(type); },
    },
  });
  return { ...exported, stored, handlers };
}

test("Enter sends by default", () => {
  const store = fixture();
  assert.equal(store.useEnterSendMode(), "enter");
});

test("a stored choice is read on mount", () => {
  const store = fixture({ initial: "ctrlEnter" });
  assert.equal(store.useEnterSendMode(), "ctrlEnter");
});

test("changing the choice persists it", () => {
  const store = fixture();
  store.setEnterSendMode("ctrlEnter");
  assert.equal(store.useEnterSendMode(), "ctrlEnter");
  assert.equal(store.stored.get(STORAGE_KEY), "ctrlEnter");
});

test("a choice the browser refused to save still applies until a reload", () => {
  const store = fixture({ refusesWrites: true });
  store.setEnterSendMode("ctrlEnter");
  assert.equal(store.useEnterSendMode(), "ctrlEnter");
});

test("another tab's choice reaches this tab", () => {
  const store = fixture();
  assert.equal(store.useEnterSendMode(), "enter");
  // The tab subscribed when the composer mounted.
  assert.equal(store.useEnterSendMode(), "enter");
  const handler = store.handlers.get("storage");
  assert.ok(handler, "the store listens for storage events");
  store.stored.set(STORAGE_KEY, "ctrlEnter");
  handler({ key: STORAGE_KEY });
  assert.equal(store.useEnterSendMode(), "ctrlEnter");
  // An unrelated key changes nothing.
  handler({ key: "something-else" });
  assert.equal(store.useEnterSendMode(), "ctrlEnter");
});