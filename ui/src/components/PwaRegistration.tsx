"use client";

import { useEffect } from "react";
import { isDesktop } from "@/lib/desktop";

export function PwaRegistration() {
  useEffect(() => {
    // The desktop shell has no service worker story (and its origin is a
    // stable localhost port); skip registration there.
    if (isDesktop() || !import.meta.env.PROD || !("serviceWorker" in navigator)) {
      return;
    }

    const register = () => {
      const appVersion = import.meta.env.VITE_APP_VERSION ?? "dev";
      const scriptUrl = `/sw.js?v=${encodeURIComponent(appVersion)}`;

      void navigator.serviceWorker.register(scriptUrl, {
        scope: "/",
        updateViaCache: "none",
      }).catch((error: unknown) => {
        console.error("Failed to register the Pi Star service worker:", error);
      });
    };

    if (document.readyState === "complete") {
      register();
      return;
    }

    window.addEventListener("load", register, { once: true });
    return () => window.removeEventListener("load", register);
  }, []);

  return null;
}
