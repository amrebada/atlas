import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Icon } from "../../components/Icon";
import { claudePermissionResolve, type PermissionDecision } from "../../ipc";
import { useUiStore } from "../../state/store";
import { fireNativeNotification } from "./nativeNotification";
import { shortCwd } from "./useClaudeNotifications";

// Atlas - Claude Code permission dialog. Listens for
// `claude:permission:request` (relayed from Claude Code's PermissionRequest
// hook), shows the tool + its input, and sends Allow / Deny back through
// the hook bridge. "Ask in terminal" hands the decision back to Claude
// Code's own prompt. Requests queue up; the oldest is shown first.

interface PermissionRequestPayload {
  id: string;
  projectId: string | null;
  projectName: string | null;
  cwd: string;
  sessionId: string;
  toolName: string;
  toolInput: unknown;
  permissionMode: string | null;
  expiresInSecs: number;
}

interface Pending extends PermissionRequestPayload {
  receivedAt: number;
}

export function ClaudePermissionDialog() {
  const [queue, setQueue] = useState<Pending[]>([]);
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const pushToast = useUiStore((s) => s.pushToast);

  useEffect(() => {
    let unlistenRequest: UnlistenFn | undefined;
    let unlistenCancel: UnlistenFn | undefined;
    let cancelled = false;

    (async () => {
      try {
        unlistenRequest = await listen<PermissionRequestPayload>(
          "claude:permission:request",
          (e) => {
            if (cancelled) return;
            const p = e.payload;
            setQueue((prev) =>
              prev.some((r) => r.id === p.id)
                ? prev
                : [...prev, { ...p, receivedAt: Date.now() }],
            );
            if (!document.hasFocus()) {
              void fireNativeNotification(
                `Permission needed · ${p.projectName ?? shortCwd(p.cwd)}`,
                `${p.toolName}: ${summarize(p.toolInput)}`,
                `claude-perm-${p.id}`,
              );
            }
          },
        );
        unlistenCancel = await listen<{ id: string }>(
          "claude:permission:cancelled",
          (e) => {
            if (cancelled) return;
            setQueue((prev) => prev.filter((r) => r.id !== e.payload.id));
          },
        );
      } catch (err) {
        console.warn("[claude-hooks] permission listen failed:", err);
      }
    })();

    return () => {
      cancelled = true;
      unlistenRequest?.();
      unlistenCancel?.();
    };
  }, []);

  // Countdown tick while something is showing.
  useEffect(() => {
    if (queue.length === 0) return;
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, [queue.length]);

  const head = queue[0];

  // Keyboard: ⌘⏎ allow, Esc = ask in terminal.
  useEffect(() => {
    if (!head) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        void decide("allow");
      } else if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        void decide("ask");
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [head?.id]);

  if (!head) return null;

  const decide = async (decision: PermissionDecision) => {
    if (busy) return;
    setBusy(true);
    try {
      await claudePermissionResolve(head.id, decision);
      if (decision !== "ask") {
        pushToast(
          decision === "allow" ? "success" : "warn",
          `${decision === "allow" ? "Allowed" : "Denied"} ${head.toolName} in ${head.projectName ?? shortCwd(head.cwd)}`,
        );
      }
    } catch (err) {
      pushToast("error", `Permission reply failed: ${String(err)}`, 6000);
    } finally {
      setBusy(false);
      setQueue((prev) => prev.filter((r) => r.id !== head.id));
    }
  };

  const secondsLeft = Math.max(
    0,
    head.expiresInSecs - Math.floor((now - head.receivedAt) / 1000),
  );
  const where = head.projectName ?? shortCwd(head.cwd);
  const detail = formatInput(head.toolName, head.toolInput);

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.45)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 1100,
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) e.stopPropagation();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Claude Code permission request"
        style={{
          width: 520,
          maxWidth: "92vw",
          background: "var(--surface)",
          border: "1px solid var(--line)",
          borderRadius: 10,
          padding: 18,
          boxShadow: "0 20px 60px rgba(0,0,0,0.35)",
          fontFamily: "var(--sans)",
          color: "var(--text)",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            fontSize: 11,
            fontWeight: 600,
            letterSpacing: 0.4,
            textTransform: "uppercase",
            color: "var(--text-dim)",
            marginBottom: 6,
          }}
        >
          <Icon name="sparkle" size={12} stroke="var(--accent)" />
          Claude Code wants permission
          <span style={{ marginLeft: "auto", fontFamily: "var(--mono)", fontWeight: 400 }}>
            {secondsLeft}s
          </span>
        </div>
        <div style={{ fontSize: 15, fontWeight: 600, marginBottom: 4 }}>
          {head.toolName}
          <span style={{ color: "var(--text-dim)", fontWeight: 400 }}> in </span>
          {where}
        </div>
        <div
          style={{
            fontSize: 11,
            fontFamily: "var(--mono)",
            color: "var(--text-dimmer)",
            marginBottom: 12,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
          title={head.cwd}
        >
          {head.cwd}
          {head.permissionMode ? ` · mode: ${head.permissionMode}` : ""}
        </div>

        <pre
          style={{
            margin: 0,
            marginBottom: 14,
            fontSize: 12,
            lineHeight: 1.5,
            background: "var(--surface-2)",
            border: "1px solid var(--line-soft)",
            borderRadius: 6,
            padding: "10px 12px",
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
            fontFamily: "var(--mono)",
            maxHeight: 240,
            overflowY: "auto",
          }}
        >
          {detail}
        </pre>

        {queue.length > 1 && (
          <div style={{ fontSize: 11, color: "var(--text-dim)", marginBottom: 10 }}>
            +{queue.length - 1} more request{queue.length === 2 ? "" : "s"} waiting
          </div>
        )}

        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <button
            type="button"
            onClick={() => void decide("ask")}
            disabled={busy}
            title="No decision here — answer in the terminal instead (Esc)"
            style={ghostBtn}
          >
            Ask in terminal
          </button>
          <div style={{ flex: 1 }} />
          <button
            type="button"
            onClick={() => void decide("deny")}
            disabled={busy}
            style={{ ...ghostBtn, color: "var(--danger)", borderColor: "var(--danger)" }}
          >
            Deny
          </button>
          <button
            type="button"
            onClick={() => void decide("allow")}
            disabled={busy}
            title="Allow (⌘⏎)"
            style={{
              ...ghostBtn,
              background: "var(--accent)",
              color: "var(--accent-fg)",
              borderColor: "var(--accent)",
              fontWeight: 600,
            }}
          >
            Allow
          </button>
        </div>
      </div>
    </div>
  );
}

const ghostBtn: React.CSSProperties = {
  padding: "6px 12px",
  borderRadius: 5,
  border: "1px solid var(--line)",
  background: "transparent",
  color: "var(--text)",
  fontSize: 12,
  cursor: "pointer",
};

/** One-line gist for the OS banner. */
function summarize(input: unknown): string {
  const obj = asRecord(input);
  const first =
    (obj.command as string | undefined) ??
    (obj.file_path as string | undefined) ??
    (obj.path as string | undefined) ??
    (obj.pattern as string | undefined) ??
    (obj.url as string | undefined) ??
    (obj.description as string | undefined);
  const text = typeof first === "string" ? first : JSON.stringify(input ?? {});
  return text.length > 120 ? `${text.slice(0, 117)}…` : text;
}

/** Tool-aware rendering of the input block. */
function formatInput(tool: string, input: unknown): string {
  const obj = asRecord(input);
  if (tool === "Bash" && typeof obj.command === "string") {
    return obj.description
      ? `$ ${obj.command}\n\n# ${String(obj.description)}`
      : `$ ${obj.command}`;
  }
  if ((tool === "Edit" || tool === "Write" || tool === "Read") && typeof obj.file_path === "string") {
    const rest = { ...obj };
    delete rest.file_path;
    const body = Object.keys(rest).length > 0 ? `\n\n${pretty(rest)}` : "";
    return `${obj.file_path}${body}`;
  }
  return pretty(input ?? {});
}

function pretty(v: unknown): string {
  try {
    const s = JSON.stringify(v, null, 2);
    return s.length > 4000 ? `${s.slice(0, 3997)}…` : s;
  } catch {
    return String(v);
  }
}

function asRecord(v: unknown): Record<string, unknown> {
  return v && typeof v === "object" && !Array.isArray(v)
    ? (v as Record<string, unknown>)
    : {};
}
