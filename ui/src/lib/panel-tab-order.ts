export function movePanelTab(ids: string[], source: string, target: string, after: boolean): string[] {
  if (source === target || !ids.includes(source) || !ids.includes(target)) return ids;
  const result = ids.filter((id) => id !== source);
  result.splice(result.indexOf(target) + (after ? 1 : 0), 0, source);
  return result;
}
