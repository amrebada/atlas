import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useUiStore } from "../../state/store";
import { fireNativeNotification } from "./nativeNotification";

// Atlas - relay Claude Code `Notification` hook events (permission needed,
// waiting for input, …) as an in-app toast plus a macOS banner. Mounted
// once in App.

export interface ClaudeNotificationPayload {
  id: string;
  projectId: string | null;
  projectName: string | null;
  cwd: string;
  sessionId: string;
  message: string;
  title: string | null;
  notificationType: string | null;
}

export function useClaudeNotifications(): void {
  const pushToast = useUiStore((s) => s.pushToast);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    (async () => {
      try {
        unlisten = await listen<ClaudeNotificationPayload>(
          "claude:notification",
          (e) => {
            if (cancelled) return;
            const p = e.payload;
            const where = p.projectName ?? shortCwd(p.cwd);
            const headline = p.title ?? labelFor(p.notificationType);
            const kind = p.notificationType === "permission_prompt" ? "warn" : "info";
            pushToast(kind, `Claude Code · ${where}: ${p.message || headline}`, 6000);
            void fireNativeNotification(
              `${headline} · ${where}`,
              p.message || "Claude Code needs your attention",
              `claude-${p.sessionId || p.id}`,
            );
          },
        );
      } catch (err) {
        console.warn("[claude-hooks] notification listen failed:", err);
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [pushToast]);
}

function labelFor(type: string | null): string {
  switch (type) {
    case "permission_prompt":
      return "Permission needed";
    case "idle_prompt":
      return "Waiting for you";
    case "agent_needs_input":
      return "Agent needs input";
    case "agent_completed":
      return "Agent finished";
    case "elicitation_dialog":
    case "elicitation_url_dialog":
      return "Input requested";
    default:
      return "Claude Code";
  }
}

export function shortCwd(cwd: string): string {
  const trimmed = cwd.replace(/\/+$/, "");
  const last = trimmed.split("/").pop();
  return last && last.length > 0 ? last : trimmed || "unknown project";
}
