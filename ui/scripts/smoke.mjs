#!/usr/bin/env node
// Browser smoke test against a running pi-star server.
// Usage: PI_STAR_URL=http://127.0.0.1:30151 npm run smoke
import { chromium } from "playwright";

const url = process.env.PI_STAR_URL ?? "http://127.0.0.1:30151";
const executablePath = process.env.PI_STAR_CHROMIUM;
const browser = await chromium.launch({
  headless: true,
  ...(executablePath ? { executablePath } : {}),
  args: ["--no-sandbox"],
});
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const errors = [];
page.on("console", (message) => {
  if (message.type() === "error") errors.push(message.text());
});
page.on("pageerror", (error) => errors.push(`pageerror: ${error.message}`));
try {
  await page.goto(url, { waitUntil: "domcontentloaded", timeout: 30_000 });
  await page.waitForTimeout(8_000);
  const body = await page.locator("body").innerText();
  if (!body.includes("Pi Web")) throw new Error("the Pi Web shell did not render");
  if (errors.length > 0) throw new Error(`browser console errors: ${errors.slice(0, 5).join(" | ")}`);
  await page.screenshot({ path: process.env.PI_STAR_SCREENSHOT ?? "/tmp/pi-star-smoke.png" });
  console.log("ui smoke ok");
} finally {
  await browser.close();
}
