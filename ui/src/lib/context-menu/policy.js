/**
 * App-wide context-menu policy.
 *
 * The browser/WebView default menu (Back/Reload/Open Link…, and Inspect Element
 * in a debug build) is a browser artefact; Pi Star should not show it on its
 * own chrome. wry only exposes a native switch for WebView2 on Windows
 * (`with_default_context_menus`), and Tauri does not forward it, so the
 * portable route is cancelling the DOM `contextmenu` event. WKWebView and
 * WebKitGTK honour that, like Safari does.
 *
 * The policy is installed for every build and every platform, including the
 * plain web page; it is not desktop-only.
 *
 * This module only decides *when* to suppress. It deliberately does not draw
 * anything: areas that own a menu already call `preventDefault()` themselves,
 * and a future `ContextMenuHost` will read the same opt-in markers. Until then
 * suppression is the whole behaviour.
 *
 * Everything stays native where the system menu carries value:
 * - editable fields (copy/paste, spell check, input methods);
 * - a non-empty text selection (copy, Look Up, Translate);
 * - subtrees marked with `data-native-context-menu` as an escape hatch;
 * - elements without an app menu, in a debug build (Inspect Element).
 */

/**
 * @typedef {object} ContextMenuTarget
 * @property {boolean} editable
 * @property {boolean} hasSelection
 * @property {boolean} nativeOptOut
 */

/** Mark a subtree that must keep the system menu (e.g. an embedded preview). */
export const NATIVE_CONTEXT_MENU_ATTRIBUTE = "data-native-context-menu";

/**
 * Whether the default menu must be cancelled for this target.
 *
 * Components that open their own menu call `preventDefault()` before the
 * document-level listener runs; callers of this predicate do not need to
 * special-case them.
 *
 * @param {ContextMenuTarget} target
 * @returns {boolean}
 */
/**
 * Debug builds keep the system menu on surfaces that do not declare their own.
 * @param {{ debug?: boolean } | null | undefined} bridge
 * @returns {boolean}
 */
export function keepsNativeMenuInDebug(bridge) {
  return bridge?.debug === true;
}

export function shouldSuppressContextMenu(target) {
  return !target.editable && !target.hasSelection && !target.nativeOptOut;
}

const EDITABLE_SELECTOR =
  'input, textarea, [contenteditable]:not([contenteditable="false"])';

/**
 * Whether the page currently has a non-empty text selection. Text-centric
 * menus (message bodies, code blocks, diffs) call this before taking over the
 * right click, so Copy / Look Up / Translate keep working.
 *
 * @returns {boolean}
 */
export function hasNonEmptySelection() {
  if (typeof window === "undefined" || typeof window.getSelection !== "function") {
    return false;
  }
  const selection = window.getSelection();
  return Boolean(selection && !selection.isCollapsed && selection.toString().trim());
}

/**
 * @param {EventTarget | null} node
 * @returns {ContextMenuTarget}
 */
function targetInfo(node) {
  const element = typeof Element !== "undefined" && node instanceof Element ? node : null;
  return {
    editable: Boolean(element?.closest(EDITABLE_SELECTOR)),
    hasSelection: hasNonEmptySelection(),
    nativeOptOut: Boolean(element?.closest(`[${NATIVE_CONTEXT_MENU_ATTRIBUTE}]`)),
  };
}

/**
 * Install the document-level policy. Returns an uninstall function.
 *
 * The listener is registered in the bubble phase: React attaches its handlers
 * to the root container, which bubbles before `document`, so an area that has
 * already opened its own menu shows up as `event.defaultPrevented`.
 *
 * @returns {() => void}
 */
export function installContextMenuPolicy() {
  if (typeof document === "undefined") {
    return () => {};
  }
  // A debug build keeps the system menu on unclaimed surfaces so Inspect
  // Element stays reachable; declared menus still preventDefault themselves.
  if (typeof window !== "undefined" && keepsNativeMenuInDebug(window.__PI_STAR__)) {
    return () => {};
  }
  const onContextMenu = (event) => {
    if (event.defaultPrevented) return;
    if (!shouldSuppressContextMenu(targetInfo(event.target))) return;
    event.preventDefault();
  };
  document.addEventListener("contextmenu", onContextMenu);
  return () => document.removeEventListener("contextmenu", onContextMenu);
}
