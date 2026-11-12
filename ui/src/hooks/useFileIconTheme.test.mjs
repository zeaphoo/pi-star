import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { Script } from "node:vm";
import ts from "typescript";

const STORAGE_KEY = "pi-file-icon-theme";
const source = readFileSync(new URL("./useFileIconTheme.ts", import.meta.url), "utf8");
const script = new Script(ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS },
}).outputText);

function fixture({ initial = {}, server = false } = {}) {
  const stored = new Map(Object.entries(initial));
  const events = new Map();
  const exported = {};
  script.runInNewContext({
    exports: exported,
    require(id) {
      if (id === "react") return { useSyncExternalStore: (_subscribe, getSnapshot) => getSnapshot() };
      if (id === "@/lib/icon-themes") {
        return { ICON_THEMES: [{ id: "catppuccin" }, { id: "material" }, { id: "seti" }, { id: "vscode" }] };
      }
      throw new Error(`Unexpected import ${id}`);
    },
    ...(!server && {
      window: {
        localStorage: {
          getItem(key) { return stored.get(key) ?? null; },
          setItem(key, value) { stored.set(key, value); },
          removeItem(key) { stored.delete(key); },
        },
        addEventListener(type, listener) { events.set(type, listener); },
        removeEventListener(type, listener) { if (events.get(type) === listener) events.delete(type); },
      },
    }),
  });
  return { ...exported, stored, events };
}

test("server rendering uses the Catppuccin default", () => {
  const store = fixture({ server: true });
  assert.equal(store.useFileIconTheme().theme, "catppuccin");
});

test("a saved theme is applied when the tree mounts", () => {
  const store = fixture({ initial: { [STORAGE_KEY]: "material" } });
  assert.equal(store.useFileIconTheme().theme, "material");
});

test("the selector persists a choice and removes the default key", () => {
  const store = fixture();
  assert.equal(store.useFileIconTheme().theme, "catppuccin");
  store.useFileIconTheme().setTheme("seti");
  assert.equal(store.stored.get(STORAGE_KEY), "seti");
  assert.equal(store.useFileIconTheme().theme, "seti");
  store.useFileIconTheme().setTheme("catppuccin");
  assert.equal(store.stored.has(STORAGE_KEY), false);
});

test("an unknown stored value falls back to the default", () => {
  const store = fixture({ initial: { [STORAGE_KEY]: "unknown" } });
  assert.equal(store.useFileIconTheme().theme, "catppuccin");
});