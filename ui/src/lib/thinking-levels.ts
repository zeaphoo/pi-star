/**
 * pi's reasoning levels, shared by the composer's selector and the Settings
 * default. The ids are sent to the backend and stored in settings.json; only
 * their titles and descriptions are localized.
 */
export const THINKING_LEVEL_IDS = ["auto", "off", "minimal", "low", "medium", "high", "xhigh", "max"] as const;
export type ThinkingLevelId = typeof THINKING_LEVEL_IDS[number];

/** Levels a session can be pinned to. "auto" means "follow the saved default". */
export const CONCRETE_THINKING_LEVEL_IDS = ["off", "minimal", "low", "medium", "high", "xhigh", "max"] as const;
export type ConcreteThinkingLevelId = typeof CONCRETE_THINKING_LEVEL_IDS[number];

/** pi's own fallback when settings.json has no defaultThinkingLevel. */
export const DEFAULT_THINKING_LEVEL: ConcreteThinkingLevelId = "medium";

export const THINKING_LEVEL_LABEL_KEYS: Record<ThinkingLevelId, string> = {
  auto: "chat.thinkingLevelAuto",
  off: "chat.thinkingLevelOff",
  minimal: "chat.thinkingLevelMinimal",
  low: "chat.thinkingLevelLow",
  medium: "chat.thinkingLevelMedium",
  high: "chat.thinkingLevelHigh",
  xhigh: "chat.thinkingLevelXhigh",
  max: "chat.thinkingLevelMax",
};

export const THINKING_LEVEL_DESC_KEYS: Record<ThinkingLevelId, string> = {
  auto: "chat.thinkingUseDefault",
  off: "chat.thinkingOff",
  minimal: "chat.thinkingMinimal",
  low: "chat.thinkingLow",
  medium: "chat.thinkingMedium",
  high: "chat.thinkingHigh",
  xhigh: "chat.thinkingXhigh",
  max: "chat.thinkingMax",
};

export function isConcreteThinkingLevel(value: unknown): value is ConcreteThinkingLevelId {
  return typeof value === "string" && (CONCRETE_THINKING_LEVEL_IDS as readonly string[]).includes(value);
}
