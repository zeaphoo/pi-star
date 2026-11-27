import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const { formatDirectoryModified, sortDirectoryEntries } = await createJiti(import.meta.url).import("./directory-picker.ts");

const entries = [
  { name: "zeta", modifiedMs: 30 },
  { name: "Alpha", modifiedMs: 20 },
  { name: "beta", modifiedMs: 10 },
];

test("sorts folders by name or newest modification", () => {
  assert.deepEqual(sortDirectoryEntries(entries, "name", "en").map((entry) => entry.name), ["Alpha", "beta", "zeta"]);
  assert.deepEqual(sortDirectoryEntries(entries, "modified", "en").map((entry) => entry.name), ["zeta", "Alpha", "beta"]);
  assert.deepEqual(sortDirectoryEntries([{ name: "b", modifiedMs: 5 }, { name: "a", modifiedMs: 5 }], "modified", "en").map((entry) => entry.name), ["a", "b"]);
});

test("formats today as a clock time and earlier years as a date", () => {
  const now = new Date(2026, 0, 2, 15, 0);
  assert.equal(formatDirectoryModified(new Date(2026, 0, 2, 9, 5).getTime(), "en-US", now), "09:05 AM");
  assert.equal(formatDirectoryModified(new Date(2025, 10, 20, 9, 5).getTime(), "en-US", now), "11/20/2025");
  assert.equal(formatDirectoryModified(null, "en-US", now), "");
});
