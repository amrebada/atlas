import { useEffect, useRef } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

import { useUiStore } from "../../state/store";

/** Called once on app launch. If a newer version exists at the
 *  configured `endpoints` URL, downloads + verifies + installs it,
 *  then relaunches. Failures are non-fatal — the user keeps using the
 *  current build and we try again next launch. */
export function useAutoUpdate() {
  const ranOnce = useRef(false);
  const pushToast = useUiStore((s) => s.pushToast);

  useEffect(() => {
    if (ranOnce.current) return;
    ranOnce.current = true;

    void (async () => {
      let update: Update | null = null;
      try {
        update = await check();
      } catch (err) {
        // No network or manifest unreachable. Silent — auto-update is
        // best-effort, not a feature the user is waiting on.
        // eslint-disable-next-line no-console
        console.info("[updater] check failed:", err);
        return;
      }
      if (!update) return;

      pushToast("info", `Atlas ${update.version} downloading…`, 4_000);
      try {
        await update.downloadAndInstall();
      } catch (err) {
        pushToast(
          "error",
          `Update to ${update.version} failed: ${String(err)}`,
        );
        return;
      }

      pushToast(
        "success",
        `Atlas ${update.version} ready — restarting…`,
        2_500,
      );
      // Give the toast a beat, then relaunch.
      setTimeout(() => {
        void relaunch();
      }, 1_500);
    })();
  }, [pushToast]);
}
