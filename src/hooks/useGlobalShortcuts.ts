import { useEffect } from "react";
import { openInEditor } from "../ipc";
import { useUiStore } from "../state/store";

// Atlas - global keyboard shortcuts.
export function useGlobalShortcuts(): void {
  const paletteOpen = useUiStore((s) => s.paletteOpen);
  const setPaletteOpen = useUiStore((s) => s.setPaletteOpen);
  const newProjectOpen = useUiStore((s) => s.newProjectOpen);
  const openNewProject = useUiStore((s) => s.openNewProject);
  const closeNewProject = useUiStore((s) => s.closeNewProject);
  const settingsOpen = useUiStore((s) => s.settingsOpen);
  const openSettings = useUiStore((s) => s.openSettings);
  const closeSettings = useUiStore((s) => s.closeSettings);
  const contextMenu = useUiStore((s) => s.contextMenu);
  const closeContextMenu = useUiStore((s) => s.closeContextMenu);
  const openNote = useUiStore((s) => s.openNote);
  const setOpenNote = useUiStore((s) => s.setOpenNote);
  const selectedProjectId = useUiStore((s) => s.selectedProjectId);
  const pushToast = useUiStore((s) => s.pushToast);
  const multiSelect = useUiStore((s) => s.multiSelect);
  const startMultiSelect = useUiStore((s) => s.startMultiSelect);
  const clearMultiSelect = useUiStore((s) => s.clearMultiSelect);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      const key = e.key.toLowerCase();

      // Esc closes the topmost overlay (palette > modals > context menu).
      if (key === "escape") {
        if (paletteOpen) {
          setPaletteOpen(false);
          e.preventDefault();
          return;
        }
        if (newProjectOpen) {
          closeNewProject();
          e.preventDefault();
          return;
        }
        if (settingsOpen) {
          closeSettings();
          e.preventDefault();
          return;
        }
        if (contextMenu) {
          closeContextMenu();
          e.preventDefault();
          return;
        }
        if (openNote) {
          setOpenNote(null);
          e.preventDefault();
          return;
        }
        if (multiSelect.active) {
          clearMultiSelect();
          e.preventDefault();
          return;
        }
        return;
      }

      // ⇧⌘A - enter multi-select mode, seeding with the currently
      if (mod && e.shiftKey && key === "a") {
        e.preventDefault();
        startMultiSelect(selectedProjectId ? [selectedProjectId] : []);
        return;
      }

      // Shortcuts from here on require a modifier.
      if (!mod) return;

      // Respect text fields: allow Cmd-based shortcuts but bail on letter-only.

      if (key === "k") {
        e.preventDefault();
        setPaletteOpen(!paletteOpen);
        return;
      }
      if (key === "n") {
        e.preventDefault();
        openNewProject(e.shiftKey ? "clone" : "new");
        return;
      }
      if (key === ",") {
        e.preventDefault();
        openSettings("general");
        return;
      }
      if (key === "e") {
        if (!selectedProjectId) {
          // Keep the shortcut silent rather than toasting an error; users
          return;
        }
        e.preventDefault();
        openInEditor(selectedProjectId).catch((err) =>
          pushToast("error", `Open failed: ${String(err)}`),
        );
        return;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    paletteOpen,
    setPaletteOpen,
    newProjectOpen,
    openNewProject,
    closeNewProject,
    settingsOpen,
    openSettings,
    closeSettings,
    contextMenu,
    closeContextMenu,
    openNote,
    setOpenNote,
    selectedProjectId,
    pushToast,
    multiSelect.active,
    startMultiSelect,
    clearMultiSelect,
  ]);
}
