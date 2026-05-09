import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { makePane, useTerminalStore } from "../terminal/layout";
import type { PaneKind } from "../../types";

// Listen for `mcp:pane:opened` events emitted when an MCP tool spawns a new
// terminal/session pane via the embedded server. Without this hook the
// backend `TerminalManager` would have a live PTY that the React UI knows
// nothing about — so the user wouldn't see Claude boot in the strip.

type PaneOpenedPayload = {
  id: string;
  kind: PaneKind;
  cwd: string;
  title: string;
  projectId?: string | null;
  projectLabel?: string | null;
  branch?: string | null;
  scriptId?: string | null;
  sessionId?: string | null;
};

export function useMcpPaneEvents() {
  const addPane = useTerminalStore((s) => s.addPane);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    (async () => {
      unlisten = await listen<PaneOpenedPayload>("mcp:pane:opened", (e) => {
        if (cancelled) return;
        const p = e.payload;
        addPane(
          makePane(p.id, p.kind, p.cwd, p.title, {
            ...(p.branch ? { branch: p.branch } : {}),
            ...(p.projectId ? { projectId: p.projectId } : {}),
            ...(p.projectLabel ? { projectLabel: p.projectLabel } : {}),
            ...(p.scriptId ? { scriptId: p.scriptId } : {}),
            ...(p.sessionId ? { sessionId: p.sessionId } : {}),
          }),
        );
      });
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [addPane]);
}
