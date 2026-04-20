import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { Kbd } from "../../components/Icon";

// Atlas - keyboard shortcut cheat-sheet overlay.

interface Shortcut {
  /** Human-readable label, e.g. "Open command palette". */
  label: string;
  /** Array of tokens rendered as `<Kbd>`; order matters. */
  keys: string[];
}

interface ShortcutGroup {
  heading: string;
  rows: Shortcut[];
}

const SHORTCUTS: ShortcutGroup[] = [
  {
    heading: "Navigate",
    rows: [
      { label: "Open command palette", keys: ["⌘", "K"] },
      { label: "Open settings", keys: ["⌘", ","] },
      { label: "Show this cheat sheet", keys: ["?"] },
      { label: "Close overlay / cancel", keys: ["Esc"] },
    ],
  },
  {
    heading: "Projects",
    rows: [
      { label: "New project", keys: ["⌘", "N"] },
      { label: "Clone from git URL", keys: ["⌘", "⇧", "N"] },
      { label: "Open selected in editor", keys: ["⌘", "E"] },
    ],
  },
  {
    heading: "Terminal",
    rows: [
      { label: "New terminal pane", keys: ["⌃", "`"] },
      { label: "Maximize terminal strip", keys: ["⌃", "⌘", "F"] },
    ],
  },
  {
    heading: "Inspector",
    rows: [
      { label: "Navigate palette results", keys: ["↑", "↓"] },
      { label: "Open palette result", keys: ["↵"] },
      { label: "Reveal palette result in Finder", keys: ["⌘", "↵"] },
    ],
  },
];

// Returns `true` if the currently focused element would swallow a typed
function isTextFocusActive(): boolean {
  const el = document.activeElement as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  if (el.isContentEditable) return true;
  // Role-based fallback for rich editors that use `role="textbox"` without
  if (el.getAttribute("role") === "textbox") return true;
  return false;
}

export function CheatSheetOverlay() {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && open) {
        e.preventDefault();
        setOpen(false);
        return;
      }
      // `?` is shift+/ on US layouts but `e.key` resolves to "?" directly.
      const isQuestion = e.key === "?" || (e.key === "/" && e.shiftKey);
      if (!isQuestion) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (isTextFocusActive()) return;
      e.preventDefault();
      setOpen((v) => !v);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  if (!open) return null;

  return createPortal(
    <div
      onClick={() => setOpen(false)}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 600,
        background: "rgba(0,0,0,0.45)",
        backdropFilter: "blur(4px)",
        WebkitBackdropFilter: "blur(4px)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: 20,
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Keyboard shortcuts"
        onClick={(e) => e.stopPropagation()}
        style={{
          width: 640,
          maxWidth: "100%",
          maxHeight: "80vh",
          overflowY: "auto",
          background: "var(--palette-bg)",
          border: "1px solid var(--line)",
          borderRadius: 10,
          backdropFilter: "blur(30px) saturate(180%)",
          WebkitBackdropFilter: "blur(30px) saturate(180%)",
          boxShadow: "0 30px 80px rgba(0,0,0,0.5)",
          color: "var(--text)",
          fontFamily: "var(--sans)",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            padding: "14px 18px",
            borderBottom: "1px solid var(--line)",
          }}
        >
          <div style={{ fontSize: 14, fontWeight: 600 }}>
            Keyboard shortcuts
          </div>
          <div style={{ flex: 1 }} />
          <button
            type="button"
            onClick={() => setOpen(false)}
            aria-label="Close"
            style={{
              background: "transparent",
              border: "none",
              color: "var(--text-dim)",
              cursor: "pointer",
              fontSize: 18,
              width: 24,
              height: 24,
              lineHeight: 1,
            }}
          >
            ×
          </button>
        </div>

        <div
          style={{
            display: "grid",
            gridTemplateColumns: "1fr 1fr",
            gap: "18px 32px",
            padding: 18,
          }}
        >
          {SHORTCUTS.map((group) => (
            <div key={group.heading}>
              <div
                style={{
                  fontSize: 10,
                  fontFamily: "var(--mono)",
                  color: "var(--text-dim)",
                  textTransform: "uppercase",
                  letterSpacing: 0.6,
                  marginBottom: 8,
                }}
              >
                {group.heading}
              </div>
              <div style={{ display: "flex", flexDirection: "column" }}>
                {group.rows.map((row) => (
                  <div
                    key={row.label}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      padding: "7px 0",
                      borderBottom: "1px solid var(--line-soft)",
                      fontSize: 12,
                      gap: 10,
                    }}
                  >
                    <span style={{ flex: 1 }}>{row.label}</span>
                    <span style={{ display: "flex", gap: 3 }}>
                      {row.keys.map((k, i) => (
                        <Kbd key={i}>{k}</Kbd>
                      ))}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>

        <div
          style={{
            display: "flex",
            gap: 14,
            padding: "10px 18px",
            borderTop: "1px solid var(--line)",
            fontSize: 10,
            fontFamily: "var(--mono)",
            color: "var(--text-dim)",
          }}
        >
          <span>
            press <Kbd>?</Kbd> any time
          </span>
          <span style={{ flex: 1 }} />
          <span>
            <Kbd>Esc</Kbd> close
          </span>
        </div>
      </div>
    </div>,
    document.body,
  );
}
