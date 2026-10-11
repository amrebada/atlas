import { useMutation, useQueryClient } from "@tanstack/react-query";
import { gitSync, type GitSyncResult } from "../../ipc";
import { useUiStore } from "../../state/store";

// Atlas - shared "sync" mutation: stash (if dirty) → pull --rebase → push →
// stash pop. Used by the Inspector (Overview / Files), the context menu
// and the Home page so every entry point reports the outcome the same way.

export function useGitSync() {
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();

  return useMutation<GitSyncResult, unknown, string>({
    mutationFn: (projectId: string) => gitSync(projectId),
    onSuccess: (res, projectId) => {
      // Failures carry a one-line summary that already names the step;
      // keep them on screen longer so the user can read it.
      pushToast(res.ok ? "success" : "warn", res.summary, res.ok ? 3200 : 7000);
      queryClient.invalidateQueries({ queryKey: ["files", projectId] });
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      queryClient.invalidateQueries({ queryKey: ["home"] });
    },
    onError: (err) => pushToast("error", `Sync failed: ${String(err)}`, 6000),
  });
}
