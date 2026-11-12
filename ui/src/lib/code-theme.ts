// The code view's colours are roles defined by CSS variables (lib/code-roles.ts,
// app/globals.css). By default those roles come from the appearance theme, so
// code is always in step with the app. A palette listed here replaces them —
// background included — when the user wants code that reads differently from
// the rest of the window (a dark editor on a light app, say).
//
// A palette is only a set of `--code-*` declarations; nothing here knows Prism.

export type CodeThemeScheme = "light" | "dark";

export const CODE_THEME_OPTIONS = [
  { id: "vscode-dark", scheme: "dark", label: "settings.codeThemeVscodeDark" },
  { id: "vscode-light", scheme: "light", label: "settings.codeThemeVscodeLight" },
  { id: "github-dark", scheme: "dark", label: "settings.codeThemeGithubDark" },
  { id: "github-light", scheme: "light", label: "settings.codeThemeGithubLight" },
  { id: "solarized-dark", scheme: "dark", label: "settings.codeThemeSolarizedDark" },
  { id: "solarized-light", scheme: "light", label: "settings.codeThemeSolarizedLight" },
  { id: "catppuccin-mocha", scheme: "dark", label: "settings.codeThemeCatppuccinMocha" },
  { id: "catppuccin-latte", scheme: "light", label: "settings.codeThemeCatppuccinLatte" },
  { id: "two-monokai", scheme: "dark", label: "settings.codeThemeTwoMonokai" },
  { id: "zenburn", scheme: "dark", label: "settings.codeThemeZenburn" },
  { id: "contrast-dark", scheme: "dark", label: "settings.codeThemeContrastDark" },
  { id: "contrast-light", scheme: "light", label: "settings.codeThemeContrastLight" },
] as const;

export type CodeThemeId = (typeof CODE_THEME_OPTIONS)[number]["id"];
/** `auto` follows the appearance theme; the rest name a palette. */
export type CodeThemePreference = CodeThemeId | "auto";

export const CODE_THEME_STORAGE_KEY = "pi-code-theme";
export const CODE_THEME_DEFAULT: CodeThemePreference = "auto";

export const CODE_THEME_IDS: readonly CodeThemeId[] = CODE_THEME_OPTIONS.map((option) => option.id);

export function isCodeThemeId(value: unknown): value is CodeThemeId {
  return typeof value === "string" && (CODE_THEME_IDS as readonly string[]).includes(value);
}

export function parseCodeThemePreference(raw: string | null): CodeThemePreference {
  return isCodeThemeId(raw) ? raw : CODE_THEME_DEFAULT;
}

/** The palette's own brightness, or null when it follows the appearance theme. */
export function codeThemeScheme(preference: CodeThemePreference): CodeThemeScheme | null {
  return CODE_THEME_OPTIONS.find((option) => option.id === preference)?.scheme ?? null;
}