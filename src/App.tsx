import { useEffect, useMemo } from "react";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { TitleBar } from "./components/TitleBar";
import { Sidebar } from "./components/Sidebar";
import { ProjectList, CenterHeader } from "./components/ProjectList";
import { ProjectGrid } from "./components/ProjectGrid";
import { Inspector } from "./components/Inspector";
import { OnboardingEmpty } from "./components/OnboardingEmpty";
import { ToastHost } from "./components/ToastHost";
import { NoteEditorOverlay } from "./features/notes/NoteEditor";
import { CommandPalette } from "./features/palette/CommandPalette";
import { NewProjectModal } from "./features/new-project/NewProjectModal";
import { SettingsPanel } from "./features/settings/SettingsPanel";
import { ContextMenu } from "./features/context-menu/ContextMenu";
import { BulkActionBar } from "./features/multi-select/BulkActionBar";
import { CheatSheetOverlay } from "./features/help/CheatSheetOverlay";
import { TerminalStrip } from "./features/terminal/TerminalStrip";
import { useTerminalStore } from "./features/terminal/layout";
import { getSettings, listProjects, listWatchers } from "./ipc";
import type { Settings } from "./types";
import { useUiStore } from "./state/store";
import { useProjectEvents } from "./hooks/useProjectEvents";
import { useGlobalShortcuts } from "./hooks/useGlobalShortcuts";
import { useTerminalProjectSync } from "./hooks/useTerminalProjectSync";
import type { Project, WatchRoot } from "./types";

// Atlas - root component.

// One QueryClient per app. Default stale time picked to keep the prototype
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      refetchOnWindowFocus: false,
    },
  },
});

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <AppInner />
    </QueryClientProvider>
  );
}

function AppInner() {
  const theme = useUiStore((s) => s.theme);
  const density = useUiStore((s) => s.density);
  const font = useUiStore((s) => s.font);
  const sidebarWidth = useUiStore((s) => s.sidebarWidth);
  const viewMode = useUiStore((s) => s.viewMode);
  const collection = useUiStore((s) => s.collection);
  const selectedTag = useUiStore((s) => s.selectedTag);
  const selectedProjectId = useUiStore((s) => s.selectedProjectId);
  const openNote = useUiStore((s) => s.openNote);
  const setOpenNote = useUiStore((s) => s.setOpenNote);
  const setPaletteOpen = useUiStore((s) => s.setPaletteOpen);
  const openNewProject = useUiStore((s) => s.openNewProject);
  const openSettings = useUiStore((s) => s.openSettings);
  const openContextMenu = useUiStore((s) => s.openContextMenu);

  // Mount the Rust→React event bridge once at the top of the tree. Every
  useProjectEvents();

  // Global keyboard shortcuts (⌘K, ⌘N, ⌘,, ⌘E, Esc). Mounted ONCE here so
  useGlobalShortcuts();

  // Terminal strip ↔ project sync. Lives at the App level (not inside
  useTerminalProjectSync(selectedProjectId);

  // Hydrate the Zustand UI store from persisted settings. The store is
  const setTheme = useUiStore((s) => s.setTheme);
  const { data: persistedSettings } = useQuery<Settings>({
    queryKey: ["settings"],
    queryFn: getSettings,
    retry: false,
  });
  useEffect(() => {
    if (persistedSettings?.general.theme) {
      setTheme(persistedSettings.general.theme);
    }
  }, [persistedSettings?.general.theme, setTheme]);

  // Apply data attributes + sidebar width CSS var on <html>. Runs on every
  useEffect(() => {
    const root = document.documentElement;
    const mql = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const effective =
        theme === "system" ? (mql.matches ? "dark" : "light") : theme;
      root.dataset.theme = effective;
    };
    apply();
    root.dataset.density = density;
    root.dataset.font = font;
    root.style.setProperty("--sidebar-w", `${sidebarWidth}px`);
    if (theme === "system") {
      mql.addEventListener("change", apply);
      return () => mql.removeEventListener("change", apply);
    }
    return undefined;
  }, [theme, density, font, sidebarWidth]);

  // TanStack Query fetches the real project list via the Rust `list` command.
  const { data: projects = [] } = useQuery<Project[]>({
    queryKey: ["projects"],
    queryFn: listProjects,
  });

  // `retry: false` so a not-yet-ready Rust side doesn't fill the console;
  const { data: watchers = [] } = useQuery<WatchRoot[]>({
    queryKey: ["watchRoots"],
    queryFn: listWatchers,
    retry: false,
  });

  const filtered = useMemo(
    () => filterProjects(projects, collection, selectedTag),
    [projects, collection, selectedTag],
  );

  const selectedProject = useMemo(
    () => projects.find((p) => p.id === selectedProjectId) ?? null,
    [projects, selectedProjectId],
  );

  // effectively empty: zero projects AND zero configured watch roots. As
  const showOnboarding = projects.length === 0 && watchers.length === 0;

  // Resolve a user-friendly label for the current collection. Custom
  const collections = useUiStore((s) => s.collections);
  const collectionLabel =
    collection === "all"
      ? "All Projects"
      : collection === "pinned"
        ? "Pinned"
        : collection === "archive"
          ? "Archive"
          : collections.find((c) => c.id === collection)?.label ?? "Collection";

  // Pass 3 - the persisted `git.showAuthor` toggle decides whether the
  const showAuthor = persistedSettings?.git.showAuthor ?? false;

  // `maxed` is true, the strip paints itself as a fixed overlay (inside the
  const termPaneCount = useTerminalStore((s) => s.panes.length);
  const termMaxed = useTerminalStore((s) => s.maxed);
  const showStrip = termPaneCount > 0 || termMaxed;
  const stripInGrid = showStrip && !termMaxed;

  return (
    <div className="mac-window">
      <div
        className="grid h-full w-full bg-bg text-text overflow-hidden"
        style={{
          gridTemplateColumns: `var(--sidebar-w, ${sidebarWidth}px) 1fr 340px`,
          gridTemplateRows: stripInGrid
            ? "36px 1fr auto"
            : termMaxed
              ? "36px 1fr 0"
              : "36px 1fr",
        }}
      >
        <TitleBar
          project={selectedProject}
          onOpenPalette={() => setPaletteOpen(true)}
          onNew={() => openNewProject("new")}
          onSettings={() => openSettings("general")}
        />

        <Sidebar projects={projects} />

        <div className="flex flex-col border-r border-line overflow-hidden min-w-0">
          {showOnboarding ? (
            <OnboardingEmpty />
          ) : (
            <>
              <CenterHeader
                projectCount={filtered.length}
                collectionLabel={collectionLabel}
              />
              {viewMode === "list" ? (
                <ProjectList
                  projects={filtered}
                  showAuthor={showAuthor}
                  onContextMenu={(e, p) =>
                    openContextMenu({
                      x: e.clientX,
                      y: e.clientY,
                      projectId: p.id,
                    })
                  }
                />
              ) : (
                <ProjectGrid
                  projects={filtered}
                  showAuthor={showAuthor}
                  onContextMenu={(e, p) =>
                    openContextMenu({
                      x: e.clientX,
                      y: e.clientY,
                      projectId: p.id,
                    })
                  }
                />
              )}
            </>
          )}
        </div>

        <Inspector project={selectedProject} />

        {showStrip && (
          <div
            className="overflow-hidden min-w-0"
            style={{ gridColumn: "1 / -1" }}
          >
            <TerminalStrip
              projectId={selectedProject?.id ?? null}
              projectLabel={selectedProject?.name ?? null}
              projectPath={selectedProject?.path ?? null}
              branch={selectedProject?.branch}
            />
          </div>
        )}
      </div>

      {/* Global overlay for transient toasts (forwarded from `toast` events
          and placeholder-button clicks). */}
      <ToastHost />

      {/* Iter 5 overlays. All mounted unconditionally — each one renders
          `null` when its Zustand slice is closed, so z-index stacking stays
          deterministic without bespoke portal containers. */}
      <CommandPalette />
      <NewProjectModal />
      <SettingsPanel />
      <ContextMenu projects={projects} />
      <BulkActionBar projects={projects} />
      <CheatSheetOverlay />

      {/* Full-bleed Tiptap note editor. Mounted here so it can
          cover sidebar + inspector via a Portal while staying under the
          native title-bar drag region. */}
      {openNote &&
        (() => {
          const noteProject = projects.find((p) => p.id === openNote.projectId);
          if (!noteProject) return null;
          return (
            <NoteEditorOverlay
              project={noteProject}
              noteId={openNote.noteId}
              onClose={() => setOpenNote(null)}
            />
          );
        })()}
    </div>
  );
}

function filterProjects(
  projects: Project[],
  collection: string,
  tag: string | null,
): Project[] {
  let out: Project[];
  if (collection === "all") out = projects.filter((p) => !p.archived);
  else if (collection === "pinned")
    out = projects.filter((p) => p.pinned && !p.archived);
  else if (collection === "archive") out = projects.filter((p) => p.archived);
  else
    out = projects.filter(
      (p) => !p.archived && p.collectionIds.includes(collection),
    );
  if (tag) out = out.filter((p) => p.tags.includes(tag));
  return out;
}
