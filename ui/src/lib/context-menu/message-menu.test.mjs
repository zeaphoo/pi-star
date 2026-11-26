import assert from "node:assert/strict";
import test from "node:test";
import { messageMarkdown } from "./message-menu.js";

test("a string body is already Markdown", () => {
  assert.equal(messageMarkdown({ content: "**hi**" }), "**hi**");
});

test("text blocks are joined and other blocks are skipped", () => {
  const message = {
    content: [
      { type: "text", text: "first" },
      { type: "image", source: { type: "url", url: "x" } },
      { type: "text", text: "second" },
      { type: "thinking", thinking: "hidden" },
    ],
  };
  assert.equal(messageMarkdown(message), "first\n\nsecond");
});

test("no text blocks yields no action", () => {
  assert.equal(messageMarkdown({ content: [{ type: "toolCall", toolCallId: "t" }] }), "");
  assert.equal(messageMarkdown({}), "");
  assert.equal(messageMarkdown(null), "");
});
