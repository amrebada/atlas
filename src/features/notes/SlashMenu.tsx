import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import type { SlashCommand } from "./slash-commands";

// Atlas - Tiptap slash menu popup.

interface SlashMenuProps {
  /** Viewport-space top-left for the popup. */
  x: number;
  y: number;
  /** Query string shown in the header (`query = ""` → "Blocks"). */
  query: string;
  items: SlashCommand[];
  activeIdx: number;
  onPick: (cmd: SlashCommand) => void;
  onHover: (idx: number) => void;
}

export function SlashMenu({
  x,
  y,
  query,
  items,
  activeIdx,
  onPick,
  onHover,
}: SlashMenuProps) {
  const listRef = useRef<HTMLDivElement>(null);

  // Keep the active row scrolled into view when arrow keys change activeIdx.
  useEffect(() => {
    const el = listRef.current?.querySelector(
      `[data-active="true"]`,
    ) as HTMLElement | null;
    if (el) el.scrollIntoView({ block: "nearest" });
  }, [activeIdx, items.length]);

  if (items.length === 0) return null;

  return createPortal(
    <div
      ref={listRef}
      // `mousedown` on the menu would otherwise steal focus from the editor
      onMouseDown={(e) => e.preventDefault()}
      className="fixed z-[200] rounded-[6px] overflow-y-auto"
      style={{
        left: Math.max(8, x),
        top: y,
        minWidth: 260,
        maxWidth: 320,
        maxHeight: 320,
        background: "var(--surface)",
        border: "1px solid var(--line)",
        boxShadow:
          "0 10px 30px rgba(0,0,0,0.35), 0 2px 6px rgba(0,0,0,0.2)",
        fontFamily: "var(--sans)",
        padding: 4,
      }}
      role="listbox"
    >
      <div
        className="px-[8px] pt-[4px] pb-[6px] mb-[2px] border-b border-line-soft font-mono text-[10px] uppercase tracking-[0.6px] text-text-dimmer"
      >
        {query ? `/${query}` : "Blocks"} · {items.length}
      </div>
      {items.map((item, i) => {
        const active = i === activeIdx;
        return (
          <div
            key={item.id}
            data-active={active}
            role="option"
            aria-selected={active}
            onMouseEnter={() => onHover(i)}
            onClick={() => onPick(item)}
            className="flex items-center gap-[10px] px-[8px] py-[6px] rounded-[4px] cursor-pointer"
            style={{
              background: active ? "var(--row-active)" : "transparent",
              color: "var(--text)",
            }}
          >
            <div
              className="w-[26px] h-[26px] shrink-0 inline-flex items-center justify-center rounded-[4px] font-mono text-[11px]"
              style={{
                background: "var(--surface-2)",
                border: "1px solid var(--line)",
                color: active ? "var(--accent)" : "var(--text-dim)",
              }}
            >
              {item.kbd}
            </div>
            <div className="flex-1 min-w-0">
              <div className="text-[13px] font-medium leading-tight">
                {item.label}
              </div>
              <div className="text-[11px] text-text-dim leading-snug">
                {item.hint}
              </div>
            </div>
          </div>
        );
      })}
      <div
        className="pt-[6px] px-[8px] pb-[4px] mt-[2px] border-t border-line-soft font-mono text-[10px] text-text-dimmer flex gap-[10px]"
      >
        <span>↑↓ nav</span>
        <span>↵ select</span>
        <span>esc cancel</span>
      </div>
    </div>,
    document.body,
  );
}
