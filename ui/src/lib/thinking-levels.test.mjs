import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url);
const {
  CONCRETE_THINKING_LEVEL_IDS,
  DEFAULT_THINKING_LEVEL,
  THINKING_LEVEL_IDS,
  isConcreteThinkingLevel,
} = await jiti.import("./thinking-levels.ts");

test("concrete levels are every id except auto", () => {
  assert.deepEqual(
    [...CONCRETE_THINKING_LEVEL_IDS],
    THINKING_LEVEL_IDS.filter((id) => id !== "auto"),
  );
  assert.equal(DEFAULT_THINKING_LEVEL, "medium");
});

test("isConcreteThinkingLevel rejects auto and unknown values", () => {
  for (const level of CONCRETE_THINKING_LEVEL_IDS) assert.equal(isConcreteThinkingLevel(level), true);
  assert.equal(isConcreteThinkingLevel("auto"), false);
  assert.equal(isConcreteThinkingLevel(undefined), false);
  assert.equal(isConcreteThinkingLevel(""), false);
});
