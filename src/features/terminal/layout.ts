import { create } from "zustand";
import type { Pane, PaneId, PaneKind, PaneStatus } from "../../types";

// Atlas - terminal pane store.

export type LayoutMode = "tabs" | "split-v" | "split-h" | "grid";
export type GroupId = string;

// A group bundles an ordered list of panes (referenced by id) plus its own
// view state. Switching groups never tears down panes — every group's
// PaneArea remains mounted (just hidden), so PTYs and xterm scrollback in
// inactive groups keep running.
export interface TerminalGroup {
  id: GroupId;
  name: string;
  paneIds: PaneId[];
  layout: LayoutMode;
  activePaneId: PaneId | null;
}

interface TerminalState {
  // Flat live-pane list. A pane id is referenced by exactly one group's
  // `paneIds`. Keeping it flat lets event handlers patch by id without a
  // group lookup.
  panes: Pane[];
  groups: TerminalGroup[];
  activeGroupId: GroupId;
  maxed: boolean;
  // When true, the strip collapses to just its top bar (tabs + toolbar)
  collapsed: boolean;
  // User-set strip height in pixels, or null to fall back to the
  // viewport-relative default (40vh). Lets a user drag the strip larger
  // when they're running TUIs that need more rows than the default cap.
  stripHeight: number | null;

  // Append a pane + auto-focus it inside the active group. Promotes that
  // group's tabs → grid when its pane count crosses 2.
  addPane: (pane: Pane) => void;
  closePane: (id: PaneId) => void;
  closeAll: () => void;
  setLayout: (l: LayoutMode) => void;
  setMaxed: (b: boolean) => void;
  setCollapsed: (b: boolean) => void;
  setStripHeight: (height: number | null) => void;
  setActive: (id: PaneId | null) => void;
  patchPane: (id: PaneId, patch: Partial<Pane>) => void;
  patchPaneStatus: (id: PaneId, status: PaneStatus) => void;
  // Swap a pane's id (e.g. after a rerun spawned a fresh PTY). Updates
  // both the flat panes list and any group references.
  replacePaneId: (oldId: PaneId, newId: PaneId, patch?: Partial<Pane>) => void;
  // Reorder panes inside whichever group owns `fromId`.
  movePane: (fromId: PaneId, toId: PaneId) => void;

  // Group operations.
  addGroup: (name?: string) => GroupId;
  removeGroup: (id: GroupId) => void;
  setActiveGroup: (id: GroupId) => void;
  renameGroup: (id: GroupId, name: string) => void;
  // Detach a pane from its current group and attach it to `targetGroupId`.
  // The target group becomes active so the pane stays in view. No-op if
  // the pane is already in the target group.
  movePaneToGroup: (paneId: PaneId, targetGroupId: GroupId) => void;

  // Replace the active group's pane set - used when restoring a saved
  // layout on project switch (single-group shape today).
  restore: (
    next: { panes: Pane[]; layout: LayoutMode; activePaneId: PaneId | null },
  ) => void;
}

const DEFAULT_GROUP_ID = "group-1";

function makeInitialGroup(): TerminalGroup {
  return {
    id: DEFAULT_GROUP_ID,
    name: "Group 1",
    paneIds: [],
    layout: "tabs",
    activePaneId: null,
  };
}

function makeGroupId(): GroupId {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `g_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

export const useTerminalStore = create<TerminalState>((set) => ({
  panes: [],
  groups: [makeInitialGroup()],
  activeGroupId: DEFAULT_GROUP_ID,
  maxed: false,
  collapsed: false,
  stripHeight: null,

  addPane: (pane) =>
    set((s) => {
      // Avoid accidental duplicates (e.g. a race where the backend returns
      // before the optimistic add lands).
      if (s.panes.some((p) => p.id === pane.id)) {
        return {
          groups: s.groups.map((g) =>
            g.id === s.activeGroupId ? { ...g, activePaneId: pane.id } : g,
          ),
          collapsed: false,
        };
      }
      const nextPanes = [...s.panes, pane];
      const groups = s.groups.map((g) => {
        if (g.id !== s.activeGroupId) return g;
        const paneIds = [...g.paneIds, pane.id];
        const promoteToGrid = g.layout === "tabs" && paneIds.length >= 2;
        return {
          ...g,
          paneIds,
          activePaneId: pane.id,
          layout: promoteToGrid ? ("grid" as LayoutMode) : g.layout,
        };
      });
      return {
        panes: nextPanes,
        groups,
        // Auto-expand when the user opens a new pane. They just asked for
        // a fresh terminal.
        collapsed: false,
      };
    }),

  closePane: (id) =>
    set((s) => {
      const nextPanes = s.panes.filter((p) => p.id !== id);
      const groups = s.groups.map((g) => {
        if (!g.paneIds.includes(id)) return g;
        const paneIds = g.paneIds.filter((pid) => pid !== id);
        let activePaneId: PaneId | null = g.activePaneId;
        if (activePaneId === id) {
          activePaneId = paneIds.length ? paneIds[paneIds.length - 1] : null;
        }
        return { ...g, paneIds, activePaneId };
      });
      return {
        panes: nextPanes,
        groups,
        // Leaving max/collapsed when the very last pane closes keeps the
        // sidebar/inspector visible and the strip auto-hides.
        maxed: nextPanes.length === 0 ? false : s.maxed,
        collapsed: nextPanes.length === 0 ? false : s.collapsed,
      };
    }),

  closeAll: () =>
    set(() => ({
      panes: [],
      groups: [makeInitialGroup()],
      activeGroupId: DEFAULT_GROUP_ID,
      maxed: false,
      collapsed: false,
    })),

  setLayout: (layout) =>
    set((s) => ({
      groups: s.groups.map((g) =>
        g.id === s.activeGroupId ? { ...g, layout } : g,
      ),
    })),
  setMaxed: (maxed) =>
    // Entering maxed cancels collapse; the two states are mutually
    set(maxed ? { maxed: true, collapsed: false } : { maxed: false }),
  setCollapsed: (collapsed) =>
    set(collapsed ? { collapsed: true, maxed: false } : { collapsed: false }),
  setStripHeight: (stripHeight) => set({ stripHeight }),
  setActive: (paneId) =>
    set((s) => {
      if (paneId == null) {
        return {
          groups: s.groups.map((g) =>
            g.id === s.activeGroupId ? { ...g, activePaneId: null } : g,
          ),
        };
      }
      // Clicking a pane (e.g. via context menu, palette, etc.) also pulls
      // its owning group into view.
      const owner = s.groups.find((g) => g.paneIds.includes(paneId));
      if (!owner) return {};
      return {
        activeGroupId: owner.id,
        groups: s.groups.map((g) =>
          g.id === owner.id ? { ...g, activePaneId: paneId } : g,
        ),
      };
    }),

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
      groups: s.groups.map((g) => {
        if (!g.paneIds.includes(oldId) && g.activePaneId !== oldId) return g;
        return {
          ...g,
          paneIds: g.paneIds.map((pid) => (pid === oldId ? newId : pid)),
          activePaneId: g.activePaneId === oldId ? newId : g.activePaneId,
        };
      }),
    })),

  movePane: (fromId, toId) =>
    set((s) => {
      const owner = s.groups.find((g) => g.paneIds.includes(fromId));
      if (!owner || !owner.paneIds.includes(toId)) return {};
      const fromIdx = owner.paneIds.indexOf(fromId);
      const toIdx = owner.paneIds.indexOf(toId);
      if (fromIdx < 0 || toIdx < 0 || fromIdx === toIdx) return {};
      const nextIds = owner.paneIds.slice();
      const [moved] = nextIds.splice(fromIdx, 1);
      nextIds.splice(toIdx, 0, moved);
      return {
        groups: s.groups.map((g) =>
          g.id === owner.id ? { ...g, paneIds: nextIds } : g,
        ),
      };
    }),

  addGroup: (name) => {
    const id = makeGroupId();
    set((s) => {
      const finalName = (name && name.trim()) || `Group ${s.groups.length + 1}`;
      const group: TerminalGroup = {
        id,
        name: finalName,
        paneIds: [],
        layout: "tabs",
        activePaneId: null,
      };
      return {
        groups: [...s.groups, group],
        activeGroupId: id,
        collapsed: false,
      };
    });
    return id;
  },

  removeGroup: (id) =>
    set((s) => {
      const target = s.groups.find((g) => g.id === id);
      if (!target) return {};
      const removed = new Set(target.paneIds);
      const nextPanes = s.panes.filter((p) => !removed.has(p.id));
      let groups = s.groups.filter((g) => g.id !== id);
      // Always keep at least one group around so addPane has a home.
      if (groups.length === 0) {
        groups = [makeInitialGroup()];
      }
      const activeGroupId =
        s.activeGroupId === id ? groups[0].id : s.activeGroupId;
      return {
        panes: nextPanes,
        groups,
        activeGroupId,
        maxed: nextPanes.length === 0 ? false : s.maxed,
        collapsed: nextPanes.length === 0 ? false : s.collapsed,
      };
    }),

  setActiveGroup: (id) =>
    set((s) =>
      s.groups.some((g) => g.id === id) ? { activeGroupId: id } : {},
    ),

  renameGroup: (id, name) =>
    set((s) => ({
      groups: s.groups.map((g) =>
        g.id === id ? { ...g, name: name.trim() || g.name } : g,
      ),
    })),

  movePaneToGroup: (paneId, targetGroupId) =>
    set((s) => {
      const target = s.groups.find((g) => g.id === targetGroupId);
      if (!target) return {};
      // Already in the target group — make sure it's the focused pane and
      // surface the group, but otherwise no shape change.
      if (target.paneIds.includes(paneId)) {
        return {
          activeGroupId: targetGroupId,
          groups: s.groups.map((g) =>
            g.id === targetGroupId ? { ...g, activePaneId: paneId } : g,
          ),
        };
      }
      let owner: TerminalGroup | undefined;
      const detached = s.groups.map((g) => {
        if (!g.paneIds.includes(paneId)) return g;
        owner = g;
        const paneIds = g.paneIds.filter((pid) => pid !== paneId);
        let activePaneId: PaneId | null = g.activePaneId;
        if (activePaneId === paneId) {
          activePaneId = paneIds.length ? paneIds[paneIds.length - 1] : null;
        }
        return { ...g, paneIds, activePaneId };
      });
      if (!owner) return {};
      const groups = detached.map((g) => {
        if (g.id !== targetGroupId) return g;
        const paneIds = [...g.paneIds, paneId];
        const promoteToGrid = g.layout === "tabs" && paneIds.length >= 2;
        return {
          ...g,
          paneIds,
          activePaneId: paneId,
          layout: promoteToGrid ? ("grid" as LayoutMode) : g.layout,
        };
      });
      return {
        groups,
        activeGroupId: targetGroupId,
      };
    }),

  restore: (next) =>
    set((s) => ({
      panes: next.panes,
      groups: s.groups.map((g) =>
        g.id === s.activeGroupId
          ? {
              ...g,
              paneIds: next.panes.map((p) => p.id),
              layout: next.layout,
              activePaneId: next.activePaneId,
            }
          : g,
      ),
      maxed: false,
    })),
}));

// Derived helper: returns the active group, falling back to the first one
// in case `activeGroupId` got out of sync with `groups`.
export function getActiveGroup(state: {
  groups: TerminalGroup[];
  activeGroupId: GroupId;
}): TerminalGroup {
  return (
    state.groups.find((g) => g.id === state.activeGroupId) ?? state.groups[0]
  );
}

// Persisted layout shape exchanged with `pane_layout_get` / `pane_layout_save`.
export interface PaneLayout {
  mode: LayoutMode;
  panes: Pane[];
  activePaneId: PaneId | null;
}

/** Helper: current store value → persisted shape. Snapshots the active
 *  group only — the persistence schema predates groups and the restore
 *  hook is currently a no-op. */
export function snapshotLayout(): PaneLayout {
  const s = useTerminalStore.getState();
  const g = getActiveGroup(s);
  const ids = new Set(g.paneIds);
  // Preserve group order, not flat-store insertion order.
  const ordered = g.paneIds
    .map((id) => s.panes.find((p) => p.id === id))
    .filter((p): p is Pane => p != null);
  // Defensive: include any pane that's in the flat list but not yet
  // referenced (shouldn't happen, but avoids data loss in a save race).
  const orphans = s.panes.filter((p) => !ids.has(p.id));
  return {
    mode: g.layout,
    panes: [...ordered, ...orphans],
    activePaneId: g.activePaneId,
  };
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
