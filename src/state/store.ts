import { create } from "zustand";
import type { Collection as DomainCollection, WatchRoot } from "../types";

// Atlas - root UI store.

export type Theme = "dark" | "light" | "system";
export type Density = "comfortable" | "dense" | "compact";
export type Font = "plex" | "inter" | "grotesk" | "system";
export type ViewMode = "list" | "grid";
export type SortKey = "recent" | "name" | "size" | "branch";
export type Collection = "all" | "pinned" | "archive" | string;

/** Inspector tab ids - stay aligned with `Inspector/TabStrip.tsx`. */
export type InspectorTab =
  | "overview"
  | "files"
  | "sessions"
  | "scripts"
  | "todos"
  | "notes"
  | "disk";

export type ToastKind = "info" | "success" | "warn" | "error";
export interface Toast {
  id: string;
  kind: ToastKind;
  message: string;
  /** Epoch ms at which the toast should auto-dismiss. */
  expiresAt: number;
}

// Live progress for an in-flight `watchers_add` operation. Keyed by the
export type DiscoveryPhase = "walking" | "git-status" | "done";
export interface DiscoveryProgress {
  root: string;
  phase: DiscoveryPhase;
  current: string | null;
  found: number;
  total: number | null;
  /** Wall-clock ms this root's scan started, so we can derive "time elapsed". */
  startedAt: number;
}

export interface UiState {
  /** Core appearance. */
  theme: Theme;
  terminalTheme: Theme;
  density: Density;
  font: Font;

  /** Layout / view. */
  viewMode: ViewMode;
  sidebarWidth: number;

  /** Empty-state toggle (matches prototype tweak). */
  onboarding: boolean;

  /** Selection + filters. */
  selectedProjectId: string | null;
  collection: Collection;
  sort: SortKey;
  // Active tag filter. When non-null the project list is further narrowed
  selectedTag: string | null;

  /** Iter 3 - currently focused inspector tab. */
  activeInspectorTab: InspectorTab;

  /** Iter 2 - secondary caches fed by TanStack Query + live events. */
  collections: DomainCollection[];
  tags: string[];
  watchRoots: WatchRoot[];

  /** Iter 2 - toast message queue. */
  toasts: Toast[];

  /** Iter 2.1 - live discovery progress keyed by watch-root path. */
  discovery: Record<string, DiscoveryProgress>;

  // -  currently-open note overlay. `null` means the workspace is
  openNote: { projectId: string; noteId: string } | null;

  // -  overlay visibility slices. Each overlay is mounted once at
  paletteOpen: boolean;
  newProjectOpen: null | { tab: "new" | "clone" | "import" };
  settingsOpen: null | { section: SettingsSection };
  contextMenu: null | { x: number; y: number; projectId: string };

  // -  when not null, the project row with this id renders an inline
  renamingProjectId: string | null;

  // Multi-select mode for bulk project actions. Off by default - no
  multiSelect: { active: boolean; ids: string[] };

  /** Setters. Plain field setters kept explicit for call-site clarity. */
  setTheme: (t: Theme) => void;
  setTerminalTheme: (t: Theme) => void;
  setDensity: (d: Density) => void;
  setFont: (f: Font) => void;
  setViewMode: (v: ViewMode) => void;
  setSidebarWidth: (w: number) => void;
  setOnboarding: (o: boolean) => void;
  setSelectedProjectId: (id: string | null) => void;
  setCollection: (c: Collection) => void;
  setSort: (s: SortKey) => void;
  setSelectedTag: (t: string | null) => void;
  setActiveInspectorTab: (t: InspectorTab) => void;
  setCollections: (c: DomainCollection[]) => void;
  setTags: (t: string[]) => void;
  setWatchRoots: (r: WatchRoot[]) => void;
  pushToast: (kind: ToastKind, message: string, ttlMs?: number) => void;
  dismissToast: (id: string) => void;
  updateDiscovery: (p: Omit<DiscoveryProgress, "startedAt">) => void;
  clearDiscovery: (root: string) => void;
  setOpenNote: (v: { projectId: string; noteId: string } | null) => void;

  /** Overlay setters. */
  setPaletteOpen: (open: boolean) => void;
  openNewProject: (tab?: NewProjectTab) => void;
  closeNewProject: () => void;
  openSettings: (section?: SettingsSection) => void;
  closeSettings: () => void;
  openContextMenu: (m: { x: number; y: number; projectId: string }) => void;
  closeContextMenu: () => void;
  setRenamingProjectId: (id: string | null) => void;

  // Enter multi-select with `ids` pre-seeded (usually the row the user
  startMultiSelect: (ids: string[]) => void;
  /** Flip membership for one id. */
  toggleMultiSelect: (id: string) => void;
  /** Replace the whole selection set (used by "Select all" later). */
  setMultiSelect: (ids: string[]) => void;
  /** Exit multi-select mode and drop the set. */
  clearMultiSelect: () => void;
}

/** Iter 5 - overlay enums hoisted for cross-file reuse. */
export type NewProjectTab = "new" | "clone" | "import";
export type SettingsSection =
  | "general"
  | "editors"
  | "git"
  | "watchers"
  | "templates"
  | "shortcuts"
  | "advanced"
  | "about";

export const useUiStore = create<UiState>((set) => ({
  theme: "dark",
  terminalTheme: "system",
  density: "dense",
  font: "plex",
  viewMode: "list",
  sidebarWidth: 220,
  onboarding: false,
  selectedProjectId: null,
  collection: "all",
  sort: "recent",
  selectedTag: null,
  activeInspectorTab: "overview",

  collections: [],
  tags: [],
  watchRoots: [],
  toasts: [],
  discovery: {},
  openNote: null,

  paletteOpen: false,
  newProjectOpen: null,
  settingsOpen: null,
  contextMenu: null,
  renamingProjectId: null,
  multiSelect: { active: false, ids: [] },

  setTheme: (theme) => set({ theme }),
  setTerminalTheme: (terminalTheme) => set({ terminalTheme }),
  setDensity: (density) => set({ density }),
  setFont: (font) => set({ font }),
  setViewMode: (viewMode) => set({ viewMode }),
  setSidebarWidth: (sidebarWidth) => set({ sidebarWidth }),
  setOnboarding: (onboarding) => set({ onboarding }),
  setSelectedProjectId: (selectedProjectId) => set({ selectedProjectId }),
  setCollection: (collection) => set({ collection }),
  setSort: (sort) => set({ sort }),
  setSelectedTag: (selectedTag) => set({ selectedTag }),
  setActiveInspectorTab: (activeInspectorTab) => set({ activeInspectorTab }),
  setCollections: (collections) => set({ collections }),
  setTags: (tags) => set({ tags }),
  setWatchRoots: (watchRoots) => set({ watchRoots }),

  pushToast: (kind, message, ttlMs = 2200) => {
    const id = `t_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
    const expiresAt = Date.now() + ttlMs;
    set((s) => ({ toasts: [...s.toasts, { id, kind, message, expiresAt }] }));
  },
  dismissToast: (id) =>
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),

  updateDiscovery: (p) =>
    set((s) => {
      const prev = s.discovery[p.root];
      const startedAt = prev?.startedAt ?? Date.now();
      return {
        discovery: { ...s.discovery, [p.root]: { ...p, startedAt } },
      };
    }),
  clearDiscovery: (root) =>
    set((s) => {
      const next = { ...s.discovery };
      delete next[root];
      return { discovery: next };
    }),

  setOpenNote: (openNote) => set({ openNote }),

  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
  openNewProject: (tab = "new") => set({ newProjectOpen: { tab } }),
  closeNewProject: () => set({ newProjectOpen: null }),
  openSettings: (section = "general") => set({ settingsOpen: { section } }),
  closeSettings: () => set({ settingsOpen: null }),
  openContextMenu: (m) => set({ contextMenu: m }),
  closeContextMenu: () => set({ contextMenu: null }),
  setRenamingProjectId: (renamingProjectId) => set({ renamingProjectId }),

  startMultiSelect: (ids) =>
    set((s) => {
      if (s.multiSelect.active) {
        // Merge without duplicates; preserves existing selection when the
        const merged = Array.from(new Set([...s.multiSelect.ids, ...ids]));
        return { multiSelect: { active: true, ids: merged } };
      }
      return { multiSelect: { active: true, ids: Array.from(new Set(ids)) } };
    }),
  toggleMultiSelect: (id) =>
    set((s) => {
      if (!s.multiSelect.active) return s;
      const set_ = new Set(s.multiSelect.ids);
      if (set_.has(id)) set_.delete(id);
      else set_.add(id);
      return { multiSelect: { active: true, ids: Array.from(set_) } };
    }),
  setMultiSelect: (ids) =>
    set({ multiSelect: { active: true, ids: Array.from(new Set(ids)) } }),
  clearMultiSelect: () => set({ multiSelect: { active: false, ids: [] } }),
}));
