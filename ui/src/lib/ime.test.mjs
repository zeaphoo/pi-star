import assert from "node:assert/strict";
import test from "node:test";

const { isImeComposing } = await import("./ime.ts");

test("recognizes an input method confirming a candidate", () => {
  assert.equal(isImeComposing({ nativeEvent: { isComposing: true } }), true);
  assert.equal(isImeComposing({ isComposing: true }), true);
  // Safari's confirming Enter: keyCode 229 with isComposing already cleared.
  assert.equal(isImeComposing({ nativeEvent: { isComposing: false }, keyCode: 229 }), true);
});

test("leaves an ordinary key press alone", () => {
  assert.equal(isImeComposing({ nativeEvent: { isComposing: false }, keyCode: 13 }), false);
  assert.equal(isImeComposing({}), false);
});
