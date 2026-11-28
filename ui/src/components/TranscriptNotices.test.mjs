import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url, {
  jsx: { runtime: "automatic" },
  tsconfigPaths: true,
});
const { transcriptNoticeText } = await jiti.import("./TranscriptNotices.tsx");

/** The page's translator, with only the keys these notices use filled in. */
function translate(key, params = {}) {
  const table = {
    "transcriptNotice.costSuffix": " (~{cost})",
    "transcriptNotice.cacheMiss": "Cache miss: {tokens} tokens re-billed{cost}",
    "transcriptNotice.cacheWarmed": "Cache warmed{note}: {cost}",
    "transcriptNotice.compactionCost": "Compaction: {tokens} tokens billed{cost}",
    "transcriptNotice.branchSummaryCost": "Branch summary: {tokens} tokens billed{cost}",
    "transcriptNotice.thinkingDroppedOne": "Anthropic dropped 1 thinking block{reasons} (details in session)",
    "transcriptNotice.thinkingDropped": "Anthropic dropped {count} thinking blocks{reasons} (details in session)",
    "transcriptNotice.reasoningItemsDroppedOne": "OpenAI dropped 1 reasoning item{reasons} (details in session)",
    "transcriptNotice.reasoningItemsDropped": "OpenAI dropped {count} reasoning items{reasons} (details in session)",
    "transcriptNotice.droppedReasons": ": {reasons}",
  };
  const template = table[key];
  assert.ok(template, `unexpected key ${key}`);
  return template.replace(/\{(\w+)\}/g, (_, name) => String(params[name] ?? ""));
}

const text = (notice) => transcriptNoticeText(translate, notice);

test("names the provider that dropped reasoning", () => {
  assert.equal(
    text({ kind: "reasoning_dropped", provider: "anthropic", count: 2 }),
    "Anthropic dropped 2 thinking blocks (details in session)",
  );
  assert.equal(
    text({ kind: "reasoning_dropped", provider: "openai", count: 1 }),
    "OpenAI dropped 1 reasoning item (details in session)",
  );
});

test("lists the reasons a provider gave", () => {
  assert.equal(
    text({ kind: "reasoning_dropped", provider: "openai", count: 2, reasons: ["prefix_mismatch"] }),
    "OpenAI dropped 2 reasoning items: prefix_mismatch (details in session)",
  );
});

test("keeps the cost and cache notices they had", () => {
  assert.equal(
    text({ kind: "cache_miss", missedTokens: 30_000, missedCost: 0.5 }),
    "Cache miss: 30k tokens re-billed (~$0.50)",
  );
  assert.equal(
    text({ kind: "summary_cost", source: "compaction", tokens: 12_000, cost: 0 }),
    "Compaction: 12k tokens billed",
  );
});