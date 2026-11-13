"use client";

import { useCallback, useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { terminalFontSize, useChatAppearance } from "@/hooks/useChatAppearance";
import { useI18n } from "@/hooks/useI18n";
import { useFontPreferences } from "@/hooks/useFontPreferences";
import { useTerminalAppearance } from "@/hooks/useTerminalAppearance";
import { useTerminalTheme } from "@/hooks/useTerminalTheme";
import {
  resolveTerminalFontSize,
  resolveTerminalFontWeight,
  terminalFontStack,
} from "@/lib/terminal-appearance";
import { createTerminalWriter, TerminalRequestError, terminalRequest } from "@/lib/terminal-client";
import { pageId, trackEventSource } from "@/lib/page-debug";
import {
  applyTerminalModifiers,
  NO_TERMINAL_MODIFIERS,
  terminalExtraKeySequence,
  type TerminalExtraKey,
  type TerminalModifier,
  type TerminalModifiers,
} from "@/lib/terminal-extra-keys";
import type { TerminalEvent } from "@/lib/terminal-manager";
import { TerminalExtraKeys, useTerminalExtraKeysVisible } from "./TerminalExtraKeys";
import type { TerminalTab } from "./terminal-tab-state";

export type TerminalStatus = "connecting" | "ready" | "exited" | "error";

interface Props {
  tab: TerminalTab;
  active: boolean;
  onClosed: () => void;
  onCloseError: () => void;
  /** The saved shell no longer exists; the parent recreates it in the same cwd. */
  onExpired: () => void;
  /** The shell changed directory (OSC 7 or the server's cwd watcher). */
  onCwdChange?: (cwd: string) => void;
  /** Fall back here when a restored cwd no longer exists. */
  fallbackCwd?: string | null;
  /** The replacement shell was created; later remounts should reattach. */
  onStarted?: () => void;
  /** Mark this pane active when something inside it is pressed. */
  onActivate?: () => void;
  /** The connection state, for the dot on the tab that owns this pane. */
  onStatusChange?: (status: TerminalStatus) => void;
  /** The shell process ended; 0 is a deliberate `exit`. */
  onExit?: (exitCode: number) => void;
  /** Extra mobile keys belong to the active pane only. */
  showExtraKeys?: boolean;
  /** Per-pane controls drawn over the terminal, in its own theme. */
  actions?: ReactNode;
}

export function TerminalPanel({
  tab,
  active,
  onClosed,
  onCloseError,
  onExpired,
  onCwdChange,
  fallbackCwd,
  onStarted,
  onActivate,
  onStatusChange,
  onExit,
  showExtraKeys: extraKeysEnabled = true,
  actions,
}: Props) {
  const { t } = useI18n();
  const { id, cwd, restored } = tab;
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const startRef = useRef<Promise<void>>(Promise.resolve());
  const writerRef = useRef<ReturnType<typeof createTerminalWriter> | null>(null);
  const cwdRef = useRef(cwd);
  cwdRef.current = cwd;
  const callbacksRef = useRef({ onClosed, onCloseError, onExpired, onCwdChange, onStarted, onStatusChange, onExit });
  callbacksRef.current = { onClosed, onCloseError, onExpired, onCwdChange, onStarted, onStatusChange, onExit };
  const [status, setStatus] = useState<TerminalStatus>("connecting");
  const [error, setError] = useState<string | null>(null);
  const [exitCode, setExitCode] = useState<number | null>(null);
  const [reconnectKey, setReconnectKey] = useState(0);
  const [modifiers, setModifiers] = useState<TerminalModifiers>(NO_TERMINAL_MODIFIERS);
  const modifiersRef = useRef(modifiers);
  // Always call the hook: `extraKeysEnabled` only decides whether the keys
  // render, never whether this component calls a hook.
  const extraKeysVisible = useTerminalExtraKeysVisible();
  const showExtraKeys = extraKeysEnabled && extraKeysVisible;
  const preset = useTerminalTheme();
  const appearance = useTerminalAppearance();
  const fontPreferences = useFontPreferences();
  const chatFontSize = useChatAppearance().fontSize;
  const fontSize = resolveTerminalFontSize(appearance.fontSize, terminalFontSize(chatFontSize));
  const fontWeight = resolveTerminalFontWeight(appearance.fontWeight, fontPreferences.monoWeight);
  const fontFamily = terminalFontStack(appearance.font, appearance.customFont);
  const appearanceRef = useRef(appearance);
  appearanceRef.current = appearance;
  const presetRef = useRef(preset);
  presetRef.current = preset;
  const fontSizeRef = useRef(fontSize);
  fontSizeRef.current = fontSize;
  const fontWeightRef = useRef(fontWeight);
  fontWeightRef.current = fontWeight;
  const fontFamilyRef = useRef(fontFamily);
  fontFamilyRef.current = fontFamily;
  const refitRef = useRef<() => void>(() => {});

  useEffect(() => {
    callbacksRef.current.onStatusChange?.(status);
  }, [status]);

  const consumeModifiers = useCallback(() => {
    const current = modifiersRef.current;
    if (current.ctrl || current.alt) {
      modifiersRef.current = NO_TERMINAL_MODIFIERS;
      setModifiers(NO_TERMINAL_MODIFIERS);
    }
    return current;
  }, []);
  const sendExtraKey = (key: TerminalExtraKey) => {
    const terminal = terminalRef.current;
    if (!terminal) return;
    const sequence = terminalExtraKeySequence(key, consumeModifiers(), terminal.modes.applicationCursorKeysMode);
    terminal.input(sequence, true);
  };
  const toggleModifier = (modifier: TerminalModifier) => {
    const next = { ...modifiersRef.current, [modifier]: !modifiersRef.current[modifier] };
    modifiersRef.current = next;
    setModifiers(next);
  };

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    let disposed = false;
    let events: EventSource | null = null;
    let offset: number | undefined;
    let connected = false;
    let exited = false;
    let inputFailed = false;
    setStatus("connecting");
    setError(null);
    setExitCode(null);

    const readTerminalFont = () => {
      const style = getComputedStyle(container);
      const inherited = style.getPropertyValue("--font-mono").trim();
      return {
        fontFamily: fontFamilyRef.current ?? (inherited || "monospace"),
        fontWeight: fontWeightRef.current,
      };
    };
    const settings = appearanceRef.current;
    const terminal = new Terminal({
      cursorBlink: settings.cursorBlink,
      ...readTerminalFont(),
      fontSize: fontSizeRef.current,
      lineHeight: settings.lineHeight,
      letterSpacing: settings.letterSpacing,
      cursorStyle: settings.cursorStyle,
      minimumContrastRatio: settings.minimumContrastRatio,
      scrollback: settings.scrollback,
      screenReaderMode: true,
      disableStdin: true,
      theme: presetRef.current.xterm,
    });
    terminalRef.current = terminal;
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(container);
    // xterm measures its cell grid from the font it can see. A bundled webfont
    // may still be arriving, and setting an option re-measures, so re-apply the
    // family through a throwaway one once the file is in.
    const primaryFamily = (fontFamilyRef.current ?? getComputedStyle(container).getPropertyValue("--font-mono"))
      .split(",")[0]?.trim().replace(/^['"]|['"]$/g, "");
    if (primaryFamily && document.fonts && !document.fonts.check(`16px "${primaryFamily}"`)) {
      void document.fonts.load(`16px "${primaryFamily}"`).then(() => {
        if (disposed) return;
        const font = readTerminalFont();
        terminal.options.fontFamily = "monospace";
        terminal.options.fontFamily = font.fontFamily;
      }).catch(() => {
        // The fallback stack stays; it is what xterm already measured.
      });
    }
    terminal.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown") return true;
      const key = event.key.toLowerCase();
      if ((event.ctrlKey || event.metaKey) && key === "v") return false;
      if ((event.ctrlKey || event.metaKey) && key === "c" && terminal.hasSelection()) return false;
      return true;
    });

    // OSC 7 is the portable half of cwd tracking: shells that already report
    // their directory (and the Linux server watcher) both end up here.
    const osc7 = terminal.parser.registerOscHandler(7, (data) => {
      try {
        const url = new URL(data);
        if (url.protocol !== "file:") return false;
        let path = decodeURIComponent(url.pathname);
        if (/^\/[A-Za-z]:\//.test(path)) path = path.slice(1);
        if (path) callbacksRef.current.onCwdChange?.(path);
      } catch {
        // A malformed OSC 7 must not disturb the shell output.
      }
      return true;
    });

    const writer = createTerminalWriter(id, (reason) => {
      if (disposed) return;
      inputFailed = true;
      terminal.options.disableStdin = true;
      setError(reason.message);
      setStatus("error");
    });
    writerRef.current = writer;
    const onData = terminal.onData((data) => {
      if (connected && !exited && !inputFailed) writer.write(applyTerminalModifiers(data, consumeModifiers()));
    });
    const fitAndResize = () => {
      if (!container.offsetWidth || !container.offsetHeight) return;
      fit.fit();
    };
    const onResize = terminal.onResize(({ cols, rows }) => {
      if (!connected || exited || inputFailed) return;
      writer.resize(Math.max(2, cols), Math.max(2, rows));
    });
    refitRef.current = fitAndResize;
    const resizeObserver = new ResizeObserver(fitAndResize);
    resizeObserver.observe(container);

    let verifying = false;
    let verifyTimer: ReturnType<typeof setTimeout> | null = null;
    const verifyTerminal = async () => {
      if (disposed || exited || verifying) return;
      verifying = true;
      try {
        await terminalRequest(`/api/terminal/${encodeURIComponent(id)}`);
        if (!disposed && !exited) connect();
      } catch (reason) {
        if (disposed || exited) return;
        if (reason instanceof TerminalRequestError && reason.status === 404) {
          // The server forgot this PTY (a restart or a long-lived page). The
          // parent replaces the id and creates a shell in the saved cwd.
          callbacksRef.current.onExpired();
        } else {
          setError(reason instanceof Error ? reason.message : String(reason));
          setStatus("error");
        }
      } finally {
        verifying = false;
      }
    };

    const connect = () => {
      if (disposed || exited || !navigator.onLine) return;
      events?.close();
      // `holder`: the page, for the Debug panel's connected pages.
      const query = new URLSearchParams({ holder: pageId() });
      if (offset !== undefined) query.set("after", String(offset));
      events = trackEventSource("terminal", new EventSource(`/api/terminal/${encodeURIComponent(id)}/events?${query}`), cwdRef.current);
      events.onmessage = (message) => {
        const event = JSON.parse(message.data) as TerminalEvent;
        if (event.type === "cwd") {
          callbacksRef.current.onCwdChange?.(event.cwd);
          return;
        }
        if (event.type === "output") {
          if (event.reset) terminal.reset();
          else if (offset !== undefined && event.offset <= offset) return;
          terminal.write(event.data);
          offset = event.offset;
        } else {
          exited = true;
          connected = false;
          terminal.options.disableStdin = true;
          events?.close();
          setExitCode(event.type === "exit" ? event.exitCode : null);
          setStatus("exited");
          if (event.type === "exit") callbacksRef.current.onExit?.(event.exitCode);
        }
      };
      events.onopen = () => {
        connected = true;
        if (inputFailed) return;
        terminal.options.disableStdin = false;
        setStatus("ready");
        fitAndResize();
        writer.resize(Math.max(2, terminal.cols), Math.max(2, terminal.rows));
        if (container.offsetWidth && container.offsetHeight) terminal.focus();
      };
      events.onerror = () => {
        if (disposed || exited) return;
        connected = false;
        terminal.options.disableStdin = true;
        setStatus("connecting");
        if (events?.readyState !== EventSource.CLOSED) return;
        if (verifyTimer) clearTimeout(verifyTimer);
        verifyTimer = setTimeout(() => {
          verifyTimer = null;
          void verifyTerminal();
        }, 500);
      };
    };

    const create = (targetCwd: string) => terminalRequest("/api/terminal", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id, cwd: targetCwd, cols: terminal.cols, rows: terminal.rows }),
    });

    startRef.current = (async () => {
      fitAndResize();
      try {
        if (restored || reconnectKey > 0) {
          // Restoring a tab must never silently launch a replacement shell.
          await terminalRequest(`/api/terminal/${encodeURIComponent(id)}`);
        } else {
          await create(cwdRef.current);
          callbacksRef.current.onStarted?.();
        }
      } catch (reason) {
        if (disposed) return;
        if (!restored && reason instanceof TerminalRequestError
          && (reason.status === 400 || reason.status === 403)
          && fallbackCwd && fallbackCwd !== cwdRef.current) {
          await create(fallbackCwd);
          callbacksRef.current.onCwdChange?.(fallbackCwd);
          callbacksRef.current.onStarted?.();
        } else if (reason instanceof TerminalRequestError && reason.status === 404) {
          callbacksRef.current.onExpired();
          return;
        } else {
          throw reason;
        }
      }
      connect();
    })().catch((reason: Error) => {
      if (disposed) return;
      setError(reason.message);
      setStatus("error");
    });

    const pageHide = () => {
      connected = false;
      terminal.options.disableStdin = true;
      events?.close();
      if (!exited && !inputFailed) setStatus("connecting");
    };
    const pageShow = (event: PageTransitionEvent) => { if (event.persisted) connect(); };
    window.addEventListener("pagehide", pageHide);
    window.addEventListener("pageshow", pageShow);
    window.addEventListener("offline", pageHide);
    window.addEventListener("online", connect);
    return () => {
      disposed = true;
      if (verifyTimer) clearTimeout(verifyTimer);
      events?.close();
      osc7.dispose();
      void writer.stop();
      resizeObserver.disconnect();
      onData.dispose();
      onResize.dispose();
      window.removeEventListener("pagehide", pageHide);
      window.removeEventListener("pageshow", pageShow);
      window.removeEventListener("offline", pageHide);
      window.removeEventListener("online", connect);
      terminal.dispose();
      terminalRef.current = null;
      refitRef.current = () => {};
    };
  }, [id, reconnectKey, consumeModifiers]);

  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal) return;
    const container = containerRef.current;
    const inherited = container ? getComputedStyle(container).getPropertyValue("--font-mono").trim() : "";
    const resolvedFamily = fontFamily ?? (inherited || "monospace");
    terminal.options.fontFamily = resolvedFamily;
    terminal.options.fontWeight = fontWeight;
    terminal.options.fontSize = fontSize;
    terminal.options.lineHeight = appearance.lineHeight;
    terminal.options.letterSpacing = appearance.letterSpacing;
    terminal.options.cursorStyle = appearance.cursorStyle;
    terminal.options.cursorBlink = appearance.cursorBlink;
    terminal.options.minimumContrastRatio = appearance.minimumContrastRatio;
    terminal.options.scrollback = appearance.scrollback;
    terminal.options.theme = preset.xterm;
    // Font metrics changed; a refit sends the new cols/rows to the shell.
    refitRef.current();
    // A face chosen while the panel is open may still be downloading. Measure
    // again once it arrives, or xterm keeps the fallback's cell grid.
    const primary = resolvedFamily.split(",")[0]?.trim().replace(/^['"]|['"]$/g, "");
    if (primary && document.fonts && !document.fonts.check(`16px "${primary}"`)) {
      void document.fonts.load(`16px "${primary}"`).then(() => {
        if (terminalRef.current !== terminal) return;
        terminal.options.fontFamily = "monospace";
        terminal.options.fontFamily = resolvedFamily;
        refitRef.current();
      }).catch(() => {
        // The fallback stack stays; it is what xterm already measured.
      });
    }
  }, [appearance, preset, fontFamily, fontWeight, fontSize, fontPreferences.mono, fontPreferences.monoWeight]);

  useEffect(() => {
    if (active) terminalRef.current?.focus();
  }, [active]);

  useEffect(() => {
    if (!tab.closing) return;
    let cancelled = false;
    if (terminalRef.current) terminalRef.current.options.disableStdin = true;
    void (async () => {
      await startRef.current;
      await writerRef.current?.stop();
      await terminalRequest(`/api/terminal/${encodeURIComponent(id)}`, { method: "DELETE", keepalive: true });
      if (!cancelled) callbacksRef.current.onClosed();
    })().catch((reason: Error) => {
      if (cancelled) return;
      setError(reason.message);
      setStatus("error");
      callbacksRef.current.onCloseError();
    });
    return () => { cancelled = true; };
  }, [id, tab.closing]);

  const themeStyle = {
    "--terminal-bg": preset.chrome.background,
    "--terminal-header-bg": preset.chrome.panel,
    "--terminal-border": preset.chrome.border,
    "--terminal-button-border": preset.chrome.border,
    "--terminal-hover-bg": preset.chrome.hover,
    "--terminal-muted": preset.chrome.muted,
    "--terminal-strong": preset.chrome.strong,
    "--terminal-key": preset.chrome.key,
    "--terminal-accent": preset.chrome.accent,
    "--terminal-error-border": preset.chrome.errorBorder,
    "--terminal-error-bg": preset.chrome.errorBackground,
    "--terminal-error-text": preset.chrome.errorText,
    "--terminal-scrollbar": preset.chrome.scrollbar,
    colorScheme: preset.kind,
  } as CSSProperties;

  return (
    <section
      className="terminal-panel"
      data-terminal-theme={preset.kind}
      data-terminal-preset={preset.id}
      data-status={status}
      style={themeStyle}
      aria-label={t("terminal.title")}
      onPointerDown={() => onActivate?.()}
    >
      <div>
        {error && <div className="terminal-panel-error" role="alert">{error}</div>}
        {status === "exited" && <div className="terminal-panel-exit" role="status">{exitCode === null ? t("terminal.exited") : t("terminal.exitCode", { code: exitCode })}</div>}
      </div>
      <div className="terminal-xterm"><div ref={containerRef} className="terminal-xterm-host" /></div>
      {showExtraKeys && <TerminalExtraKeys modifiers={modifiers} onKey={sendExtraKey} onToggleModifier={toggleModifier} />}
      {actions}
    </section>
  );
}
