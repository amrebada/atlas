import { create } from "zustand";
import type { Pane, PaneId, PaneKind, PaneStatus } from "../../types";

// Atlas - terminal pane store.

export type LayoutMode = "tabs" | "split-v" | "split-h" | "grid";

interface TerminalState {
  panes: Pane[];
  activePaneId: PaneId | null;
  layout: LayoutMode;
  maxed: boolean;
  // When true, the strip collapses to just its top bar (tabs + toolbar)
  collapsed: boolean;

  // Append a pane + auto-focus it. Promotes tabs → grid when pane count
  addPane: (pane: Pane) => void;
  closePane: (id: PaneId) => void;
  closeAll: () => void;
  setLayout: (l: LayoutMode) => void;
  setMaxed: (b: boolean) => void;
  setCollapsed: (b: boolean) => void;
  setActive: (id: PaneId | null) => void;
  patchPane: (id: PaneId, patch: Partial<Pane>) => void;
  patchPaneStatus: (id: PaneId, status: PaneStatus) => void;
  // Swap a pane's id (e.g. after a rerun spawned a fresh PTY). Keeps
  replacePaneId: (oldId: PaneId, newId: PaneId, patch?: Partial<Pane>) => void;
  // Replace the whole pane set - used when restoring a saved layout on
  restore: (
    next: { panes: Pane[]; layout: LayoutMode; activePaneId: PaneId | null },
  ) => void;
}

export const useTerminalStore = create<TerminalState>((set) => ({
  panes: [],
  activePaneId: null,
  layout: "tabs",
  maxed: false,
  collapsed: false,

  addPane: (pane) =>
    set((s) => {
      // Avoid accidental duplicates (e.g. a race where the backend returns
      if (s.panes.some((p) => p.id === pane.id)) {
        return { activePaneId: pane.id };
      }
      const next = [...s.panes, pane];
      const promoteToGrid = s.layout === "tabs" && next.length >= 2;
      return {
        panes: next,
        activePaneId: pane.id,
        layout: promoteToGrid ? "grid" : s.layout,
        // Auto-expand when the user opens a new pane. They just asked for
        collapsed: false,
      };
    }),

  closePane: (id) =>
    set((s) => {
      const next = s.panes.filter((p) => p.id !== id);
      let active = s.activePaneId;
      if (active === id) {
        active = next.length > 0 ? next[next.length - 1].id : null;
      }
      return {
        panes: next,
        activePaneId: active,
        // Leaving max when the last pane closes keeps the sidebar/inspector
        maxed: next.length === 0 ? false : s.maxed,
        collapsed: next.length === 0 ? false : s.collapsed,
      };
    }),

  closeAll: () =>
    set({
      panes: [],
      activePaneId: null,
      maxed: false,
      collapsed: false,
      layout: "tabs",
    }),

  setLayout: (layout) => set({ layout }),
  setMaxed: (maxed) =>
    // Entering maxed cancels collapse; the two states are mutually
    set(maxed ? { maxed: true, collapsed: false } : { maxed: false }),
  setCollapsed: (collapsed) =>
    set(collapsed ? { collapsed: true, maxed: false } : { collapsed: false }),
  setActive: (activePaneId) => set({ activePaneId }),

  patchPane: (id, patch) =>
    set((s) => ({
      panes: s.panes.map((p) => (p.id === id ? { ...p, ...patch } : p)),
    })),
  patchPaneStatus: (id, status) =>
    set((s) => ({
      panes: s.panes.map((p) => (p.id === id ? { ...p, status } : p)),
    })),

  replacePaneId: (oldId, newId, patch) =>
    set((s) => ({
      panes: s.panes.map((p) =>
        p.id === oldId ? { ...p, ...patch, id: newId } : p,
      ),
      activePaneId: s.activePaneId === oldId ? newId : s.activePaneId,
    })),

  restore: (next) =>
    set({
      panes: next.panes,
      layout: next.layout,
      activePaneId: next.activePaneId,
      maxed: false,
    }),
}));

// Persisted layout shape exchanged with `pane_layout_get` / `pane_layout_save`.
export interface PaneLayout {
  mode: LayoutMode;
  panes: Pane[];
  activePaneId: PaneId | null;
}

/** Helper: current store value → persisted shape. */
export function snapshotLayout(): PaneLayout {
  const { panes, layout, activePaneId } = useTerminalStore.getState();
  return { mode: layout, panes, activePaneId };
}

// Lightweight `Pane` factory - builds a minimum-viable pane from a pane id
export function makePane(
  id: PaneId,
  kind: PaneKind,
  cwd: string,
  title: string,
  extras: Partial<Pane> = {},
): Pane {
  return {
    id,
    kind,
    cwd,
    title,
    status: kind === "script" ? "running" : "idle",
    ...extras,
  };
}
