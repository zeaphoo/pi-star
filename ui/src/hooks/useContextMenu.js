"use client";

import { createContext, useContext } from "react";

/** @type {import("react").Context<any>} */
export const ContextMenuContext = createContext(null);

/**
 * The app-wide context menu: `open(request)` shows one menu, `close()` hides
 * it. Components that own state build their own entries; simple surfaces can
 * use `data-context-menu` + `registerContextMenu`.
 *
 * The provider lives in `ContextMenuProvider.jsx`; this file stays free of JSX
 * so component tests can import the hook without a JSX transform.
 *
 * @returns {{ open: (request: any) => void, close: () => void }}
 */
export function useContextMenu() {
  const context = useContext(ContextMenuContext);
  // Components are rendered in isolation by tests (and could be embedded
  // without the app shell): a missing provider just means no menu.
  return /** @type {{ open: (request: any) => void, close: () => void }} */ (
    context ?? { open: () => {}, close: () => {} }
  );
}
