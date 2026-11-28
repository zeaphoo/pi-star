import assert from "node:assert/strict";
import test from "node:test";
import { createJiti } from "jiti";

const jiti = createJiti(import.meta.url, { tsconfigPaths: true });
const { parseDiscoveredModels } = await jiti.import("./model-discovery.ts");

test("reads a bare list of ids", () => {
  assert.deepEqual(parseDiscoveredModels({ data: [{ id: "a" }, "b"] }), [{ id: "a" }, { id: "b" }]);
});

test("keeps the capability fields whose meaning does not vary between gateways", () => {
  assert.deepEqual(parseDiscoveredModels({
    data: [{
      id: "gpt-6.2",
      display_name: "GPT-6.2",
      context_length: 400_000,
      max_output_tokens: 128_000,
      supports_reasoning: true,
      supported_efforts: ["low", "medium", "high", "max"],
      modalities: { input: ["text", "image"] },
    }],
  }), [{
    id: "gpt-6.2",
    name: "GPT-6.2",
    reasoning: true,
    // Levels the list advertises inherit pi's own name; the rest are marked off.
    thinkingLevelMap: { minimal: null, xhigh: null, max: "max" },
    input: ["text", "image"],
    contextWindow: 400_000,
    maxTokens: 128_000,
  }]);
});

test("reads input_modalities and a nested effort list (#1791)", () => {
  assert.deepEqual(parseDiscoveredModels({
    object: "list",
    data: [{
      context_length: 1_000_000,
      created: 0,
      effort: { default_level: "high", supported_levels: ["low", "high", "max"] },
      id: "deepseek-v4.1-flash-1017",
      input_modalities: ["text", "image"],
      max_output_tokens: 384_000,
      object: "model",
      supports_reasoning: true,
    }],
  }), [{
    id: "deepseek-v4.1-flash-1017",
    reasoning: true,
    thinkingLevelMap: { minimal: null, medium: null, xhigh: null, max: "max" },
    input: ["text", "image"],
    contextWindow: 1_000_000,
    maxTokens: 384_000,
  }]);
});

test("reads OpenRouter's architecture.input_modalities", () => {
  assert.deepEqual(parseDiscoveredModels({
    data: [{
      id: "openai/gpt-5",
      name: "OpenAI: GPT-5",
      context_length: 400_000,
      architecture: { modality: "text+image->text", input_modalities: ["image", "text", "file"] },
    }],
  }), [{
    id: "openai/gpt-5",
    name: "OpenAI: GPT-5",
    input: ["image", "text"],
    contextWindow: 400_000,
  }]);
});

test("keeps the input modalities pi knows and drops the rest", () => {
  assert.deepEqual(parseDiscoveredModels({
    data: [
      { id: "audio-only", input: ["audio"] },
      { id: "image-only", input: ["image"] },
    ],
  }), [{ id: "audio-only" }, { id: "image-only", input: ["image"] }]);
});

test("names nothing when the list is empty or unmapped", () => {
  assert.deepEqual(parseDiscoveredModels({ data: [{ id: "x", modalities: { input: [] } }] }), [{ id: "x" }]);
});

test("drops ambiguous and junk values", () => {
  assert.deepEqual(parseDiscoveredModels({
    data: [{
      id: "x",
      context_length: 0,
      max_tokens: 4096,
      supported_efforts: ["none"],
      pricing: { input: "0.000001" },
    }],
  }), [{ id: "x" }]);
});

test("sorts by shown name and drops a repeated id", () => {
  assert.deepEqual(parseDiscoveredModels([
    { id: "b", name: "Beta" },
    { id: "a", name: "alpha" },
    { id: "a", name: "again" },
  ]).map((model) => model.id), ["a", "b"]);
});