import type { ITheme } from "@xterm/xterm";

/** A preset's own light/dark identity, used for the auto pair and the CSS hint. */
export type TerminalThemeKind = "dark" | "light";
/** `auto` follows the app theme; anything else is a preset id. */
export type TerminalThemePreference = "auto" | string;

export const TERMINAL_THEME_STORAGE_KEY = "pi-star:terminal-theme";
export const TERMINAL_THEME_DEFAULT: TerminalThemePreference = "pi-dark";

/** Chrome is the terminal's own frame: tabs, split borders and pane controls. */
export interface TerminalChrome {
  background: string;
  panel: string;
  border: string;
  hover: string;
  muted: string;
  strong: string;
  key: string;
  accent: string;
  scrollbar: string;
  errorBorder: string;
  errorBackground: string;
  errorText: string;
}

export interface TerminalThemePreset {
  id: string;
  name: string;
  kind: TerminalThemeKind;
  xterm: ITheme;
  chrome: TerminalChrome;
}

interface PaletteInput {
  id: string;
  name: string;
  kind: TerminalThemeKind;
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent?: string;
  selection: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

function parseHex(value: string): [number, number, number] {
  const hex = value.replace("#", "").trim();
  const full = hex.length === 3 ? hex.split("").map((part) => part + part).join("") : hex;
  const number = Number.parseInt(full, 16);
  return [(number >> 16) & 0xff, (number >> 8) & 0xff, number & 0xff];
}

function toHex([red, green, blue]: [number, number, number]): string {
  return `#${[red, green, blue].map((part) => Math.round(part).toString(16).padStart(2, "0")).join("")}`;
}

/** `ratio` is how much of `to` is mixed into `from`. */
function mix(from: string, to: string, ratio: number): string {
  const a = parseHex(from);
  const b = parseHex(to);
  return toHex([
    a[0] + (b[0] - a[0]) * ratio,
    a[1] + (b[1] - a[1]) * ratio,
    a[2] + (b[2] - a[2]) * ratio,
  ]);
}

function rgbTriplet(value: string): string {
  return parseHex(value).join(", ");
}

/** Frame colours derive from the palette, so a new preset cannot leave the tabs behind. */
function chromeFor(palette: PaletteInput): TerminalChrome {
  const dark = palette.kind === "dark";
  return {
    background: palette.background,
    panel: mix(palette.background, palette.foreground, dark ? 0.06 : 0.035),
    border: mix(palette.background, palette.foreground, dark ? 0.18 : 0.14),
    hover: mix(palette.background, palette.foreground, dark ? 0.12 : 0.08),
    muted: mix(palette.foreground, palette.background, 0.35),
    strong: palette.foreground,
    key: palette.foreground,
    accent: palette.cursor,
    scrollbar: rgbTriplet(mix(palette.foreground, palette.background, 0.45)),
    errorBorder: dark ? "#5f2424" : "#fecaca",
    errorBackground: dark ? "#321b1b" : "#fef2f2",
    errorText: dark ? "#fca5a5" : "#b91c1c",
  };
}

function preset(palette: PaletteInput): TerminalThemePreset {
  return {
    id: palette.id,
    name: palette.name,
    kind: palette.kind,
    chrome: chromeFor(palette),
    xterm: {
      background: palette.background,
      foreground: palette.foreground,
      cursor: palette.cursor,
      cursorAccent: palette.cursorAccent,
      selectionBackground: palette.selection,
      black: palette.black,
      red: palette.red,
      green: palette.green,
      yellow: palette.yellow,
      blue: palette.blue,
      magenta: palette.magenta,
      cyan: palette.cyan,
      white: palette.white,
      brightBlack: palette.brightBlack,
      brightRed: palette.brightRed,
      brightGreen: palette.brightGreen,
      brightYellow: palette.brightYellow,
      brightBlue: palette.brightBlue,
      brightMagenta: palette.brightMagenta,
      brightCyan: palette.brightCyan,
      brightWhite: palette.brightWhite,
    },
  };
}

/** Curated presets; each one is a complete ANSI palette plus its frame. */
export const TERMINAL_THEME_PRESETS: readonly TerminalThemePreset[] = [
  preset({
    id: "pi-dark", name: "Pi Dark", kind: "dark",
    background: "#111318", foreground: "#d7dce5", cursor: "#60a5fa", selection: "#365b8a",
    black: "#1d222b", red: "#f87171", green: "#4ade80", yellow: "#facc15",
    blue: "#60a5fa", magenta: "#c084fc", cyan: "#22d3ee", white: "#e5e7eb",
    brightBlack: "#6b7280", brightRed: "#fca5a5", brightGreen: "#86efac",
    brightYellow: "#fde047", brightBlue: "#93c5fd", brightMagenta: "#d8b4fe",
    brightCyan: "#67e8f9", brightWhite: "#ffffff",
  }),
  preset({
    id: "pi-light", name: "Pi Light", kind: "light",
    background: "#fbfbfc", foreground: "#24292f", cursor: "#2563eb",
    cursorAccent: "#fbfbfc", selection: "#bfd7fb",
    black: "#24292f", red: "#cf222e", green: "#116329", yellow: "#7d4e00",
    blue: "#0969da", magenta: "#8250df", cyan: "#1b7c83", white: "#6e7781",
    brightBlack: "#57606a", brightRed: "#a40e26", brightGreen: "#1a7f37",
    brightYellow: "#633c01", brightBlue: "#218bff", brightMagenta: "#a475f9",
    brightCyan: "#3192aa", brightWhite: "#57606a",
  }),
  preset({
    id: "one-dark", name: "One Dark", kind: "dark",
    background: "#282c34", foreground: "#abb2bf", cursor: "#528bff", selection: "#3e4451",
    black: "#1e2127", red: "#e06c75", green: "#98c379", yellow: "#d19a66",
    blue: "#61afef", magenta: "#c678dd", cyan: "#56b6c2", white: "#abb2bf",
    brightBlack: "#5c6370", brightRed: "#e06c75", brightGreen: "#98c379",
    brightYellow: "#e5c07b", brightBlue: "#61afef", brightMagenta: "#c678dd",
    brightCyan: "#56b6c2", brightWhite: "#ffffff",
  }),
  preset({
    id: "dracula", name: "Dracula", kind: "dark",
    background: "#282a36", foreground: "#f8f8f2", cursor: "#f8f8f2", selection: "#44475a",
    black: "#21222c", red: "#ff5555", green: "#50fa7b", yellow: "#f1fa8c",
    blue: "#bd93f9", magenta: "#ff79c6", cyan: "#8be9fd", white: "#f8f8f2",
    brightBlack: "#6272a4", brightRed: "#ff6e6e", brightGreen: "#69ff94",
    brightYellow: "#ffffa5", brightBlue: "#d6acff", brightMagenta: "#ff92df",
    brightCyan: "#a4ffff", brightWhite: "#ffffff",
  }),
  preset({
    id: "nord", name: "Nord", kind: "dark",
    background: "#2e3440", foreground: "#d8dee9", cursor: "#d8dee9", selection: "#434c5e",
    black: "#3b4252", red: "#bf616a", green: "#a3be8c", yellow: "#ebcb8b",
    blue: "#81a1c1", magenta: "#b48ead", cyan: "#88c0d0", white: "#e5e9f0",
    brightBlack: "#4c566a", brightRed: "#bf616a", brightGreen: "#a3be8c",
    brightYellow: "#ebcb8b", brightBlue: "#81a1c1", brightMagenta: "#b48ead",
    brightCyan: "#8fbcbb", brightWhite: "#eceff4",
  }),
  preset({
    id: "gruvbox-dark", name: "Gruvbox Dark", kind: "dark",
    background: "#282828", foreground: "#ebdbb2", cursor: "#ebdbb2", selection: "#504945",
    black: "#282828", red: "#cc241d", green: "#98971a", yellow: "#d79921",
    blue: "#458588", magenta: "#b16286", cyan: "#689d6a", white: "#a89984",
    brightBlack: "#928374", brightRed: "#fb4934", brightGreen: "#b8bb26",
    brightYellow: "#fabd2f", brightBlue: "#83a598", brightMagenta: "#d3869b",
    brightCyan: "#8ec07c", brightWhite: "#ebdbb2",
  }),
  preset({
    id: "gruvbox-light", name: "Gruvbox Light", kind: "light",
    background: "#fbf1c7", foreground: "#3c3836", cursor: "#3c3836", selection: "#ebdbb2",
    black: "#fbf1c7", red: "#cc241d", green: "#98971a", yellow: "#d79921",
    blue: "#458588", magenta: "#b16286", cyan: "#689d6a", white: "#7c6f64",
    brightBlack: "#928374", brightRed: "#9d0006", brightGreen: "#79740e",
    brightYellow: "#b57614", brightBlue: "#076678", brightMagenta: "#8f3f71",
    brightCyan: "#427b58", brightWhite: "#3c3836",
  }),
  preset({
    id: "solarized-dark", name: "Solarized Dark", kind: "dark",
    background: "#002b36", foreground: "#839496", cursor: "#93a1a1", selection: "#073642",
    black: "#073642", red: "#dc322f", green: "#859900", yellow: "#b58900",
    blue: "#268bd2", magenta: "#d33682", cyan: "#2aa198", white: "#93a1a1",
    brightBlack: "#002b36", brightRed: "#cb4b16", brightGreen: "#586e75",
    brightYellow: "#657b83", brightBlue: "#839496", brightMagenta: "#6c71c4",
    brightCyan: "#93a1a1", brightWhite: "#fdf6e3",
  }),
  preset({
    id: "solarized-light", name: "Solarized Light", kind: "light",
    background: "#fdf6e3", foreground: "#657b83", cursor: "#586e75", selection: "#eee8d5",
    black: "#073642", red: "#dc322f", green: "#859900", yellow: "#b58900",
    blue: "#268bd2", magenta: "#d33682", cyan: "#2aa198", white: "#eee8d5",
    brightBlack: "#002b36", brightRed: "#cb4b16", brightGreen: "#586e75",
    brightYellow: "#657b83", brightBlue: "#839496", brightMagenta: "#6c71c4",
    brightCyan: "#93a1a1", brightWhite: "#586e75",
  }),
  preset({
    id: "github-light", name: "GitHub Light", kind: "light",
    background: "#ffffff", foreground: "#24292f", cursor: "#0969da", selection: "#b6d7ff",
    black: "#24292f", red: "#cf222e", green: "#116329", yellow: "#4d2d00",
    blue: "#0969da", magenta: "#8250df", cyan: "#1b7c83", white: "#6e7781",
    brightBlack: "#57606a", brightRed: "#a40e26", brightGreen: "#1a7f37",
    brightYellow: "#633c01", brightBlue: "#218bff", brightMagenta: "#a475f9",
    brightCyan: "#3192aa", brightWhite: "#57606a",
  }),
  preset({
    id: "catppuccin-mocha", name: "Catppuccin Mocha", kind: "dark",
    background: "#1e1e2e", foreground: "#cdd6f4", cursor: "#f5e0dc", selection: "#585b70",
    black: "#45475a", red: "#f38ba8", green: "#a6e3a1", yellow: "#f9e2af",
    blue: "#89b4fa", magenta: "#f5c2e7", cyan: "#94e2d5", white: "#bac2de",
    brightBlack: "#585b70", brightRed: "#f38ba8", brightGreen: "#a6e3a1",
    brightYellow: "#f9e2af", brightBlue: "#89b4fa", brightMagenta: "#f5c2e7",
    brightCyan: "#94e2d5", brightWhite: "#a6adc8",
  }),
  preset({
    id: "catppuccin-latte", name: "Catppuccin Latte", kind: "light",
    background: "#eff1f5", foreground: "#4c4f69", cursor: "#dc8a78", selection: "#ccd0da",
    black: "#5c5f77", red: "#d20f39", green: "#40a02b", yellow: "#df8e1d",
    blue: "#1e66f5", magenta: "#ea76cb", cyan: "#179299", white: "#6c6f85",
    brightBlack: "#6c6f85", brightRed: "#d20f39", brightGreen: "#40a02b",
    brightYellow: "#df8e1d", brightBlue: "#1e66f5", brightMagenta: "#ea76cb",
    brightCyan: "#179299", brightWhite: "#4c4f69",
  }),
  preset({
    id: "tokyo-night", name: "Tokyo Night", kind: "dark",
    background: "#1a1b26", foreground: "#c0caf5", cursor: "#c0caf5", selection: "#33467c",
    black: "#15161e", red: "#f7768e", green: "#9ece6a", yellow: "#e0af68",
    blue: "#7aa2f7", magenta: "#bb9af7", cyan: "#7dcfff", white: "#a9b1d6",
    brightBlack: "#414868", brightRed: "#f7768e", brightGreen: "#9ece6a",
    brightYellow: "#e0af68", brightBlue: "#7aa2f7", brightMagenta: "#bb9af7",
    brightCyan: "#7dcfff", brightWhite: "#c0caf5",
  }),
];

const PRESETS_BY_ID = new Map(TERMINAL_THEME_PRESETS.map((item) => [item.id, item]));

/** Old values ("dark"/"light") migrate to the matching Pi preset. */
export function parseTerminalThemePreference(raw: string | null): TerminalThemePreference {
  if (raw === "auto") return "auto";
  if (raw === "dark") return "pi-dark";
  if (raw === "light") return "pi-light";
  return raw && PRESETS_BY_ID.has(raw) ? raw : TERMINAL_THEME_DEFAULT;
}

export function terminalThemePreset(id: string): TerminalThemePreset {
  return PRESETS_BY_ID.get(id) ?? PRESETS_BY_ID.get("pi-dark")!;
}

/** `auto` uses the matching Pi preset; an explicit preset ignores the app theme. */
export function resolveTerminalTheme(
  preference: TerminalThemePreference,
  appIsDark: boolean,
): TerminalThemePreset {
  if (preference === "auto") return terminalThemePreset(appIsDark ? "pi-dark" : "pi-light");
  return PRESETS_BY_ID.get(preference) ?? terminalThemePreset(appIsDark ? "pi-dark" : "pi-light");
}
