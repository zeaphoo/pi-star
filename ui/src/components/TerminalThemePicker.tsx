"use client";

import { useI18n } from "@/hooks/useI18n";
import { useTerminalThemePreference } from "@/hooks/useTerminalTheme";
import { TERMINAL_THEME_PRESETS, type TerminalThemePreset } from "@/lib/terminal-theme";

const SWATCHES = ["red", "green", "yellow", "blue", "magenta", "cyan"] as const;

function ThemeCard({
  preset,
  selected,
  onSelect,
}: {
  preset: TerminalThemePreset;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      className={`terminal-theme-card${selected ? " is-selected" : ""}`}
      aria-pressed={selected}
      data-terminal-preset={preset.id}
      title={preset.name}
      onClick={onSelect}
      style={{
        background: preset.xterm.background,
        color: preset.xterm.foreground,
        borderColor: selected ? preset.xterm.blue : preset.chrome.border,
        boxShadow: selected ? `0 0 0 2px ${preset.xterm.blue}` : undefined,
      }}
    >
      <span className="terminal-theme-card-name">{preset.name}</span>
      <span className="terminal-theme-card-line" aria-hidden="true">
        <span style={{ color: preset.xterm.green }}>❯</span>{" "}
        <span style={{ color: preset.xterm.blue }}>~/pi-star</span>{" "}
        <span style={{ color: preset.xterm.magenta }}>git:(main)</span>
      </span>
      <span className="terminal-theme-card-swatches" aria-hidden="true">
        {SWATCHES.map((name) => (
          <span key={name} style={{ background: preset.xterm[name] }} />
        ))}
      </span>
    </button>
  );
}

export function TerminalThemePicker() {
  const { t } = useI18n();
  const { preference, setPreference } = useTerminalThemePreference();
  const auto = preference === "auto";
  const dark = TERMINAL_THEME_PRESETS.filter((preset) => preset.kind === "dark");
  const light = TERMINAL_THEME_PRESETS.filter((preset) => preset.kind === "light");

  return (
    <div className="terminal-theme-picker">
      <button
        type="button"
        className={`terminal-theme-card terminal-theme-card-auto${auto ? " is-selected" : ""}`}
        aria-pressed={auto}
        onClick={() => setPreference("auto")}
        title={t("settings.terminalThemeAuto")}
      >
        <span className="terminal-theme-card-name">{t("settings.terminalThemeAuto")}</span>
        <span className="terminal-theme-card-auto-preview" aria-hidden="true" />
        <span className="terminal-theme-card-hint">{t("settings.terminalThemeAutoHint")}</span>
      </button>

      {[{ kind: "dark", heading: "settings.terminalThemeDark", presets: dark },
        { kind: "light", heading: "settings.terminalThemeLight", presets: light }].map((group) => (
        <div key={group.kind} className="terminal-theme-group">
          <div className="terminal-theme-group-title">{t(group.heading)}</div>
          <div className="terminal-theme-grid">
            {group.presets.map((preset) => (
              <ThemeCard
                key={preset.id}
                preset={preset}
                selected={preference === preset.id}
                onSelect={() => setPreference(preset.id)}
              />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
