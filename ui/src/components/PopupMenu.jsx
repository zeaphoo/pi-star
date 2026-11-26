"use client";

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { placeSidebarMenu } from "./SidebarMenu";
import { useIsMobile } from "@/hooks/useIsMobile";

/**
 * Generic app-wide popup/context menu.
 *
 * It shares the look (sidebar-menu.css) and the placement math
 * (`placeSidebarMenu`) with the session sidebar's menu, so there is one set of
 * edge-flip rules. `SidebarMenu` keeps its session-specific bodies (filter,
 * confirmation rows); this component serves the right-click menus added by the
 * `context-menu` change.
 *
 * @param {object} props
 * @param {boolean} props.open
 * @param {import("@/lib/context-menu/types").ContextMenuAnchor | null} props.anchor
 * @param {import("@/lib/context-menu/types").ContextMenuEntry[]} props.items
 * @param {string} props.ariaLabel
 * @param {string} [props.title]
 * @param {string} [props.cancelLabel]
 * @param {number} [props.width]
 * @param {HTMLElement | null} [props.returnFocusTo]
 * @param {(reason: "escape" | "outside" | "select" | "cancel") => void} props.onClose
 */
export function PopupMenu({
  open,
  anchor,
  items,
  ariaLabel,
  title,
  cancelLabel = "Cancel",
  width = 208,
  returnFocusTo = null,
  onClose,
}) {
  const sheet = useIsMobile();
  const surfaceRef = useRef(null);
  const [position, setPosition] = useState(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const focusRequested = useRef(false);

  const close = useCallback((reason) => {
    onCloseRef.current?.(reason);
  }, []);

  // Placement: measure the surface once items exist, then use the shared
  // point/rect rules. A sheet ignores the anchor.
  useLayoutEffect(() => {
    if (!open) {
      setPosition(null);
      return;
    }
    if (sheet) return;
    const surface = surfaceRef.current;
    if (!surface || !anchor) return;
    const size = surface.getBoundingClientRect();
    setPosition(placeSidebarMenu(anchor, { width: size.width, height: size.height }, {
      left: 0,
      top: 0,
      width: window.innerWidth,
      height: window.innerHeight,
    }));
  }, [open, sheet, anchor, items]);

  // Focus the first enabled item on open; keyboard users land in the menu.
  useEffect(() => {
    if (!open || focusRequested.current) return;
    focusRequested.current = true;
    const frame = requestAnimationFrame(() => {
      focusFirst(surfaceRef.current);
    });
    return () => cancelAnimationFrame(frame);
  }, [open]);
  useEffect(() => {
    if (!open) focusRequested.current = false;
  }, [open]);

  // Dismissal. Scroll closes a desktop menu; a sheet keeps its own scroll.
  useEffect(() => {
    if (!open) return;
    const inside = (target) => eventInsideMenu(target, surfaceRef.current);
    const onPointerDown = (event) => {
      if (inside(event.target)) return;
      close("outside");
    };
    const onKeyDown = (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        close("escape");
      }
    };
    const onScroll = (event) => {
      if (inside(event.target)) return;
      close("outside");
    };
    const onBlur = () => close("outside");
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("scroll", onScroll, true);
    window.addEventListener("blur", onBlur);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown, true);
      document.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("blur", onBlur);
    };
  }, [open, close]);

  // Return focus after closing, unless the opener is gone.
  const restoreRef = useRef(returnFocusTo);
  restoreRef.current = returnFocusTo;
  useEffect(() => {
    if (open) return;
    const target = restoreRef.current;
    if (target && target.isConnected) target.focus();
    restoreRef.current = null;
  }, [open]);

  if (!open || typeof document === "undefined") return null;

  const surface = (
    <div
      ref={surfaceRef}
      role="menu"
      aria-label={ariaLabel}
      data-popup-surface
      className="sidebar-menu"
      style={sheet
        ? { left: 0, right: 0, bottom: 0, top: "auto", width: "100%", maxWidth: "none", maxHeight: "70dvh", borderRadius: "12px 12px 0 0", zIndex: 620 }
        : { left: position?.left ?? -9999, top: position?.top ?? -9999, width, maxWidth: "calc(100vw - 16px)" }}
    >
      {sheet && title ? <div className="sidebar-sheet-title">{title}</div> : null}
      <MenuEntries items={items} onClose={close} root />
      {sheet ? (
        <button type="button" className="sidebar-menu-item" onClick={() => close("cancel")}>
          {cancelLabel}
        </button>
      ) : null}
    </div>
  );

  return createPortal(
    sheet ? (
      <>
        <div className="sidebar-sheet-backdrop" onPointerDown={() => close("outside")} />
        {surface}
      </>
    ) : surface,
    document.body,
  );
}

/**
 * @param {import("@/lib/context-menu/types").ContextMenuEntry[]} items
 */
function MenuEntries({ items, onClose, root = false }) {
  return (
    <>
      {items.map((item, index) => {
        if (!item || item.type === "separator") {
          return <div key={item?.id ?? `separator-${index}`} className="sidebar-menu-separator" role="separator" />;
        }
        if (item.type === "header") {
          return <div key={item.id} className="sidebar-menu-header">{item.label}</div>;
        }
        if (item.type === "submenu") {
          return <Submenu key={item.id} item={item} onClose={onClose} />;
        }
        return <MenuButton key={item.id} item={item} onClose={onClose} root={root} />;
      })}
    </>
  );
}

function MenuButton({ item, onClose, root }) {
  const activate = (event) => {
    if (item.disabled) return;
    let keepOpen = false;
    item.onSelect?.({ shiftKey: event.shiftKey, keepOpen: () => { keepOpen = true; } });
    if (!keepOpen) onClose?.("select");
  };
  return (
    <button
      type="button"
      role="menuitem"
      data-popup-menu-item
      aria-disabled={item.disabled ? "true" : undefined}
      aria-checked={item.checked === undefined ? undefined : Boolean(item.checked)}
      className={`sidebar-menu-item${item.danger ? " is-danger" : ""}`}
      title={item.disabled ? item.disabledReason : item.title}
      onClick={activate}
      onKeyDown={(event) => {
        if (event.key === "ArrowDown" || event.key === "ArrowUp" || event.key === "Home" || event.key === "End") {
          event.preventDefault();
          moveFocus(event.currentTarget, event.key);
        } else if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          activate(event);
        } else if (event.key === "Tab") {
          event.preventDefault();
          onClose?.("outside");
        } else if (event.key.length === 1) {
          jumpByLetter(event.currentTarget, event.key);
        }
      }}
    >
      {item.checked === undefined ? null : (
        <span className="sidebar-menu-icon" aria-hidden="true">{item.checked ? "✓" : ""}</span>
      )}
      {item.icon ? <span className="sidebar-menu-icon" aria-hidden="true">{item.icon}</span> : null}
      <span className="sidebar-menu-label">{item.label}</span>
      {item.note ? <span className="sidebar-menu-note">{item.note}</span> : null}
      {item.shortcut ? <span className="sidebar-menu-shortcut">{item.shortcut}</span> : null}
    </button>
  );
}

function Submenu({ item, onClose }) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  const buttonRef = useRef(null);
  const surfaceRef = useRef(null);

  const show = () => {
    const rect = buttonRef.current?.getBoundingClientRect();
    if (!rect) return;
    setPosition({ left: rect.right + 2, top: rect.top });
    setOpen(true);
  };

  useLayoutEffect(() => {
    if (!open || !surfaceRef.current) return;
    const size = surfaceRef.current.getBoundingClientRect();
    const viewport = { left: 0, top: 0, width: window.innerWidth, height: window.innerHeight };
    const anchor = { kind: "rect", rect: buttonRef.current?.getBoundingClientRect() ?? { left: 0, top: 0, right: 0, bottom: 0 }, align: "start" };
    const placed = placeSidebarMenu(anchor, { width: size.width, height: size.height }, viewport);
    // A submenu opens to the right of its item; use the generic rule only for
    // vertical flipping, keeping the horizontal side stable unless it overflows.
    const left = placed.left < (buttonRef.current?.getBoundingClientRect().left ?? 0)
      ? (buttonRef.current?.getBoundingClientRect().left ?? 0) - size.width - 2
      : placed.left - 2;
    setPosition({ left: Math.max(8, Math.min(left, window.innerWidth - size.width - 8)), top: placed.top });
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event) => {
      if (surfaceRef.current?.contains(event.target)) return;
      setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, [open]);

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        role="menuitem"
        data-popup-menu-item
        aria-haspopup="menu"
        aria-expanded={open}
        className="sidebar-menu-item"
        onMouseEnter={show}
        onClick={show}
        onKeyDown={(event) => {
          if (event.key === "ArrowRight" || event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            show();
            requestAnimationFrame(() => focusFirst(surfaceRef.current));
          } else if (event.key === "ArrowLeft") {
            setOpen(false);
          }
        }}
      >
        {item.icon ? <span className="sidebar-menu-icon" aria-hidden="true">{item.icon}</span> : null}
        <span className="sidebar-menu-label">{item.label}</span>
        <span className="sidebar-menu-shortcut" aria-hidden="true">›</span>
      </button>
      {open ? createPortal(
        <div
          ref={surfaceRef}
          role="menu"
          aria-label={item.label}
          data-popup-surface
          className="sidebar-menu"
          style={{ left: position.left, top: position.top, width: 208, maxWidth: "calc(100vw - 16px)" }}
        >
          <MenuEntries items={item.items} onClose={(reason) => { setOpen(false); onClose?.(reason); }} />
        </div>,
        document.body,
      ) : null}
    </>
  );
}

/** Whether a pointer target belongs to this menu, including a portaled submenu. */
function eventInsideMenu(target, surface) {
  if (surface?.contains(target)) return true;
  return Boolean(target?.closest?.("[data-popup-surface]"));
}

function focusableItems(container) {
  if (!container) return [];
  return [...container.querySelectorAll("[data-popup-menu-item]:not([aria-disabled='true'])")];
}

function focusFirst(container) {
  const items = focusableItems(container);
  (items[0] ?? container)?.focus();
}

function moveFocus(current, key) {
  const container = current.closest("[role='menu']");
  const items = focusableItems(container);
  if (items.length === 0) return;
  const index = items.indexOf(current);
  let next = 0;
  if (key === "ArrowDown") next = (index + 1) % items.length;
  else if (key === "ArrowUp") next = (index - 1 + items.length) % items.length;
  else if (key === "End") next = items.length - 1;
  items[next]?.focus();
}

function jumpByLetter(current, letter) {
  const container = current.closest("[role='menu']");
  const wanted = letter.toLowerCase();
  const match = focusableItems(container).find((item) => item.textContent?.trim().toLowerCase().startsWith(wanted));
  match?.focus();
}
