import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Icon, type IconName } from "../../Icon";
import { TabEmpty, TabError, TabSkeleton } from "../TabStates";
import { listSessions, sessionResumeInfo } from "../../../ipc";
import { useUiStore } from "../../../state/store";
import { spawnSessionPane } from "../../../features/terminal/TerminalStrip";
import type { Project, Session, SessionStatus } from "../../../types";

// Inspector / Sessions tab. Lists Claude Code sessions discovered under

interface SessionsProps {
  project: Project;
}

export function Sessions({ project }: SessionsProps) {
  const pushToast = useUiStore((s) => s.pushToast);

  const { data, isLoading, error, refetch } = useQuery<Session[]>({
    queryKey: ["sessions", project.id],
    queryFn: () => listSessions(project.id),
    staleTime: 15_000,
    retry: false,
  });

  const sessions = data ?? [];

  const openInClaude = async (session: Session) => {
    // "Open in Claude" still surfaces resume info as a toast - the real
    await resumeToast(session, pushToast, "claude");
  };

  const openInTerminal = async (session: Session) => {
    try {
      const info = await sessionResumeInfo(session.id);
      const id = await spawnSessionPane({
        sessionId: session.id,
        cwd: info.cwd || project.path,
        command: info.command,
        cmdArgs: info.args,
        title: session.title
          ? `${session.id.slice(0, 6)} · ${session.title}`
          : `session ${session.id.slice(0, 8)}`,
        branch: session.branch ?? project.branch ?? null,
        projectId: project.id,
        projectLabel: project.name,
      });
      if (!id) {
        // Terminal backend not available yet - fall back to the info toast.
        await resumeToast(session, pushToast, "terminal");
      } else {
        pushToast(
          "success",
          `Resuming session ${session.id.slice(0, 8)} in terminal`,
        );
      }
    } catch (err) {
      pushToast("error", `Resume failed: ${String(err)}`);
    }
  };

  const startNewSession = async () => {
    // "New session" now spawns a shell pane anchored to the project cwd so
    const id = await spawnSessionPane({
      sessionId: `new-${Date.now().toString(36)}`,
      cwd: project.path,
      command: "claude",
      cmdArgs: [],
      title: `new session · ${project.name}`,
      branch: project.branch ?? null,
      projectId: project.id,
      projectLabel: project.name,
    });
    if (!id) {
      pushToast(
        "warn",
        "Terminal backend unavailable — try again after restart",
      );
    }
  };

  return (
    <div className="flex flex-col h-full">
      <div className="px-[14px] pt-[14px] pb-[10px] flex items-center gap-2 shrink-0">
        <span className="font-mono text-[10px] text-text-dim uppercase tracking-[0.6px]">
          {sessions.length}{" "}
          {sessions.length === 1 ? "SESSION" : "SESSIONS"}
        </span>
        <div className="flex-1" />
        <button
          type="button"
          onClick={startNewSession}
          className="inline-flex items-center gap-[5px] px-[8px] py-[3px] rounded-[3px] font-mono text-[10px] font-semibold"
          style={{
            background: "var(--accent)",
            color: "var(--accent-fg)",
            border: "none",
          }}
        >
          <Icon name="plus" size={10} stroke="var(--accent-fg)" />
          new session
        </button>
      </div>

      <div className="flex-1 min-h-0 px-[14px] pb-[14px] overflow-y-auto">
        {isLoading && !data && <TabSkeleton rows={3} />}
        {error && (
          <TabError
            message={error instanceof Error ? error.message : String(error)}
            onRetry={() => void refetch()}
          />
        )}
        {!isLoading && !error && sessions.length === 0 && (
          <TabEmpty
            icon="term"
            title="No Claude Code sessions yet"
            hint="Start one from the terminal or press + new session"
          />
        )}
        {sessions.map((s) => (
          <SessionCard
            key={s.id}
            session={s}
            onOpenInClaude={() => void openInClaude(s)}
            onOpenInTerminal={() => void openInTerminal(s)}
          />
        ))}
      </div>
    </div>
  );
}

// ---- card ----------------------------------------------------------------

interface SessionCardProps {
  session: Session;
  onOpenInClaude: () => void;
  onOpenInTerminal: () => void;
}

function SessionCard({
  session,
  onOpenInClaude,
  onOpenInTerminal,
}: SessionCardProps) {
  const pushToast = useUiStore((s) => s.pushToast);
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  // Close the overflow menu on outside click.
  useEffect(() => {
    if (!menuOpen) return;
    const onDoc = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    const t = window.setTimeout(
      () => document.addEventListener("click", onDoc),
      0,
    );
    return () => {
      window.clearTimeout(t);
      document.removeEventListener("click", onDoc);
    };
  }, [menuOpen]);

  const statusColor = getStatusColor(session.status);
  const statusGlow =
    session.status === "active" ? "0 0 6px var(--accent)" : "none";
  const borderColor =
    session.status === "active"
      ? "oklch(0.78 0.17 145 / 0.3)"
      : "transparent";

  const copyId = async () => {
    try {
      await navigator.clipboard.writeText(session.id);
      pushToast("info", `Copied session ID: ${session.id.slice(0, 8)}…`);
    } catch {
      pushToast("error", "Clipboard unavailable");
    }
    setMenuOpen(false);
  };

  const archive = () => {
    pushToast("info", "Session archival is not available yet");
    setMenuOpen(false);
  };
  const del = () => {
    pushToast("info", "Deleting sessions is not available yet");
    setMenuOpen(false);
  };

  return (
    <div
      className="session-card group relative px-[12px] py-[10px] rounded-[5px] mb-[8px]"
      style={{
        background: "var(--surface-2)",
        border: `1px solid ${borderColor}`,
        transition: "background 120ms, border-color 120ms",
      }}
    >
      <div className="flex items-center gap-[6px] mb-[6px]">
        <span
          className="w-[6px] h-[6px] rounded-full shrink-0"
          style={{ background: statusColor, boxShadow: statusGlow }}
        />
        <span className="text-[12px] font-semibold text-text flex-1 truncate">
          {session.title || "Untitled session"}
        </span>
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            setMenuOpen((v) => !v);
          }}
          className="w-[22px] h-[22px] inline-flex items-center justify-center rounded-[3px]"
          style={{
            background: "transparent",
            border: "none",
            color: "var(--text-dim)",
          }}
          title="More actions"
          aria-label="More actions"
          aria-haspopup="menu"
          aria-expanded={menuOpen}
        >
          <Icon name="more" size={12} stroke="var(--text-dim)" />
        </button>
      </div>

      <div
        className="text-[11px] leading-snug text-text-dim mb-[8px]"
        style={{
          display: "-webkit-box",
          WebkitLineClamp: 2,
          WebkitBoxOrient: "vertical",
          overflow: "hidden",
        }}
      >
        {session.last || (
          <span className="text-text-dimmer italic">No messages yet</span>
        )}
      </div>

      <div className="font-mono text-[10px] text-text-dimmer flex flex-wrap items-center gap-[6px]">
        <span className="inline-flex items-center gap-[3px]">
          <Icon name="clock" size={10} stroke="var(--text-dimmer)" />
          {session.when ? formatRelative(session.when) : "—"}
        </span>
        <span>·</span>
        <span>{session.turns} turns</span>
        <span>·</span>
        <span>{session.duration || "—"}</span>
        {session.branch && (
          <>
            <span>·</span>
            <span className="inline-flex items-center gap-[3px]">
              <Icon
                name="branch"
                size={10}
                stroke="var(--text-dimmer)"
              />
              {session.branch}
            </span>
          </>
        )}
      </div>

      {/* hover-reveal actions */}
      <div
        className="flex gap-[4px] mt-[8px] opacity-0 group-hover:opacity-100 transition-opacity"
      >
        <button
          type="button"
          onClick={onOpenInClaude}
          className="inline-flex items-center gap-[5px] px-[8px] py-[3px] rounded-[3px] font-mono text-[10px] font-semibold"
          style={{
            background: "var(--accent)",
            color: "var(--accent-fg)",
            border: "none",
          }}
        >
          <Icon name="sparkle" size={10} stroke="var(--accent-fg)" />
          Open in Claude
        </button>
        <button
          type="button"
          onClick={onOpenInTerminal}
          className="inline-flex items-center gap-[5px] px-[8px] py-[3px] rounded-[3px] font-mono text-[10px]"
          style={{
            background: "transparent",
            color: "var(--text-dim)",
            border: "1px solid var(--line)",
          }}
        >
          <Icon name="term" size={10} stroke="var(--text-dim)" />
          Open in terminal
        </button>
      </div>

      {menuOpen && (
        <div
          ref={menuRef}
          role="menu"
          aria-label="Session actions"
          onClick={(e) => e.stopPropagation()}
          className="absolute top-[30px] right-[8px] z-20 rounded-[5px] p-[4px] text-[12px]"
          style={{
            minWidth: 200,
            background: "var(--surface)",
            border: "1px solid var(--line)",
            boxShadow: "0 8px 20px rgba(0,0,0,0.3)",
          }}
        >
          <MenuRow
            icon="sparkle"
            label="Open in Claude"
            onClick={() => {
              onOpenInClaude();
              setMenuOpen(false);
            }}
          />
          <MenuRow
            icon="term"
            label="Open in terminal tab"
            hint="↵"
            onClick={() => {
              onOpenInTerminal();
              setMenuOpen(false);
            }}
          />
          <MenuRow icon="copy" label="Copy session ID" onClick={copyId} />
          <div
            className="h-px my-[3px]"
            style={{ background: "var(--line)" }}
          />
          <MenuRow icon="arch" label="Archive" onClick={archive} />
          <MenuRow icon="trash" label="Delete" danger onClick={del} />
        </div>
      )}

      <style>{`
        .session-card:hover {
          background: var(--surface) !important;
        }
      `}</style>
    </div>
  );
}

// ---- small primitives ----------------------------------------------------

function MenuRow({
  icon,
  label,
  hint,
  onClick,
  danger,
}: {
  icon: IconName;
  label: string;
  hint?: string;
  onClick: () => void;
  danger?: boolean;
}) {
  return (
    <div
      onClick={onClick}
      role="menuitem"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick();
        }
      }}
      className="flex items-center gap-[8px] px-[8px] py-[5px] rounded-[3px] cursor-pointer text-[12px] hover:bg-[var(--row-active)]"
      style={{ color: danger ? "var(--danger)" : "var(--text)" }}
    >
      <Icon
        name={icon}
        size={11}
        stroke={danger ? "var(--danger)" : "var(--text-dim)"}
      />
      <span className="flex-1">{label}</span>
      {hint && (
        <span
          className="font-mono text-[10px]"
          style={{ color: "var(--text-dimmer)" }}
        >
          {hint}
        </span>
      )}
    </div>
  );
}

// ---- helpers -------------------------------------------------------------

function getStatusColor(status: SessionStatus): string {
  switch (status) {
    case "active":
      return "var(--accent)";
    case "idle":
      return "var(--text-dim)";
    case "archived":
    default:
      return "var(--text-dimmer)";
  }
}

async function resumeToast(
  session: Session,
  pushToast: (
    kind: "info" | "success" | "warn" | "error",
    message: string,
  ) => void,
  target: "claude" | "terminal",
): Promise<void> {
  try {
    const info = await sessionResumeInfo(session.id);
    const argv = [info.command, ...info.args].map((tok) =>
      tok.includes(" ") ? JSON.stringify(tok) : tok,
    );
    const cmd = argv.join(" ");
    const label = target === "claude" ? "Open in Claude" : "Open in terminal";
    pushToast("info", `${label}: \`${cmd}\` in ${info.cwd}`);
  } catch (err) {
    const label = target === "claude" ? "Open in Claude" : "Open in terminal";
    pushToast(
      "info",
      `${label}: \`claude --resume ${session.id}\` - ${err instanceof Error ? err.message : String(err)}`,
    );
  }
}

function formatRelative(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const diffMs = Date.now() - d.getTime();
  const mins = Math.floor(diffMs / 60_000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  return d.toISOString().slice(0, 10);
}
