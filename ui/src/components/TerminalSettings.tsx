"use client";

import { useI18n } from "@/hooks/useI18n";
import { TerminalAppearancePreview, TerminalTypographySettings } from "./TerminalAppearanceSettings";
import { TerminalThemePicker } from "./TerminalThemePicker";

/**
 * Settings › Terminal, a section of its own: the palette and the terminal's
 * typography are unrelated to the app theme, code theme and UI fonts that
 * General › Appearance holds.
 */
export function TerminalSettings() {
  const { t } = useI18n();
  return (
    <section className="settings-general-section" aria-label={t("settings.terminal")}>
      <p className="settings-general-description">
        {t("settings.terminalThemeDescription")}
      </p>
      <TerminalAppearancePreview />
      <h4 className="settings-general-subheading">{t("settings.terminalThemeHeading")}</h4>
      <TerminalThemePicker />
      <h4 className="settings-general-subheading">{t("settings.terminalTypographyHeading")}</h4>
      <p className="settings-general-description">{t("settings.terminalFontDescription")}</p>
      <TerminalTypographySettings />
    </section>
  );
}
