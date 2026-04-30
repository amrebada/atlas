import { useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";

import { plannerSessionStart } from "../../ipc";
import type { PlannerToday, TodayItem } from "../../types";
import { itemTitle } from "./TodayShared";

/** Fires once at app launch:
 *   1. Calls `planner_session_start` so the backend gates "first session
 *      of the local day" and persists `lastSessionDate`.
 *   2. If the backend says it fired, also fires a browser Notification
 *      with the headline. The browser Notification API surfaces a real
 *      OS notification on macOS via the Tauri webview — no extra plugin.
 *   3. Subscribes to the `planner:notification` Rust event so future
 *      backend-triggered notifications (e.g. routine missed) also fire
 *      OS-level toasts. */
export function useTopPriorityNotification() {
  const queryClient = useQueryClient();
  const ranOnce = useRef(false);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    const fireNotification = (today: PlannerToday) => {
      const top = today.topPriority;
      if (!top) return;
      const title = top.kind === "milestone-deadline" ? "Today" : "Top priority";
      const body = subtitleFor(top);
      tryNativeNotification(`${title} — ${itemTitle(top)}`, body);
    };

    const subscribePlannerEvents = async () => {
      try {
        unlisten = await listen<PlannerToday>("planner:notification", (e) => {
          // Backend always sends the freshly-built `PlannerToday` payload.
          fireNotification(e.payload);
          void queryClient.invalidateQueries({ queryKey: ["planner-today"] });
        });
      } catch (err) {
        // Listening failed — non-fatal; we still got the launch fire below.
        // eslint-disable-next-line no-console
        console.warn("[planner] event listen failed:", err);
      }
    };

    const runSessionStart = async () => {
      if (ranOnce.current) return;
      ranOnce.current = true;
      try {
        const result = await plannerSessionStart();
        if (cancelled) return;
        if (result.fired && result.today) {
          fireNotification(result.today);
          void queryClient.invalidateQueries({ queryKey: ["planner-today"] });
        }
      } catch (err) {
        // eslint-disable-next-line no-console
        console.warn("[planner] session_start failed:", err);
      }
    };

    void subscribePlannerEvents();
    void runSessionStart();

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, [queryClient]);
}

function subtitleFor(item: TodayItem): string {
  switch (item.kind) {
    case "todo": {
      const overdue =
        item.daysOverdue > 0 ? `${item.daysOverdue}d overdue` : "due today";
      return `${item.projectName} · ${overdue}`;
    }
    case "milestone-deadline":
      return `${item.projectName} · milestone deadline ${item.deadline}`;
    case "routine-instance":
      return `${item.projectName ?? "global"} · routine ${item.scheduledFor}`;
  }
}

/** Try to fire a browser/OS Notification. Lazy-asks for permission on
 *  first call; silently no-ops on denial. */
async function tryNativeNotification(title: string, body: string) {
  try {
    if (typeof Notification === "undefined") return;
    if (Notification.permission === "denied") return;
    if (Notification.permission !== "granted") {
      const result = await Notification.requestPermission();
      if (result !== "granted") return;
    }
    // eslint-disable-next-line no-new
    new Notification(title, { body });
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn("[planner] notification fire failed:", err);
  }
}
