import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { useUiStore } from "../state/store";
import type { Project } from "../types";

// Atlas - Rust → React event bridge.

interface ProjectUpdatedPayload {
  id: string;
  patch: Partial<Project>;
}

interface ProjectDiscoveredPayload {
  project: Project;
}

interface ProjectRemovedPayload {
  id: string;
}

interface GitStatusPayload {
  id: string;
  dirty?: number;
  ahead?: number;
  behind?: number;
  branch?: string;
}

interface ToastPayload {
  kind: "info" | "success" | "warn" | "error";
  message: string;
}

interface DiscoveryProgressPayload {
  root: string;
  phase: "walking" | "git-status" | "done";
  current: string | null;
  found: number;
  total: number | null;
}

export function useProjectEvents(): void {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);
  const updateDiscovery = useUiStore((s) => s.updateDiscovery);
  const clearDiscovery = useUiStore((s) => s.clearDiscovery);

  useEffect(() => {
    const disposers: Promise<UnlistenFn>[] = [];

    // Merge a partial Project into the cached array by id.
    const patchProject = (id: string, patch: Partial<Project>) => {
      queryClient.setQueryData<Project[]>(["projects"], (old) =>
        old ? old.map((p) => (p.id === id ? { ...p, ...patch } : p)) : old,
      );
    };

    disposers.push(
      listen<ProjectUpdatedPayload>("project:updated", (e) => {
        patchProject(e.payload.id, e.payload.patch);
      }),
    );

    disposers.push(
      listen<ProjectDiscoveredPayload>("project:discovered", (e) => {
        // Append (avoid duplicates) then invalidate so any server-side
        queryClient.setQueryData<Project[]>(["projects"], (old) => {
          if (!old) return [e.payload.project];
          if (old.some((p) => p.id === e.payload.project.id)) return old;
          return [...old, e.payload.project];
        });
        queryClient.invalidateQueries({ queryKey: ["projects"] });
      }),
    );

    disposers.push(
      listen<ProjectRemovedPayload>("project:removed", (e) => {
        queryClient.setQueryData<Project[]>(["projects"], (old) =>
          old ? old.filter((p) => p.id !== e.payload.id) : old,
        );
      }),
    );

    disposers.push(
      listen<GitStatusPayload>("git:status", (e) => {
        const { id, ...rest } = e.payload;
        patchProject(id, rest as Partial<Project>);
      }),
    );

    disposers.push(
      listen<ToastPayload>("toast", (e) => {
        pushToast(e.payload.kind, e.payload.message);
      }),
    );

    disposers.push(
      listen<DiscoveryProgressPayload>("discovery:progress", (e) => {
        // Dev-time trace so it's easy to see the event stream in Tauri
        console.debug("[atlas] discovery:progress", e.payload);
        if (e.payload.phase === "done") {
          updateDiscovery(e.payload);
          setTimeout(() => clearDiscovery(e.payload.root), 600);
        } else {
          updateDiscovery(e.payload);
        }
      }),
    );

    return () => {
      // Unlisten from every subscription; ignore rejections from racing
      disposers.forEach((p) => {
        p.then((fn) => fn()).catch(() => {});
      });
    };
  }, [queryClient, pushToast, updateDiscovery, clearDiscovery]);
}
