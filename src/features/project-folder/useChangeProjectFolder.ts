import { useCallback, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { setProjectPath } from "../../ipc";
import { useUiStore } from "../../state/store";
import type { Project } from "../../types";

// Atlas - "Change folder…" flow. Opens the native directory picker, re-points
// the project at the chosen folder and swaps the cache entry. Project ids are
// a hash of the path, so the project comes back under a NEW id - callers
// must not hold on to the old one after this resolves.

export function useChangeProjectFolder() {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);
  const [pendingId, setPendingId] = useState<string | null>(null);

  const changeFolder = useCallback(
    async (project: Project): Promise<Project | null> => {
      let picked: string | string[] | null;
      try {
        picked = await openDialog({
          directory: true,
          multiple: false,
          defaultPath: project.path,
          title: `Choose the folder for “${project.name}”`,
        });
      } catch (err) {
        pushToast("error", `Folder picker failed: ${String(err)}`);
        return null;
      }
      if (typeof picked !== "string" || picked === project.path) return null;

      setPendingId(project.id);
      try {
        const next = await setProjectPath(project.id, picked);

        // Swap old → new in the list cache (the backend also emits
        // project:removed / project:discovered, this just makes it instant).
        queryClient.setQueryData<Project[]>(["projects"], (old) => {
          const rest = (old ?? []).filter(
            (p) => p.id !== project.id && p.id !== next.id,
          );
          return [...rest, next];
        });
        const ui = useUiStore.getState();
        if (ui.selectedProjectId === project.id) {
          ui.setSelectedProjectId(next.id);
        }
        queryClient.removeQueries({ queryKey: ["files", project.id] });
        queryClient.invalidateQueries({ queryKey: ["projects"] });
        queryClient.invalidateQueries({ queryKey: ["home"] });

        const short = next.path.replace(/^\/Users\/[^/]+\//, "~/");
        pushToast("success", `${next.name} now points at ${short}`, 3200);
        return next;
      } catch (err) {
        pushToast("error", `Couldn't change folder: ${String(err)}`, 6000);
        return null;
      } finally {
        setPendingId(null);
      }
    },
    [queryClient, pushToast],
  );

  return { changeFolder, pendingId };
}
