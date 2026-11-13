import assert from "node:assert/strict";
import { appendFileSync } from "node:fs";

/**
 * The trace view has to keep up with the log on its own.
 *
 * A conversation that is opened into the trace before its first entry is
 * written has nothing to show, and until this check existed the view only
 * reloaded while a run was in flight or when the session changed — so it stayed
 * on "this session has no events yet" until the user switched away and came
 * back. Every append now bumps the session's revision, which the catalog
 * carries to the view.
 *
 * The file is appended to here rather than through the app on purpose: that is
 * the one path no React state change announces, and it covers the "another
 * window wrote it" case as well.
 */
export async function checkTrace({ page, base, sessionId, sessionFile, artifacts }) {
  const rows = page.locator(".trace-row");
  const empty = page.locator(".trace-empty");

  await page.goto(`${base}/?session=${sessionId}&view=trace`, { waitUntil: "domcontentloaded" });
  await page.locator(".trace-view").waitFor();
  await rows.first().waitFor({ timeout: 30_000 });
  const before = await rows.count();
  assert.ok(before >= 2, `a saved session must trace at least its two messages (got ${before})`);

  // One line per event, so a user message and a reply are two rows. The lane is
  // read from the attribute, not the badge: the badge is translated.
  assert.equal(
    await rows.first().getAttribute("data-lane"),
    "input",
    "the first row is the user's turn",
  );

  const stamp = "2026-08-23T00:10:00.000Z";
  appendFileSync(
    sessionFile,
    [
      {
        type: "message",
        id: "trace-extra-user",
        parentId: "trace-reply",
        timestamp: stamp,
        message: { role: "user", content: [{ type: "text", text: "E2E trace follow-up" }], timestamp: Date.parse(stamp) },
      },
      {
        type: "message",
        id: "trace-extra-reply",
        parentId: "trace-extra-user",
        timestamp: stamp,
        message: { role: "assistant", content: [{ type: "text", text: "E2E trace follow-up answer" }], timestamp: Date.parse(stamp) },
      },
    ]
      .map((entry) => JSON.stringify(entry))
      .join("\n") + "\n",
  );

  // The sidebar's running poll notices the new sessionListVersion, reloads the
  // catalog, and the view catches up on the tail. Nothing here touches the
  // session picker.
  await rows.nth(before + 1).waitFor({ timeout: 30_000 });
  const after = await rows.count();
  // At least the two appended rows: a session that gets mounted can also gain
  // its own `model_change` / `thinking_level_change` entries, so the exact
  // number is not the app's to promise.
  assert.ok(after >= before + 2, `the trace must gain the appended rows (${before} -> ${after})`);
  assert.equal(await empty.count(), 0, "the empty state must be gone once there are rows");
  const texts = await rows.allInnerTexts();
  assert.ok(
    texts.some((text) => text.includes("E2E trace follow-up answer")),
    "the appended reply must be in the list",
  );

  await page.screenshot({ path: `${artifacts}/trace-${Date.now()}.png` });
  console.log("PASS: the trace view catches up on appends without switching sessions");
}
