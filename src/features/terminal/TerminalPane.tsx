import { useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Pane } from "../../types";
import { terminalResize, terminalWrite } from "../../ipc";
import { useTerminalStore } from "./layout";

// Atlas - single xterm.js pane.

interface TerminalPaneProps {
  pane: Pane;
  focused?: boolean;
  // When true, clicking the pane body promotes this pane to active.
  onFocus?: () => void;
}

export function TerminalPane({ pane, focused, onFocus }: TerminalPaneProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const patchPaneStatus = useTerminalStore((s) => s.patchPaneStatus);

  useEffect(() => {
    let disposed = false;
    let term: import("xterm").Terminal | null = null;
    let fit: import("xterm-addon-fit").FitAddon | null = null;
    let dataUnlisten: UnlistenFn | null = null;
    let exitUnlisten: UnlistenFn | null = null;
    let resizeObs: ResizeObserver | null = null;
    let themeObs: MutationObserver | null = null;
    let opened = false; // term.open() has been called AND container had non-zero dims
    const pendingChunks: (string | Uint8Array)[] = []; // buffered PTY output until we're opened

    (async () => {
      // Lazy-import so the first-paint bundle stays small.
      const [{ Terminal }, { FitAddon }] = await Promise.all([
        import("xterm"),
        import("xterm-addon-fit"),
      ]);
      // Side-effect CSS for xterm styling (.xterm / .xterm-screen rules).
      await import("xterm/css/xterm.css");

      if (disposed || !hostRef.current) return;

      term = new Terminal({
        fontFamily:
          'IBM Plex Mono, "JetBrains Mono", "SF Mono", ui-monospace, monospace',
        fontSize: 12,
        lineHeight: 1.25,
        cursorBlink: true,
        allowProposedApi: true,
        scrollback: 5000,
        theme: readTheme(),
      });
      fit = new FitAddon();
      term.loadAddon(fit);

      // Forward keystrokes to the PTY. Safe to register before open().
      term.onData((data) => {
        terminalWrite(pane.id, data).catch(() => {});
      });

      // `term.open()` must run against a container that has non-zero
      const tryOpen = () => {
        if (opened || disposed || !term || !fit || !hostRef.current) return;
        const { width, height } = hostRef.current.getBoundingClientRect();
        if (width < 1 || height < 1) return;
        try {
          term.open(hostRef.current);
          fit.fit();
          terminalResize(pane.id, term.cols, term.rows).catch(() => {});
          opened = true;
          if (pendingChunks.length > 0) {
            for (const chunk of pendingChunks) term.write(chunk);
            pendingChunks.length = 0;
          }
        } catch {
          /* try again on next resize tick */
        }
      };

      // Stream output. Buffer chunks while the terminal isn't opened yet so
      // serde-json serializes Rust `&[u8]` as an array of numbers; xterm
      // needs string | Uint8Array, so normalise here.
      type RawChunk = string | number[] | Uint8Array;
      type DataPayload = string | { chunk: RawChunk };
      const dataPromise = listen<DataPayload>(
        `terminal:data:${pane.id}`,
        (e) => {
          const raw: RawChunk | undefined =
            typeof e.payload === "string" ? e.payload : e.payload?.chunk;
          if (raw == null) return;
          let chunk: string | Uint8Array;
          if (typeof raw === "string") {
            chunk = raw;
          } else if (raw instanceof Uint8Array) {
            chunk = raw;
          } else if (Array.isArray(raw)) {
            chunk = new Uint8Array(raw);
          } else {
            return;
          }
          if (chunk.length === 0) return;
          if (opened && term) {
            term.write(chunk);
          } else {
            pendingChunks.push(chunk);
          }
        },
      );
      dataPromise.then((fn) => {
        if (disposed) fn();
        else dataUnlisten = fn;
      });

      const exitPromise = listen<{ code: number | null } | number | null>(
        `terminal:exit:${pane.id}`,
        (e) => {
          const code =
            typeof e.payload === "number"
              ? e.payload
              : e.payload && typeof e.payload === "object"
                ? e.payload.code
                : null;
          const status =
            code === 0 ? "idle" : code == null ? "error" : "error";
          patchPaneStatus(pane.id, status);
          const label = code == null ? "exit: terminated" : `exit: ${code}`;
          const marker = `\r\n\x1b[38;5;244m── ${label} ──\x1b[0m\r\n`;
          if (opened && term) {
            term.write(marker);
          } else {
            pendingChunks.push(marker);
          }
        },
      );
      exitPromise.then((fn) => {
        if (disposed) fn();
        else exitUnlisten = fn;
      });

      // ResizeObserver handles both the initial open() and subsequent reflows.
      resizeObs = new ResizeObserver((entries) => {
        const entry = entries[0];
        const w = entry?.contentRect.width ?? 0;
        const h = entry?.contentRect.height ?? 0;
        if (w < 1 || h < 1) return;
        if (!opened) {
          tryOpen();
          return;
        }
        if (!term || !fit) return;
        try {
          fit.fit();
          terminalResize(pane.id, term.cols, term.rows).catch(() => {});
        } catch {
          /* swallow - layout jitters */
        }
      });
      resizeObs.observe(hostRef.current);

      themeObs = new MutationObserver(() => {
        if (term) term.options.theme = readTheme();
      });
      themeObs.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ["data-term-theme"],
      });

      // Defer the initial open() to rAF so StrictMode's dev-mode
      requestAnimationFrame(() => {
        if (disposed) return;
        tryOpen();
      });
    })();

    return () => {
      disposed = true;
      if (resizeObs) resizeObs.disconnect();
      if (themeObs) themeObs.disconnect();
      if (dataUnlisten) dataUnlisten();
      if (exitUnlisten) exitUnlisten();
      // xterm.js stores a WebGL/canvas context - disposing is mandatory.
      try {
        term?.dispose();
      } catch {
        /* best effort */
      }
    };
  }, [pane.id, patchPaneStatus]);

  return (
    <div
      onClick={onFocus}
      className="flex flex-col h-full w-full min-h-0 min-w-0"
      style={{
        background: "var(--term-bg)",
        outline: focused ? "1px solid var(--accent)" : "none",
        outlineOffset: -1,
      }}
    >
      <div
        ref={hostRef}
        className="flex-1 min-h-0 min-w-0"
        style={{ padding: "6px 8px" }}
      />
    </div>
  );
}

// Read an xterm theme from the current Atlas tokens. xterm's color parser
function readTheme(): import("xterm").ITheme {
  const root = document.documentElement;
  const css = getComputedStyle(root);
  const resolve = (name: string, fallback: string): string => {
    const raw = css.getPropertyValue(name).trim();
    if (!raw) return fallback;
    try {
      const probe = document.createElement("span");
      probe.style.color = raw;
      probe.style.display = "none";
      document.body.appendChild(probe);
      const out = getComputedStyle(probe).color;
      probe.remove();
      return out || fallback;
    } catch {
      return fallback;
    }
  };
  return {
    background: resolve("--term-bg", "#111111"),
    foreground: resolve("--term-fg", "#eeeeee"),
    cursor: resolve("--term-cursor", "#7c7fee"),
    cursorAccent: resolve("--term-cursor-fg", "#111111"),
    selectionBackground: resolve(
      "--term-selection-bg",
      "rgba(124, 127, 238, 0.25)",
    ),
    selectionForeground: undefined,
  };
}
