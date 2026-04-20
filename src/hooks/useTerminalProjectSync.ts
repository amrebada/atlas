import { useEffect, useRef } from "react";
import { paneLayoutGet } from "../ipc";
import { useTerminalStore } from "../features/terminal/layout";

// Atlas - terminal-strip ↔ project sync.
export function useTerminalProjectSync(projectId: string | null): void {
  const lastProjectIdRef = useRef<string | null>(null);
  const restore = useTerminalStore((s) => s.restore);

  useEffect(() => {
    if (!projectId) return;
    if (lastProjectIdRef.current === projectId) return;
    lastProjectIdRef.current = projectId;

    // Only hydrate if the strip is currently empty. Non-empty = the user
    const current = useTerminalStore.getState().panes;
    if (current.length > 0) return;

    paneLayoutGet(projectId)
      .then((saved) => {
        if (!saved || !saved.panes?.length) return;
        restore({
          panes: saved.panes ?? [],
          layout: saved.mode ?? "tabs",
          activePaneId: saved.activePaneId ?? saved.panes?.[0]?.id ?? null,
        });
      })
      .catch(() => {
        /* pane_layout_get not yet registered - swallow */
      });
    // `restore` is a referentially stable Zustand setter.
  }, [projectId]);
}
