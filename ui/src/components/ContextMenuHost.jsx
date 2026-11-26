"use client";

import { PopupMenu } from "./PopupMenu.jsx";

/**
 * The single mounted menu for the whole app. The provider owns the request;
 * this component only maps it to `PopupMenu`.
 *
 * @param {object} props
 * @param {import("@/lib/context-menu/types").ContextMenuRequest | null} props.request
 * @param {string} props.cancelLabel
 * @param {(reason: "escape" | "outside" | "select" | "cancel") => void} props.onClose
 */
export function ContextMenuHost({ request, cancelLabel, onClose }) {
  if (!request) return null;
  return (
    <PopupMenu
      open
      anchor={request.anchor}
      items={request.items}
      ariaLabel={request.ariaLabel}
      title={request.title}
      width={request.width}
      returnFocusTo={request.returnFocusTo ?? null}
      cancelLabel={cancelLabel}
      onClose={onClose}
    />
  );
}
