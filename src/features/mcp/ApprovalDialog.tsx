import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { resolveMcpApproval } from "../../ipc";
import { useUiStore } from "../../state/store";

// MCP approval modal. Listens for `mcp:approval:request` events from the
// Rust MCP server and shows the user the plan summary with Approve / Reject
// buttons. The server emits `mcp:approval:cancelled` when its 120s timeout
// elapses; we close the dialog if the cancelled id matches what we're
// showing.

type PendingRequest = {
  id: string;
  summary: string;
  ttlSeconds: number;
  receivedAt: number;
};

type ApprovalRequestPayload = {
  id: string;
  summary: string;
  ttlSeconds: number;
};

type ApprovalCancelledPayload = { id: string };

export function McpApprovalDialog() {
  const [queue, setQueue] = useState<PendingRequest[]>([]);
  const pushToast = useUiStore((s) => s.pushToast);

  useEffect(() => {
    let unlistenRequest: UnlistenFn | undefined;
    let unlistenCancel: UnlistenFn | undefined;
    let cancelled = false;

    (async () => {
      unlistenRequest = await listen<ApprovalRequestPayload>(
        "mcp:approval:request",
        (e) => {
          if (cancelled) return;
          setQueue((prev) => [
            ...prev,
            { ...e.payload, receivedAt: Date.now() },
          ]);
        },
      );
      unlistenCancel = await listen<ApprovalCancelledPayload>(
        "mcp:approval:cancelled",
        (e) => {
          if (cancelled) return;
          setQueue((prev) => prev.filter((r) => r.id !== e.payload.id));
        },
      );
    })();

    return () => {
      cancelled = true;
      unlistenRequest?.();
      unlistenCancel?.();
    };
  }, []);

  const head = queue[0];

  if (!head) return null;

  const decide = async (approve: boolean) => {
    try {
      await resolveMcpApproval(head.id, approve);
    } catch (err) {
      pushToast("error", `MCP approval: ${String(err)}`);
    }
    // Drop the head regardless — server already removed the pending entry.
    setQueue((prev) => prev.slice(1));
  };

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
        // Click outside = no decision. Don't auto-reject — let the user
        // explicitly choose, or let the server time out at 120s.
        if (e.target === e.currentTarget) e.stopPropagation();
      }}
    >
      <div
        style={{
          width: 480,
          maxWidth: "90vw",
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
            fontSize: 11,
            fontWeight: 600,
            letterSpacing: 0.4,
            textTransform: "uppercase",
            color: "var(--text-dim)",
            marginBottom: 6,
          }}
        >
          MCP approval requested
        </div>
        <div style={{ fontSize: 15, fontWeight: 600, marginBottom: 12 }}>
          A connected AI client wants to perform actions on your behalf.
        </div>
        <div
          style={{
            fontSize: 13,
            lineHeight: 1.5,
            background: "var(--surface-2)",
            border: "1px solid var(--line-soft)",
            borderRadius: 6,
            padding: "10px 12px",
            marginBottom: 14,
            whiteSpace: "pre-wrap",
            fontFamily: "var(--mono)",
          }}
        >
          {head.summary}
        </div>
        <div
          style={{
            fontSize: 11,
            color: "var(--text-dim)",
            marginBottom: 12,
          }}
        >
          Approving issues a single-batch token valid for 60 seconds. Mutating
          tools beyond that window will need a fresh approval.
        </div>
        {queue.length > 1 && (
          <div
            style={{
              fontSize: 11,
              color: "var(--text-dim)",
              marginBottom: 12,
            }}
          >
            +{queue.length - 1} more pending after this one.
          </div>
        )}
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
          <button
            onClick={() => decide(false)}
            style={{
              padding: "7px 14px",
              fontSize: 13,
              background: "transparent",
              border: "1px solid var(--line)",
              borderRadius: 6,
              color: "var(--text)",
              cursor: "pointer",
              fontFamily: "var(--sans)",
            }}
          >
            Reject
          </button>
          <button
            onClick={() => decide(true)}
            style={{
              padding: "7px 14px",
              fontSize: 13,
              background: "var(--accent)",
              border: "1px solid var(--accent)",
              borderRadius: 6,
              color: "white",
              cursor: "pointer",
              fontFamily: "var(--sans)",
              fontWeight: 600,
            }}
          >
            Approve
          </button>
        </div>
      </div>
    </div>
  );
}
