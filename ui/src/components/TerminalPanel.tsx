"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { terminalFontSize, useChatAppearance } from "@/hooks/useChatAppearance";
import { useI18n } from "@/hooks/useI18n";
import { setTerminalTheme, useTerminalTheme } from "@/hooks/useTerminalTheme";
import { subscribeFontPreferences } from "@/hooks/useFontPreferences";
import { readFontWeight } from "@/lib/font-preferences";
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
import { TERMINAL_XTERM_THEMES } from "@/lib/terminal-theme";
import { TerminalExtraKeys, useTerminalExtraKeysVisible } from "./TerminalExtraKeys";
import type { TerminalTab } from "./terminal-tab-state";

interface Props {
  tab: TerminalTab;
  active: boolean;
  onRestart: () => void;
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
  /** Present in split panes: close this pane instead of only the whole tab. */
  onClose?: () => void;
  /** Extra mobile keys belong to the active pane only. */
  showExtraKeys?: boolean;
}

export function TerminalPanel({
  tab,
  active,
  onRestart,
  onClosed,
  onCloseError,
  onExpired,
  onCwdChange,
  fallbackCwd,
  onStarted,
  onActivate,
  onClose,
  showExtraKeys: extraKeysEnabled = true,
}: Props) {
  const { t } = useI18n();
  const { id, cwd, restored } = tab;
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const startRef = useRef<Promise<void>>(Promise.resolve());
  const writerRef = useRef<ReturnType<typeof createTerminalWriter> | null>(null);
  const cwdRef = useRef(cwd);
  cwdRef.current = cwd;
  const callbacksRef = useRef({ onClosed, onCloseError, onExpired, onCwdChange, onStarted });
  callbacksRef.current = { onClosed, onCloseError, onExpired, onCwdChange, onStarted };
  const [status, setStatus] = useState<"connecting" | "ready" | "exited" | "error">("connecting");
  const [error, setError] = useState<string | null>(null);
  const [exitCode, setExitCode] = useState<number | null>(null);
  const [reconnectKey, setReconnectKey] = useState(0);
  const [modifiers, setModifiers] = useState<TerminalModifiers>(NO_TERMINAL_MODIFIERS);
  const modifiersRef = useRef(modifiers);
  // Always call the hook: `extraKeysEnabled` only decides whether the keys
  // render, never whether this component calls a hook.
  const extraKeysVisible = useTerminalExtraKeysVisible();
  const showExtraKeys = extraKeysEnabled && extraKeysVisible;
  const theme = useTerminalTheme();
  const themeRef = useRef(theme);
  themeRef.current = theme;
  const fontSize = terminalFontSize(useChatAppearance().fontSize);
  const fontSizeRef = useRef(fontSize);
  fontSizeRef.current = fontSize;
  const refitRef = useRef<() => void>(() => {});

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
      return {
        fontFamily: style.getPropertyValue("--font-mono").trim() || "monospace",
        // Base weights stop at 600, so xterm's default bold (700) stays heavier.
        fontWeight: readFontWeight(style.getPropertyValue("--font-mono-weight")),
      };
    };
    const terminal = new Terminal({
      cursorBlink: true,
      ...readTerminalFont(),
      fontSize: fontSizeRef.current,
      lineHeight: 1.25,
      scrollback: 8000,
      screenReaderMode: true,
      disableStdin: true,
      theme: TERMINAL_XTERM_THEMES[themeRef.current],
    });
    terminalRef.current = terminal;
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(container);
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
    const updateFont = () => {
      if (disposed) return;
      const font = readTerminalFont();
      terminal.options.fontFamily = font.fontFamily;
      terminal.options.fontWeight = font.fontWeight;
      fitAndResize();
    };
    const unsubscribeFonts = subscribeFontPreferences(updateFont);

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
      unsubscribeFonts();
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
    if (terminalRef.current) terminalRef.current.options.theme = TERMINAL_XTERM_THEMES[theme];
  }, [theme]);

  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal || terminal.options.fontSize === fontSize) return;
    // A refit sends the new cols/rows to the shell through onResize, as a font change does.
    terminal.options.fontSize = fontSize;
    refitRef.current();
  }, [fontSize]);

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

  const nextTheme = theme === "dark" ? "light" : "dark";
  const themeLabel = nextTheme === "light" ? "terminal.lightTheme" : "terminal.darkTheme";

  return (
    <section
      className="terminal-panel"
      data-terminal-theme={theme}
      aria-label={t("terminal.title")}
      onPointerDown={() => onActivate?.()}
    >
      <header className="terminal-panel-header">
        <div className="terminal-panel-path">
          <span className={`terminal-status-dot is-${status}`} title={t(`terminal.${status}`)} />
          <span title={cwd}>{cwd}</span>
        </div>
        <div className="terminal-panel-actions">
          {status === "error" && (
            <button type="button" onClick={() => setReconnectKey((key) => key + 1)} disabled={Boolean(tab.closing)} title={t("terminal.reconnect")} aria-label={t("terminal.reconnect")}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-2 2M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l2-2" />
              </svg>
            </button>
          )}
          <button type="button" onClick={() => setTerminalTheme(nextTheme)} title={t(themeLabel)} aria-label={t(themeLabel)}>
            {nextTheme === "light" ? (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
              </svg>
            ) : (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
              </svg>
            )}
          </button>
          {onClose && (
            <button type="button" onClick={onClose} disabled={Boolean(tab.closing)} title={t("terminal.closePane")} aria-label={t("terminal.closePane")}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
                <line x1="6" y1="6" x2="18" y2="18" /><line x1="18" y1="6" x2="6" y2="18" />
              </svg>
            </button>
          )}
          <button type="button" onClick={onRestart} disabled={Boolean(tab.closing)} title={t("terminal.restart")} aria-label={t("terminal.restart")}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M20 11a8 8 0 1 0-2.34 5.66" /><polyline points="20 4 20 11 13 11" />
            </svg>
          </button>
        </div>
      </header>
      <div>
        {error && <div className="terminal-panel-error" role="alert">{error}</div>}
        {status === "exited" && <div className="terminal-panel-exit" role="status">{exitCode === null ? t("terminal.exited") : t("terminal.exitCode", { code: exitCode })}</div>}
      </div>
      <div className="terminal-xterm"><div ref={containerRef} className="terminal-xterm-host" /></div>
      {showExtraKeys && <TerminalExtraKeys modifiers={modifiers} onKey={sendExtraKey} onToggleModifier={toggleModifier} />}
    </section>
  );
}
