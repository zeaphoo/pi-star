export type DirectorySort = "name" | "modified";

export interface DirectoryEntryLike {
  name: string;
  modifiedMs?: number | null;
}

const collatorCache = new Map<string, Intl.Collator>();

function collatorFor(locale: string): Intl.Collator {
  let collator = collatorCache.get(locale);
  if (!collator) {
    collator = new Intl.Collator(locale, { sensitivity: "base", numeric: true });
    collatorCache.set(locale, collator);
  }
  return collator;
}

/** A copy of the entries in display order: names ascending, or newest first. */
export function sortDirectoryEntries<T extends DirectoryEntryLike>(
  entries: readonly T[],
  sort: DirectorySort,
  locale: string,
): T[] {
  const collator = collatorFor(locale);
  return [...entries].sort((a, b) => {
    if (sort === "modified") {
      const byModified = (b.modifiedMs ?? 0) - (a.modifiedMs ?? 0);
      if (byModified !== 0) return byModified;
    }
    return collator.compare(a.name, b.name);
  });
}

/** Compact last-modified time: the clock today, else the date. */
export function formatDirectoryModified(
  modifiedMs: number | null | undefined,
  locale: string,
  now = new Date(),
): string {
  if (modifiedMs === null || modifiedMs === undefined || !Number.isFinite(modifiedMs)) return "";
  const date = new Date(modifiedMs);
  if (Number.isNaN(date.getTime())) return "";
  const sameDay = date.getFullYear() === now.getFullYear()
    && date.getMonth() === now.getMonth()
    && date.getDate() === now.getDate();
  if (sameDay) {
    return new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit" }).format(date);
  }
  if (date.getFullYear() === now.getFullYear()) {
    return new Intl.DateTimeFormat(locale, { month: "2-digit", day: "2-digit" }).format(date);
  }
  return new Intl.DateTimeFormat(locale, { year: "numeric", month: "2-digit", day: "2-digit" }).format(date);
}
