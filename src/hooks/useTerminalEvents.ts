import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useTerminalStore } from "../features/terminal/layout";

// Atlas - per-pane terminal event bridge.

interface TerminalStatusPayload {
  paneId: string;
  status: "idle" | "running" | "active" | "error";
}

interface TerminalExitPayload {
  paneId: string;
  code: number | null;
}

export function useTerminalEvents(): void {
  const patchPaneStatus = useTerminalStore((s) => s.patchPaneStatus);

  useEffect(() => {
    const disposers: Promise<UnlistenFn>[] = [];

    disposers.push(
      listen<TerminalStatusPayload>("terminal:status", (e) => {
        patchPaneStatus(e.payload.paneId, e.payload.status);
      }),
    );

    disposers.push(
      listen<TerminalExitPayload>("terminal:exit", (e) => {
        // Non-zero exit → red dot, zero → idle. `null` (signalled) → error.
        const status =
          e.payload.code === 0
            ? "idle"
            : "error";
        patchPaneStatus(e.payload.paneId, status);
      }),
    );

    return () => {
      disposers.forEach((p) => {
        p.then((fn) => fn()).catch(() => {});
      });
    };
  }, [patchPaneStatus]);
}
