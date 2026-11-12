import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { Script } from "node:vm";
import ts from "typescript";

const STORAGE_KEY = "pi-file-icon-style";
const MONO_CLASS = "file-icons-mono";
const source = readFileSync(new URL("./useFileIconStyle.ts", import.meta.url), "utf8");
const script = new Script(ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS },
}).outputText);

function fixture({ initial = {}, server = false } = {}) {
  const stored = new Map(Object.entries(initial));
  const classes = new Set();
  const exported = {};
  script.runInNewContext({
    exports: exported,
    require(id) {
      if (id === "react") return { useSyncExternalStore: (_subscribe, getSnapshot) => getSnapshot() };
      throw new Error(`Unexpected import ${id}`);
    },
    ...(!server && {
      window: {
        localStorage: {
          getItem(key) { return stored.get(key) ?? null; },
          setItem(key, value) { stored.set(key, value); },
          removeItem(key) { stored.delete(key); },
        },
        addEventListener() {},
        removeEventListener() {},
      },
      document: {
        documentElement: {
          classList: {
            toggle(name, on) { if (on) classes.add(name); else classes.delete(name); },
          },
        },
      },
    }),
  });
  return { ...exported, stored, classes };
}

test("server rendering uses the colorful default without touching the DOM", () => {
  const store = fixture({ server: true });
  assert.equal(store.useFileIconStyle().style, "colorful");
  assert.equal(store.useFileIconStyle().colorful, true);
});

test("a saved monochrome choice is applied when the app mounts", () => {
  const store = fixture({ initial: { [STORAGE_KEY]: "mono" } });
  assert.equal(store.useFileIconStyle().colorful, false);
  assert.ok(store.classes.has(MONO_CLASS));
});

test("the switch toggles the class and persists only the non-default value", () => {
  const store = fixture();
  assert.equal(store.useFileIconStyle().colorful, true);
  assert.equal(store.classes.has(MONO_CLASS), false);
  store.useFileIconStyle().setColorful(false);
  assert.ok(store.classes.has(MONO_CLASS));
  assert.equal(store.stored.get(STORAGE_KEY), "mono");
  store.useFileIconStyle().setColorful(true);
  assert.equal(store.classes.has(MONO_CLASS), false);
  assert.equal(store.stored.has(STORAGE_KEY), false, "the default is the absence of the key");
});