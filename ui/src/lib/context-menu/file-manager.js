/**
 * Which file manager the "reveal" action talks to. The desktop shell reports
 * its platform through `__PI_STAR__`; a plain browser falls back to the UA, and
 * anything unknown gets the neutral "file manager" wording.
 *
 * @param {{ platform?: string | null, userAgent?: string | null }} [info]
 * @returns {"finder" | "explorer" | "file-manager"}
 */
export function fileManagerTarget(info = {}) {
  const platform = info.platform ?? null;
  if (platform === "macos") return "finder";
  if (platform === "windows") return "explorer";
  if (platform === "linux") return "file-manager";
  const userAgent = info.userAgent ?? (typeof navigator === "undefined" ? "" : navigator.userAgent);
  if (/Macintosh|Mac OS X/i.test(userAgent)) return "finder";
  if (/Windows/i.test(userAgent)) return "explorer";
  return "file-manager";
}

/** The i18n key naming that platform's file manager. */
export function revealMenuLabelKey(info = {}) {
  switch (fileManagerTarget(info)) {
    case "finder":
      return "menu.revealInFinder";
    case "explorer":
      return "menu.revealInExplorer";
    default:
      return "menu.revealInFileManager";
  }
}
