import { useEffect } from "react";
import { useUiStore } from "../state/store";

// Atlas - bottom-center toast stack.

const KIND_STYLES: Record<string, { bg: string; fg: string; border: string }> = {
  info: { bg: "var(--palette-bg)", fg: "var(--text)", border: "var(--line)" },
  success: {
    bg: "var(--palette-bg)",
    fg: "var(--text)",
    border: "var(--accent)",
  },
  warn: { bg: "var(--palette-bg)", fg: "var(--warn)", border: "var(--warn)" },
  error: {
    bg: "var(--palette-bg)",
    fg: "var(--danger)",
    border: "var(--danger)",
  },
};

export function ToastHost() {
  const toasts = useUiStore((s) => s.toasts);
  const dismissToast = useUiStore((s) => s.dismissToast);

  // Single scheduler ticks every 250ms and reaps expired toasts. Cheap and
  useEffect(() => {
    if (toasts.length === 0) return;
    const iv = window.setInterval(() => {
      const now = Date.now();
      toasts.forEach((t) => {
        if (t.expiresAt <= now) dismissToast(t.id);
      });
    }, 250);
    return () => window.clearInterval(iv);
  }, [toasts, dismissToast]);

  if (toasts.length === 0) return null;

  return (
    <div
      className="fixed inset-x-0 bottom-5 flex flex-col items-center gap-2 pointer-events-none z-50"
      aria-live="polite"
    >
      {toasts.map((t) => {
        const style = KIND_STYLES[t.kind] ?? KIND_STYLES.info;
        return (
          <div
            key={t.id}
            onClick={() => dismissToast(t.id)}
            className="pointer-events-auto cursor-pointer px-3 py-[6px] rounded-[6px] border text-xs font-mono shadow-lg"
            style={{
              background: style.bg,
              color: style.fg,
              borderColor: style.border,
              maxWidth: 480,
            }}
          >
            {t.message}
          </div>
        );
      })}
    </div>
  );
}
