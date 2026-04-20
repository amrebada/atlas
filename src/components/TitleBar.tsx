import { Icon, Kbd } from "./Icon";
import type { Project } from "../types";

interface TitleBarProps {
  project: Project | null;
  onOpenPalette: () => void;
  onNew: () => void;
  onSettings: () => void;
}

// Atlas - macOS-style title bar.
export function TitleBar({
  project,
  onOpenPalette,
  onNew,
  onSettings,
}: TitleBarProps) {
  return (
    <div
      data-tauri-drag-region
      className="flex items-center gap-[14px] h-9 pr-[14px] border-b border-line bg-chrome"
      style={{ gridColumn: "1 / -1", paddingLeft: 78 }}
    >
      {/* Breadcrumb — children inherit drag behavior from the ancestor. */}
      <div
        data-tauri-drag-region
        className="flex items-center gap-[6px] text-text-dim text-xs"
      >
        <Icon name="folder" size={12} />
        <span>Atlas</span>
        <Icon name="chevron" size={10} />
        <span className="text-text">{project?.name ?? "All Projects"}</span>
      </div>

      <div data-tauri-drag-region className="flex-1" />

      {/* ⌘K pseudo-input — non-dragging so clicks reach the button. */}
      <button
        data-tauri-drag-region="false"
        onClick={onOpenPalette}
        aria-label="Open command palette"
        className="inline-flex items-center gap-2 px-[10px] h-6 bg-surface-2 border border-line rounded-[5px] text-text-dim text-xs whitespace-nowrap shrink-0 hover:text-text"
      >
        <Icon name="search" size={12} />
        <span>Find anything</span>
        <span className="ml-6 flex gap-[3px]">
          <Kbd>⌘</Kbd>
          <Kbd>K</Kbd>
        </span>
      </button>

      <button
        data-tauri-drag-region="false"
        onClick={onNew}
        title="New project"
        aria-label="New project"
        className="w-6 h-6 bg-transparent border border-line rounded-[5px] text-text-dim inline-flex items-center justify-center hover:text-text"
      >
        <Icon name="plus" size={12} />
      </button>
      <button
        data-tauri-drag-region="false"
        onClick={onSettings}
        title="Settings"
        aria-label="Settings"
        className="w-6 h-6 bg-transparent border border-line rounded-[5px] text-text-dim inline-flex items-center justify-center hover:text-text"
      >
        <Icon name="gear" size={12} />
      </button>
    </div>
  );
}
