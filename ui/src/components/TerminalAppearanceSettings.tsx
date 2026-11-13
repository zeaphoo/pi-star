"use client";

import { terminalFontSize, useChatAppearance } from "@/hooks/useChatAppearance";
import { useFontPreferences } from "@/hooks/useFontPreferences";
import { useI18n } from "@/hooks/useI18n";
import { useTerminalAppearance } from "@/hooks/useTerminalAppearance";
import { useTerminalTheme } from "@/hooks/useTerminalTheme";
import { FONT_FAMILY_MAX_LENGTH, FONT_WEIGHT_OPTIONS } from "@/lib/font-preferences";
import {
  resolveTerminalFontSize,
  resolveTerminalFontWeight,
  TERMINAL_CONTRAST_MAX,
  TERMINAL_CONTRAST_MIN,
  TERMINAL_FONT_OPTIONS,
  TERMINAL_FONT_SIZE_MAX,
  TERMINAL_FONT_SIZE_MIN,
  TERMINAL_LETTER_SPACING_MAX,
  TERMINAL_LETTER_SPACING_MIN,
  TERMINAL_LINE_HEIGHT_MAX,
  TERMINAL_LINE_HEIGHT_MIN,
  TERMINAL_SCROLLBACK_OPTIONS,
  terminalFontStack,
  type TerminalCursorStyle,
} from "@/lib/terminal-appearance";
import { ConfigButton, ConfigSwitch } from "./SettingsUi";

function useResolvedTerminalType() {
  const appearance = useTerminalAppearance();
  const fontPreferences = useFontPreferences();
  const chat = useChatAppearance();
  const fontSize = resolveTerminalFontSize(appearance.fontSize, terminalFontSize(chat.fontSize));
  const fontWeight = resolveTerminalFontWeight(appearance.fontWeight, fontPreferences.monoWeight);
  const fontFamily = terminalFontStack(appearance.font, appearance.customFont) ?? "var(--font-mono)";
  return { appearance, fontSize, fontWeight, fontFamily };
}

/** A small, non-live preview: the palette and metrics the terminal will use. */
export function TerminalAppearancePreview() {
  const { t } = useI18n();
  const preset = useTerminalTheme();
  const { appearance, fontSize, fontWeight, fontFamily } = useResolvedTerminalType();
  const cursor = preset.xterm.cursor ?? preset.xterm.foreground;

  return (
    <div
      className="terminal-appearance-preview"
      role="img"
      aria-label={t("settings.terminalPreview")}
      style={{
        background: preset.xterm.background,
        color: preset.xterm.foreground,
        fontFamily,
        fontWeight,
        fontSize,
        lineHeight: appearance.lineHeight,
        letterSpacing: `${appearance.letterSpacing}px`,
        borderColor: preset.chrome.border,
      }}
    >
      <div>
        <span style={{ color: preset.xterm.green }}>❯</span>{" "}
        <span style={{ color: preset.xterm.blue }}>~/pi-star</span>{" "}
        <span style={{ color: preset.xterm.magenta }}>git:(main)</span>{" "}
        <span style={{ color: preset.xterm.yellow }}>✚</span>
      </div>
      <div>
        <span style={{ color: preset.xterm.green }}>✓</span> cargo test{" "}
        <span style={{ color: preset.xterm.brightBlack }}>0.42s</span>
      </div>
      <div>
        <span style={{ color: preset.xterm.red }}>✗</span> npm run build{" "}
        <span style={{ color: preset.xterm.cyan }}>exit 1</span>
      </div>
      <div>
        <span style={{ color: preset.xterm.green }}>{"\uE0B0"}</span>{" "}
        <span style={{ color: preset.xterm.foreground }}>{"\uF121"}</span>{" "}
        <span style={{ color: preset.xterm.foreground }}>npm run dev</span>
        <span className="terminal-appearance-cursor" style={{ background: cursor }} />
      </div>
    </div>
  );
}

const CURSOR_LABELS: Record<TerminalCursorStyle, string> = {
  block: "settings.terminalCursorBlock",
  underline: "settings.terminalCursorUnderline",
  bar: "settings.terminalCursorBar",
};

export function TerminalTypographySettings() {
  const { t } = useI18n();
  const { appearance, fontSize } = useResolvedTerminalType();
  const { setAppearance, resetAppearance } = useTerminalAppearance();
  const matchChat = appearance.fontSize === null;

  return (
    <div className="settings-chat-options terminal-typography">
      <div className="settings-chat-option">
        <label htmlFor="settings-terminal-font">{t("settings.terminalFont")}</label>
        <select
          id="settings-terminal-font"
          value={appearance.font}
          onChange={(event) => setAppearance({ font: event.target.value as typeof appearance.font })}
        >
          {TERMINAL_FONT_OPTIONS.map((option) => (
            <option key={option.id} value={option.id}>{t(option.labelKey)}</option>
          ))}
        </select>
      </div>

      {appearance.font === "custom" && (
        <div className="settings-chat-option">
          <label htmlFor="settings-terminal-custom-font">{t("settings.terminalFontCustom")}</label>
          <input
            id="settings-terminal-custom-font"
            className="settings-font-input is-mono"
            type="text"
            value={appearance.customFont}
            placeholder={t("settings.terminalFontCustomPlaceholder")}
            maxLength={FONT_FAMILY_MAX_LENGTH}
            autoComplete="off"
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
            onChange={(event) => setAppearance({ customFont: event.target.value })}
          />
        </div>
      )}

      <div className="settings-chat-option">
        <label htmlFor="settings-terminal-font-weight">{t("settings.terminalFontWeight")}</label>
        <select
          id="settings-terminal-font-weight"
          value={appearance.fontWeight ?? "inherit"}
          onChange={(event) => setAppearance({
            fontWeight: event.target.value === "inherit"
              ? null
              : Number(event.target.value) as typeof appearance.fontWeight,
          })}
        >
          <option value="inherit">{t("settings.terminalFontWeightInherit")}</option>
          {FONT_WEIGHT_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>{t(option.label)}</option>
          ))}
        </select>
      </div>

      <div className="settings-chat-option settings-chat-range-option">
        <div className="settings-chat-range-header is-triple">
          <label htmlFor="settings-terminal-font-size">{t("settings.terminalFontSize")}</label>
          <span className="settings-terminal-match">
            <span>{t("settings.terminalFontSizeMatch")}</span>
            <ConfigSwitch
              checked={matchChat}
              label={t("settings.terminalFontSizeMatch")}
              onChange={(checked) => setAppearance({ fontSize: checked ? null : fontSize })}
            />
          </span>
          <output htmlFor="settings-terminal-font-size">{t("settings.terminalFontSizeValue", { size: fontSize })}</output>
        </div>
        <input
          id="settings-terminal-font-size"
          type="range"
          min={TERMINAL_FONT_SIZE_MIN}
          max={TERMINAL_FONT_SIZE_MAX}
          step={1}
          value={fontSize}
          disabled={matchChat}
          onChange={(event) => setAppearance({ fontSize: Number(event.target.value) })}
        />
      </div>

      <div className="settings-chat-option settings-chat-range-option">
        <div className="settings-chat-range-header">
          <label htmlFor="settings-terminal-line-height">{t("settings.terminalLineHeight")}</label>
          <output htmlFor="settings-terminal-line-height">{t("settings.terminalLineHeightValue", { value: appearance.lineHeight.toFixed(2) })}</output>
        </div>
        <input
          id="settings-terminal-line-height"
          type="range"
          min={TERMINAL_LINE_HEIGHT_MIN}
          max={TERMINAL_LINE_HEIGHT_MAX}
          step={0.05}
          value={appearance.lineHeight}
          onChange={(event) => setAppearance({ lineHeight: Number(event.target.value) })}
        />
      </div>

      <div className="settings-chat-option">
        <label htmlFor="settings-terminal-cursor">{t("settings.terminalCursor")}</label>
        <span className="settings-terminal-inline">
          <select
            id="settings-terminal-cursor"
            value={appearance.cursorStyle}
            onChange={(event) => setAppearance({ cursorStyle: event.target.value as TerminalCursorStyle })}
          >
            {(["block", "underline", "bar"] as const).map((style) => (
              <option key={style} value={style}>{t(CURSOR_LABELS[style])}</option>
            ))}
          </select>
          <span className="settings-terminal-blink">
            <span>{t("settings.terminalCursorBlink")}</span>
            <ConfigSwitch
              checked={appearance.cursorBlink}
              label={t("settings.terminalCursorBlink")}
              onChange={(checked) => setAppearance({ cursorBlink: checked })}
            />
          </span>
        </span>
      </div>

      <details className="settings-terminal-advanced">
        <summary>{t("settings.terminalAdvanced")}</summary>
        <div className="settings-chat-option settings-chat-range-option">
          <div className="settings-chat-range-header">
            <label htmlFor="settings-terminal-letter-spacing">{t("settings.terminalLetterSpacing")}</label>
            <output htmlFor="settings-terminal-letter-spacing">{t("settings.terminalLetterSpacingValue", { value: appearance.letterSpacing.toFixed(1) })}</output>
          </div>
          <input
            id="settings-terminal-letter-spacing"
            type="range"
            min={TERMINAL_LETTER_SPACING_MIN}
            max={TERMINAL_LETTER_SPACING_MAX}
            step={0.5}
            value={appearance.letterSpacing}
            onChange={(event) => setAppearance({ letterSpacing: Number(event.target.value) })}
          />
        </div>

        <div className="settings-chat-option settings-chat-range-option">
          <div className="settings-chat-range-header">
            <label htmlFor="settings-terminal-contrast">{t("settings.terminalMinimumContrast")}</label>
            <output htmlFor="settings-terminal-contrast">{t("settings.terminalContrastValue", { value: appearance.minimumContrastRatio })}</output>
          </div>
          <input
            id="settings-terminal-contrast"
            type="range"
            min={TERMINAL_CONTRAST_MIN}
            max={TERMINAL_CONTRAST_MAX}
            step={1}
            value={appearance.minimumContrastRatio}
            onChange={(event) => setAppearance({ minimumContrastRatio: Number(event.target.value) })}
          />
        </div>

        <div className="settings-chat-option">
          <label htmlFor="settings-terminal-scrollback">{t("settings.terminalScrollback")}</label>
          <select
            id="settings-terminal-scrollback"
            value={appearance.scrollback}
            onChange={(event) => setAppearance({ scrollback: Number(event.target.value) })}
          >
            {TERMINAL_SCROLLBACK_OPTIONS.map((lines) => (
              <option key={lines} value={lines}>{t("settings.terminalScrollbackLines", { count: lines.toLocaleString() })}</option>
            ))}
          </select>
        </div>
      </details>

      <div className="settings-chat-option settings-terminal-reset-row">
        <ConfigButton variant="ghost" size="small" onClick={resetAppearance}>
          {t("settings.terminalReset")}
        </ConfigButton>
      </div>
    </div>
  );
}
