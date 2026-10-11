// Atlas - fire an OS-level notification through the webview's
// Notification API (Tauri surfaces it as a real macOS banner). Lazily asks
// for permission on first use and silently no-ops when denied.

export async function fireNativeNotification(
  title: string,
  body: string,
  tag?: string,
): Promise<void> {
  try {
    if (typeof Notification === "undefined") return;
    if (Notification.permission === "denied") return;
    if (Notification.permission !== "granted") {
      const result = await Notification.requestPermission();
      if (result !== "granted") return;
    }
    // eslint-disable-next-line no-new
    new Notification(title, { body, tag });
  } catch (err) {
    console.warn("[claude-hooks] notification fire failed:", err);
  }
}
