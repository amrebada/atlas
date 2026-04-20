import type { CSSProperties } from "react";
import type { Pane } from "../../types";

// Header strip painted above each `TerminalPane` in non-tabs layouts.
export function PaneHeader({
  pane,
  active,
  onClose,
  onRerun,
}: {
  pane: Pane;
  active: boolean;
  onClose: () => void;
  onRerun: () => void;
}) {
  const rerunnable = pane.kind !== "claude-session";
  const dot = statusColor(pane);
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 6,
        height: 22,
        padding: "0 6px 0 8px",
        background: active ? "var(--chrome)" : "var(--surface)",
        borderBottom: "1px solid var(--line)",
        fontFamily: "var(--mono)",
        fontSize: 10,
        color: active ? "var(--text)" : "var(--text-dim)",
        flexShrink: 0,
        minWidth: 0,
      }}
    >
      <span
        style={{
          width: 6,
          height: 6,
          borderRadius: "50%",
          background: dot,
          flexShrink: 0,
        }}
      />
      {pane.projectLabel && (
        <span
          style={{
            fontSize: 9,
            fontWeight: 600,
            letterSpacing: 0.3,
            color: "var(--accent)",
            background: "var(--row-active)",
            border: "1px solid var(--line)",
            padding: "1px 5px",
            borderRadius: 3,
            whiteSpace: "nowrap",
            flexShrink: 0,
            textTransform: "uppercase",
          }}
          title={`Project: ${pane.projectLabel}`}
        >
          {pane.projectLabel}
        </span>
      )}
      <span
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          minWidth: 0,
        }}
        title={pane.title}
      >
        {pane.title}
      </span>
      <span style={{ flex: 1 }} />
      {rerunnable && (
        <button
          onClick={onRerun}
          title={pane.kind === "script" ? "Rerun script" : "Restart shell"}
          aria-label="Rerun"
          style={iconBtn()}
        >
          <RerunIcon />
        </button>
      )}
      <button
        onClick={onClose}
        title="Kill"
        aria-label="Kill pane"
        style={iconBtn(true)}
      >
        <KillIcon />
      </button>
    </div>
  );
}

function statusColor(pane: Pane): string {
  switch (pane.status) {
    case "running":
    case "active":
      return "var(--accent)";
    case "error":
      return "var(--danger)";
    case "idle":
    default:
      return "var(--text-dimmer)";
  }
}

function iconBtn(danger = false): CSSProperties {
  return {
    width: 18,
    height: 18,
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    background: "transparent",
    border: "none",
    borderRadius: 3,
    padding: 0,
    cursor: "pointer",
    color: danger ? "var(--danger)" : "var(--text-dim)",
    flexShrink: 0,
  };
}

function RerunIcon() {
  return (
    <svg
      width="11"
      height="11"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M13 4a5 5 0 1 0 1.3 4.7" />
      <path d="M13 2v3h-3" />
    </svg>
  );
}

function KillIcon() {
  return (
    <svg
      width="11"
      height="11"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
    >
      <path d="M4 4l8 8M12 4l-8 8" />
    </svg>
  );
}
