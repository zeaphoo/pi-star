/*
 * The sidebar's session rows. The files panel that once shared this space moved
 * to the Code view when the sidebar file browser was retired, so the sidebar
 * has nothing else to open.
 */

/** Shows the session rows. */
export async function showSidebarSessions(page) {
  const tab = page.getByRole("tab", { name: "Sessions", exact: true });
  await tab.or(page.locator(".sidebar-brand")).first().waitFor();
  if (await tab.count() && await tab.getAttribute("aria-selected") !== "true") await tab.click();
  await page.locator("#session-sidebar-panel-sessions").waitFor({ state: "visible" });
}