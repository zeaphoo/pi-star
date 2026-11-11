import type { SplitDiffGap, UnifiedDiffLine } from "./patch";

export type DiffHighlightLine = UnifiedDiffLine | SplitDiffGap;

export interface DiffHighlightSlot {
  segment: number;
  line: number;
}

/**
 * A diff is highlighted the way px0 does it: each hunk's old side and new side
 * are tokenised as contiguous code, then the resulting line HTML is mapped back
 * to the displayed rows. Tokenising the + and - lines separately would lose
 * multi-line comments and strings; joining the whole patch would carry state
 * from one hunk into the next.
 */
export interface DiffHighlightPlan {
  segments: string[];
  oldAt: Array<DiffHighlightSlot | null>;
  newAt: Array<DiffHighlightSlot | null>;
}

export function buildDiffHighlightPlan(lines: readonly DiffHighlightLine[]): DiffHighlightPlan {
  const segments: string[] = [];
  const oldAt: Array<DiffHighlightSlot | null> = new Array(lines.length).fill(null);
  const newAt: Array<DiffHighlightSlot | null> = new Array(lines.length).fill(null);

  let oldTexts: string[] = [];
  let newTexts: string[] = [];
  let oldSlots: Array<{ index: number; line: number }> = [];
  let newSlots: Array<{ index: number; line: number }> = [];

  const flush = () => {
    if (oldTexts.length > 0) {
      const segment = segments.length;
      segments.push(oldTexts.join("\n"));
      for (const slot of oldSlots) oldAt[slot.index] = { segment, line: slot.line };
    }
    if (newTexts.length > 0) {
      const segment = segments.length;
      segments.push(newTexts.join("\n"));
      for (const slot of newSlots) newAt[slot.index] = { segment, line: slot.line };
    }
    oldTexts = [];
    newTexts = [];
    oldSlots = [];
    newSlots = [];
  };

  lines.forEach((line, index) => {
    if (line.type === "gap") {
      flush();
      return;
    }
    if (line.type === "removed") {
      oldSlots.push({ index, line: oldTexts.length });
      oldTexts.push(line.text);
      return;
    }
    if (line.type === "added") {
      newSlots.push({ index, line: newTexts.length });
      newTexts.push(line.text);
      return;
    }
    oldSlots.push({ index, line: oldTexts.length });
    oldTexts.push(line.text);
    newSlots.push({ index, line: newTexts.length });
    newTexts.push(line.text);
  });
  flush();

  return { segments, oldAt, newAt };
}

/** The highlighted HTML for one displayed diff line, or null while it loads. */
export function diffLineHtml(
  line: DiffHighlightLine,
  index: number,
  plan: DiffHighlightPlan,
  highlighted: string[][] | null,
): string | null {
  if (!highlighted) return null;
  if (line.type === "gap") return null;
  const slot = line.type === "removed"
    ? plan.oldAt[index]
    : line.type === "added"
      ? plan.newAt[index]
      : plan.newAt[index] ?? plan.oldAt[index];
  if (!slot) return null;
  return highlighted[slot.segment]?.[slot.line] ?? null;
}