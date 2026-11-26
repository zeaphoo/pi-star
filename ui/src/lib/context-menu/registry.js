/**
 * Optional declarative registry for `data-context-menu` elements.
 *
 * Components that own state pass `items` to `useContextMenu().open(...)`
 * directly. The registry serves simple, stateless surfaces: a `data-context-menu`
 * element names an id and a builder turns it (plus its dataset) into entries.
 */

/** @type {Map<string, (element: HTMLElement) => import("./types").ContextMenuEntry[]>} */
const builders = new Map();

/**
 * @param {string} id
 * @param {(element: HTMLElement) => import("./types").ContextMenuEntry[]} build
 */
export function registerContextMenu(id, build) {
  builders.set(id, build);
  return () => {
    if (builders.get(id) === build) builders.delete(id);
  };
}

/**
 * Entries for a `data-context-menu` element, or `null` when no builder exists.
 * @param {HTMLElement | null} element
 */
export function resolveContextMenu(element) {
  const id = element?.dataset?.contextMenu;
  if (!id) return null;
  const build = builders.get(id);
  if (!build) return null;
  return build(element);
}

/** Test seam: forget every registered builder. */
export function clearContextMenus() {
  builders.clear();
}
