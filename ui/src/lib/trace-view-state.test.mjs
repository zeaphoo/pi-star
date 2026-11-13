import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url);
const {
  TRACE_DETAIL_DEFAULT_WIDTH,
  TRACE_DETAIL_MAX_WIDTH,
  TRACE_DETAIL_MIN_WIDTH,
  TRACE_VIEW_STORAGE_KEY,
  clampTraceDetailWidth,
  emptyTraceSessionState,
  getTraceSessionState,
  isTraceDetailTab,
  loadTraceViewPreferences,
  normalizeTraceFilter,
  patchTraceSessionState,
  resetTraceViewStateForTests,
  saveTraceViewPreferences,
} = await jiti.import("./trace-view-state.ts");

// --- a localStorage that the tests own --------------------------------------

class MemoryStorage {
  #items = new Map();
  getItem(key) {
    return this.#items.has(key) ? this.#items.get(key) : null;
  }
  setItem(key, value) {
    this.#items.set(key, String(value));
  }
  removeItem(key) {
    this.#items.delete(key);
  }
  clear() {
    this.#items.clear();
  }
}

function withStorage(run) {
  const previous = globalThis.window;
  const storage = new MemoryStorage();
  globalThis.window = { localStorage: storage };
  try {
    return run(storage);
  } finally {
    if (previous === undefined) delete globalThis.window;
    else globalThis.window = previous;
  }
}

// --- per-session state ------------------------------------------------------

test("a session with nothing remembered starts empty", () => {
  resetTraceViewStateForTests();
  assert.deepEqual(getTraceSessionState("s1"), emptyTraceSessionState());
  assert.deepEqual(getTraceSessionState("s1"), {
    anchor: null,
    selectedKey: null,
    detailTab: "overview",
  });
});

test("patching keeps the other fields and survives a read", () => {
  resetTraceViewStateForTests();
  patchTraceSessionState("s1", { selectedKey: "e1#call-0" });
  const next = patchTraceSessionState("s1", { detailTab: "timing" });
  assert.deepEqual(next, { anchor: null, selectedKey: "e1#call-0", detailTab: "timing" });
  assert.deepEqual(getTraceSessionState("s1"), next);
});

test("sessions are remembered separately", () => {
  resetTraceViewStateForTests();
  patchTraceSessionState("s1", { selectedKey: "a" });
  patchTraceSessionState("s2", { selectedKey: "b" });
  assert.equal(getTraceSessionState("s1").selectedKey, "a");
  assert.equal(getTraceSessionState("s2").selectedKey, "b");
});

test("the oldest session is dropped once the cap is reached", () => {
  resetTraceViewStateForTests();
  for (let index = 0; index < 40; index += 1) {
    patchTraceSessionState(`s${index}`, { selectedKey: `k${index}` });
  }
  // The 24 most recent survive; earlier ones fall back to empty.
  assert.equal(getTraceSessionState("s39").selectedKey, "k39");
  assert.equal(getTraceSessionState("s16").selectedKey, "k16");
  assert.equal(getTraceSessionState("s15").selectedKey, null);
  assert.equal(getTraceSessionState("s0").selectedKey, null);
});

test("reading a session makes it the most recently used", () => {
  resetTraceViewStateForTests();
  for (let index = 0; index < 24; index += 1) {
    patchTraceSessionState(`s${index}`, { selectedKey: `k${index}` });
  }
  // Touch s0, then add one more: s1 is now the oldest and should go.
  getTraceSessionState("s0");
  patchTraceSessionState("new", { selectedKey: "kn" });
  assert.equal(getTraceSessionState("s0").selectedKey, "k0");
  assert.equal(getTraceSessionState("s1").selectedKey, null);
});

test("an anchor round-trips through the store", () => {
  resetTraceViewStateForTests();
  patchTraceSessionState("s1", { anchor: { key: "e9#assistant", offset: -12 } });
  assert.deepEqual(getTraceSessionState("s1").anchor, { key: "e9#assistant", offset: -12 });
});

// --- validation -------------------------------------------------------------

test("isTraceDetailTab accepts only the five tabs", () => {
  for (const tab of ["overview", "args", "result", "schema", "timing"]) {
    assert.equal(isTraceDetailTab(tab), true);
  }
  for (const value of ["Overview", "", null, undefined, 1, {}, "other"]) {
    assert.equal(isTraceDetailTab(value), false);
  }
});

test("clampTraceDetailWidth keeps the panel usable", () => {
  assert.equal(clampTraceDetailWidth(TRACE_DETAIL_MIN_WIDTH - 100), TRACE_DETAIL_MIN_WIDTH);
  assert.equal(clampTraceDetailWidth(TRACE_DETAIL_MAX_WIDTH + 100), TRACE_DETAIL_MAX_WIDTH);
  assert.equal(clampTraceDetailWidth(420), 420);
  assert.equal(clampTraceDetailWidth(420.6), 421);
  // Anything unreadable falls back to the default rather than to 0.
  assert.equal(clampTraceDetailWidth(null), TRACE_DETAIL_DEFAULT_WIDTH);
  assert.equal(clampTraceDetailWidth("500"), TRACE_DETAIL_DEFAULT_WIDTH);
  assert.equal(clampTraceDetailWidth(Number.NaN), TRACE_DETAIL_DEFAULT_WIDTH);
});

test("a missing or broken filter means show everything, never hide the tools", () => {
  const all = { duration: true, turns: true, tools: true, subcalls: true };
  assert.deepEqual(normalizeTraceFilter(undefined), all);
  assert.deepEqual(normalizeTraceFilter(null), all);
  assert.deepEqual(normalizeTraceFilter("nope"), all);
  assert.deepEqual(normalizeTraceFilter([]), all);
  assert.deepEqual(normalizeTraceFilter({}), all);
  // An explicit false is honoured; a non-boolean is not taken as false.
  assert.deepEqual(normalizeTraceFilter({ tools: false }), { ...all, tools: false });
  assert.deepEqual(normalizeTraceFilter({ tools: "false" }), all);
  assert.deepEqual(normalizeTraceFilter({ duration: false, subcalls: false }), {
    ...all,
    duration: false,
    subcalls: false,
  });
});

// --- preferences ------------------------------------------------------------

test("preferences round-trip through storage", () => {
  withStorage(() => {
    const written = { filter: { duration: false, turns: true, tools: false, subcalls: true }, detailWidth: 512 };
    saveTraceViewPreferences(written);
    assert.deepEqual(loadTraceViewPreferences(), written);
  });
});

test("with no stored preference everything is shown at the default width", () => {
  withStorage(() => {
    assert.deepEqual(loadTraceViewPreferences(), {
      filter: { duration: true, turns: true, tools: true, subcalls: true },
      detailWidth: TRACE_DETAIL_DEFAULT_WIDTH,
    });
  });
});

test("unreadable storage falls back instead of throwing", () => {
  withStorage((storage) => {
    storage.setItem(TRACE_VIEW_STORAGE_KEY, "{not json");
    assert.deepEqual(loadTraceViewPreferences(), {
      filter: { duration: true, turns: true, tools: true, subcalls: true },
      detailWidth: TRACE_DETAIL_DEFAULT_WIDTH,
    });
    storage.setItem(TRACE_VIEW_STORAGE_KEY, "null");
    assert.equal(loadTraceViewPreferences().detailWidth, TRACE_DETAIL_DEFAULT_WIDTH);
    storage.setItem(TRACE_VIEW_STORAGE_KEY, '"a string"');
    assert.equal(loadTraceViewPreferences().detailWidth, TRACE_DETAIL_DEFAULT_WIDTH);
  });
});

test("a partially written preference keeps the valid half", () => {
  withStorage((storage) => {
    storage.setItem(TRACE_VIEW_STORAGE_KEY, JSON.stringify({ detailWidth: 99999 }));
    const loaded = loadTraceViewPreferences();
    assert.equal(loaded.detailWidth, TRACE_DETAIL_MAX_WIDTH);
    assert.deepEqual(loaded.filter, { duration: true, turns: true, tools: true, subcalls: true });
  });
});

test("a storage that throws on write does not break the view", () => {
  const previous = globalThis.window;
  globalThis.window = {
    localStorage: {
      getItem() {
        return null;
      },
      setItem() {
        throw new Error("quota");
      },
    },
  };
  try {
    assert.doesNotThrow(() =>
      saveTraceViewPreferences({ filter: { duration: true, turns: true, tools: true, subcalls: true }, detailWidth: 420 }),
    );
  } finally {
    if (previous === undefined) delete globalThis.window;
    else globalThis.window = previous;
  }
});