/**
 * Shared shapes for the app-wide context menu.
 *
 * Kept free of React so the registry, the host and tests can share them. New
 * code in this change is `.js` / `.jsx`; see design.md decision 9.
 */

/**
 * @typedef {{ left: number, top: number, right: number, bottom: number }} Rect
 * @typedef {{ kind: "point", x: number, y: number }} PointAnchor
 * @typedef {{ kind: "rect", rect: Rect, align: "start" | "end" }} RectAnchor
 * @typedef {PointAnchor | RectAnchor} ContextMenuAnchor
 */

/**
 * @typedef {object} ContextMenuItemBase
 * @property {string} id
 * @property {import("react").ReactNode} [icon]
 * @property {string} [shortcut]
 * @property {boolean} [disabled]
 * @property {string} [disabledReason]
 * @property {boolean} [danger]
 */

/**
 * @typedef {ContextMenuItemBase & {
 *   type?: "item",
 *   label: string,
 *   checked?: boolean,
 *   note?: string,
 *   title?: string,
 *   onSelect?: (event: { shiftKey: boolean, keepOpen: () => void }) => void,
 * }} ContextMenuAction
 */

/** @typedef {ContextMenuItemBase & { type: "submenu", label: string, items: ContextMenuEntry[] }} ContextMenuSubmenu */
/** @typedef {{ type: "separator", id: string }} ContextMenuSeparator */
/** @typedef {{ type: "header", id: string, label: string }} ContextMenuHeader */

/** @typedef {ContextMenuAction | ContextMenuSubmenu | ContextMenuSeparator | ContextMenuHeader} ContextMenuEntry */

/**
 * @typedef {object} ContextMenuRequest
 * @property {ContextMenuAnchor} anchor
 * @property {ContextMenuEntry[]} items
 * @property {string} ariaLabel
 * @property {string} [title]
 * @property {number} [width]
 * @property {HTMLElement | null} [returnFocusTo]
 */

export {};
