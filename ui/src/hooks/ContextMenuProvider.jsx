"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ContextMenuHost } from "@/components/ContextMenuHost.jsx";
import { resolveContextMenu } from "@/lib/context-menu/registry.js";
import { ContextMenuContext } from "@/hooks/useContextMenu.js";
import { useI18n } from "@/hooks/useI18n";

const LONG_PRESS_MS = 500;
const LONG_PRESS_SLOP_PX = 10;

export function ContextMenuProvider({ children }) {
  const { t } = useI18n();
  const [request, setRequest] = useState(null);

  const open = useCallback((next) => {
    const active = typeof document !== "undefined" && document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    setRequest({
      ariaLabel: "Menu",
      ...next,
      returnFocusTo: next.returnFocusTo ?? active,
    });
  }, []);

  const close = useCallback(() => setRequest(null), []);
  const value = useMemo(() => ({ open, close }), [open, close]);

  // Shift+F10 / the keyboard's ContextMenu key opens the menu of the focused
  // element; touch long-press does the same for opted-in elements.
  const openRef = useRef(open);
  openRef.current = open;
  useEffect(() => {
    const openFromEvent = (target, x, y) => {
      if (!(target instanceof Element) || typeof MouseEvent === "undefined") return;
      const event = new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        clientX: x,
        clientY: y,
        button: 2,
      });
      // Keyboard and long-press open the app menu even over a text selection,
      // unlike a pointer right-click, which leaves the system menu in charge.
      event.piStarSyntheticMenu = true;
      target.dispatchEvent(event);
      if (event.defaultPrevented) return;
      const items = resolveContextMenu(target);
      if (items) {
        openRef.current({
          anchor: { kind: "point", x, y },
          items,
          ariaLabel: target.dataset.contextMenuLabel ?? "Menu",
        });
      }
    };

    const onKeyDown = (event) => {
      const isKeyboardMenu = (event.key === "F10" && event.shiftKey) || event.key === "ContextMenu";
      if (!isKeyboardMenu) return;
      const active = document.activeElement;
      if (!(active instanceof HTMLElement) || active === document.body) return;
      const rect = active.getBoundingClientRect();
      event.preventDefault();
      openFromEvent(active, rect.left + Math.min(rect.width, 24), rect.top + Math.min(rect.height, 24));
    };

    let timer = null;
    let startX = 0;
    let startY = 0;
    let pressTarget = null;
    const cancelPress = () => {
      if (timer !== null) clearTimeout(timer);
      timer = null;
      pressTarget = null;
    };
    const onPointerDown = (event) => {
      if (event.pointerType === "mouse" || event.button !== 0) return;
      const target = event.target instanceof Element ? event.target.closest("[data-context-menu]") : null;
      if (!(target instanceof HTMLElement)) return;
      cancelPress();
      pressTarget = target;
      startX = event.clientX;
      startY = event.clientY;
      timer = setTimeout(() => {
        timer = null;
        const element = pressTarget;
        pressTarget = null;
        if (element) openFromEvent(element, startX, startY);
      }, LONG_PRESS_MS);
    };
    const onPointerMove = (event) => {
      if (timer === null) return;
      if (Math.hypot(event.clientX - startX, event.clientY - startY) > LONG_PRESS_SLOP_PX) cancelPress();
    };

    document.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("pointermove", onPointerMove, true);
    document.addEventListener("pointerup", cancelPress, true);
    document.addEventListener("pointercancel", cancelPress, true);
    return () => {
      cancelPress();
      document.removeEventListener("keydown", onKeyDown, true);
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("pointermove", onPointerMove, true);
      document.removeEventListener("pointerup", cancelPress, true);
      document.removeEventListener("pointercancel", cancelPress, true);
    };
  }, []);

  return (
    <ContextMenuContext.Provider value={value}>
      {children}
      <ContextMenuHost request={request} cancelLabel={t("sidebar.cancel")} onClose={close} />
    </ContextMenuContext.Provider>
  );
}
