import {
  fontFamilyCss,
  readFontPreference,
  readFontWeight,
  type FontWeight,
} from "./font-preferences";

export type TerminalFontId =
  | "inherit"
  | "jetbrains-mono"
  | "jetbrains-mono-nerd"
  | "hack-nerd"
  | "noto-sans-mono"
  | "custom";
export type TerminalCursorStyle = "block" | "underline" | "bar";

export interface TerminalAppearance {
  /** `inherit` keeps following the mono font chosen under Typography. */
  font: TerminalFontId;
  /** Only used when `font` is `custom`; the same comma-separated parser as the editor. */
  customFont: string;
  /** null follows the mono font weight. */
  fontWeight: FontWeight | null;
  /** null follows the chat font size, as code blocks do. */
  fontSize: number | null;
  lineHeight: number;
  letterSpacing: number;
  cursorStyle: TerminalCursorStyle;
  cursorBlink: boolean;
  minimumContrastRatio: number;
  scrollback: number;
}

export const TERMINAL_APPEARANCE_STORAGE_KEY = "pi-star:terminal-appearance:v1";
export const TERMINAL_FONT_SIZE_MIN = 10;
export const TERMINAL_FONT_SIZE_MAX = 24;
export const TERMINAL_LINE_HEIGHT_MIN = 1;
export const TERMINAL_LINE_HEIGHT_MAX = 1.6;
export const TERMINAL_LETTER_SPACING_MIN = -0.5;
export const TERMINAL_LETTER_SPACING_MAX = 2;
export const TERMINAL_CONTRAST_MIN = 1;
export const TERMINAL_CONTRAST_MAX = 21;
export const TERMINAL_SCROLLBACK_MIN = 1000;
export const TERMINAL_SCROLLBACK_MAX = 50000;
export const TERMINAL_SCROLLBACK_OPTIONS = [2000, 8000, 20000, 50000] as const;
export const TERMINAL_CURSOR_STYLES: readonly TerminalCursorStyle[] = ["block", "underline", "bar"];

export const TERMINAL_FONT_OPTIONS = [
  { id: "inherit", labelKey: "settings.terminalFontInherit" },
  { id: "jetbrains-mono", labelKey: "settings.terminalFontJetBrainsMono" },
  { id: "jetbrains-mono-nerd", labelKey: "settings.terminalFontJetBrainsMonoNerd" },
  { id: "hack-nerd", labelKey: "settings.terminalFontHackNerd" },
  { id: "noto-sans-mono", labelKey: "settings.terminalFontNotoSansMono" },
  { id: "custom", labelKey: "settings.terminalFontCustom" },
] as const satisfies readonly { id: TerminalFontId; labelKey: string }[];

const NOTO_FALLBACK = "'Noto Sans Mono', 'Noto Sans Mono CJK SC', 'JetBrainsMono Nerd Font', 'Fira Code', 'Consolas', ui-monospace, 'PingFang SC', 'Microsoft YaHei', monospace";

/** The fallback stack always ends in CJK and the platform's monospace face. */
export function terminalFontStack(font: TerminalFontId, customFont: string): string | null {
  switch (font) {
    case "inherit": return null;
    case "jetbrains-mono": return `'JetBrains Mono', 'JetBrainsMono Nerd Font', ${NOTO_FALLBACK}`;
    case "jetbrains-mono-nerd": return `'JetBrainsMono Nerd Font', ${NOTO_FALLBACK}`;
    case "hack-nerd": return `'Hack Nerd Font', ${NOTO_FALLBACK}`;
    case "noto-sans-mono": return NOTO_FALLBACK;
    case "custom": {
      const family = fontFamilyCss(customFont);
      return family ? `${family}, ${NOTO_FALLBACK}` : null;
    }
  }
}

export const DEFAULT_TERMINAL_APPEARANCE: TerminalAppearance = {
  font: "inherit",
  customFont: "",
  fontWeight: null,
  fontSize: null,
  lineHeight: 1.25,
  letterSpacing: 0,
  cursorStyle: "block",
  cursorBlink: true,
  minimumContrastRatio: TERMINAL_CONTRAST_MIN,
  scrollback: 8000,
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function readFontId(value: unknown): TerminalFontId {
  return TERMINAL_FONT_OPTIONS.some((option) => option.id === value)
    ? value as TerminalFontId
    : DEFAULT_TERMINAL_APPEARANCE.font;
}

function readCursorStyle(value: unknown): TerminalCursorStyle {
  return TERMINAL_CURSOR_STYLES.includes(value as TerminalCursorStyle)
    ? value as TerminalCursorStyle
    : DEFAULT_TERMINAL_APPEARANCE.cursorStyle;
}

function readNullableNumber(value: unknown, min: number, max: number, round = 1): number | null {
  if (value === null || value === undefined || value === "") return null;
  const number = Number(value);
  if (!Number.isFinite(number)) return null;
  return Math.round(clamp(number, min, max) * 10 ** round) / 10 ** round;
}

export function sanitizeTerminalAppearance(value: unknown): TerminalAppearance {
  if (!isRecord(value)) return { ...DEFAULT_TERMINAL_APPEARANCE };
  const cursorBlink = typeof value.cursorBlink === "boolean"
    ? value.cursorBlink
    : DEFAULT_TERMINAL_APPEARANCE.cursorBlink;
  const minimumContrastRatio = Number(value.minimumContrastRatio);
  const scrollback = Number(value.scrollback);
  return {
    font: readFontId(value.font),
    customFont: readFontPreference(value.customFont),
    fontWeight: value.fontWeight === null || value.fontWeight === undefined
      ? null
      : readFontWeight(value.fontWeight),
    fontSize: readNullableNumber(value.fontSize, TERMINAL_FONT_SIZE_MIN, TERMINAL_FONT_SIZE_MAX, 0),
    lineHeight: readNullableNumber(value.lineHeight, TERMINAL_LINE_HEIGHT_MIN, TERMINAL_LINE_HEIGHT_MAX, 2)
      ?? DEFAULT_TERMINAL_APPEARANCE.lineHeight,
    letterSpacing: readNullableNumber(value.letterSpacing, TERMINAL_LETTER_SPACING_MIN, TERMINAL_LETTER_SPACING_MAX, 1)
      ?? DEFAULT_TERMINAL_APPEARANCE.letterSpacing,
    cursorStyle: readCursorStyle(value.cursorStyle),
    cursorBlink,
    minimumContrastRatio: Number.isFinite(minimumContrastRatio)
      ? Math.round(clamp(minimumContrastRatio, TERMINAL_CONTRAST_MIN, TERMINAL_CONTRAST_MAX))
      : DEFAULT_TERMINAL_APPEARANCE.minimumContrastRatio,
    scrollback: Number.isFinite(scrollback)
      ? Math.round(clamp(scrollback, TERMINAL_SCROLLBACK_MIN, TERMINAL_SCROLLBACK_MAX))
      : DEFAULT_TERMINAL_APPEARANCE.scrollback,
  };
}

/** Font size is either an explicit value or the chat-derived fallback. */
export function resolveTerminalFontSize(preference: number | null, fallback: number): number {
  return preference ?? fallback;
}

/** Weight is either an explicit value or the editor mono weight. */
export function resolveTerminalFontWeight(preference: FontWeight | null, fallback: FontWeight): FontWeight {
  return preference ?? fallback;
}
