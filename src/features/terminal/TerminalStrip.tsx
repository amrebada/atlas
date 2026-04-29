import { useCallback, useEffect } from "react";
import type { CSSProperties, ReactNode } from "react";
import {
  DndContext,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  horizontalListSortingStrategy,
  rectSortingStrategy,
  useSortable,
  verticalListSortingStrategy,
  type SortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { useUiStore } from "../../state/store";
import {
  paneLayoutSave,
  terminalClose,
  terminalOpen,
} from "../../ipc";
import type { Pane, PaneKind } from "../../types";
import {
  makePane,
  snapshotLayout,
  useTerminalStore,
  type LayoutMode,
  type PaneLayout,
} from "./layout";
import { TerminalPane } from "./TerminalPane";
import { useTerminalEvents } from "../../hooks/useTerminalEvents";
import { PaneHeader } from "./PaneHeader";

// Atlas - multi-pane terminal strip.

interface TerminalStripProps {
  projectId: string | null;
  // Project name - attached to new panes opened from this project so
  projectLabel?: string | null;
  /** Project cwd - used as the default for "+ new shell". */
  projectPath: string | null;
  /** Optional branch label displayed on pane mini-headers. */
  branch?: string | null;
}

export function TerminalStrip({
  projectId,
  projectLabel,
  projectPath,
  branch,
}: TerminalStripProps) {
  const panes = useTerminalStore((s) => s.panes);
  const activePaneId = useTerminalStore((s) => s.activePaneId);
  const layout = useTerminalStore((s) => s.layout);
  const maxed = useTerminalStore((s) => s.maxed);
  const collapsed = useTerminalStore((s) => s.collapsed);
  const addPane = useTerminalStore((s) => s.addPane);
  const closePaneLocal = useTerminalStore((s) => s.closePane);
  const closeAll = useTerminalStore((s) => s.closeAll);
  const setLayout = useTerminalStore((s) => s.setLayout);
  const setMaxed = useTerminalStore((s) => s.setMaxed);
  const setCollapsed = useTerminalStore((s) => s.setCollapsed);
  const setActive = useTerminalStore((s) => s.setActive);
  const movePane = useTerminalStore((s) => s.movePane);
  const pushToast = useUiStore((s) => s.pushToast);

  // 8 px activation distance keeps plain clicks (focus tab, close, rerun)
  // from being swallowed as drag starts.
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
  );

  const handleDragEnd = useCallback(
    (e: DragEndEvent) => {
      const { active, over } = e;
      if (!over || active.id === over.id) return;
      movePane(String(active.id), String(over.id));
    },
    [movePane],
  );

  // Global fan-out of terminal events → status patches. Mounted here (once,
  useTerminalEvents();

  // Keyboard: ⌃⌘F toggles maximize. Scoped to this component so it only
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.metaKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        setMaxed(!maxed);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [maxed, setMaxed]);

  // NOTE: Project-switch restore used to live here, but TerminalStrip is

  // Persist layout with a 500 ms debounce whenever panes/layout changes.
  useEffect(() => {
    if (!projectId) return;
    const t = window.setTimeout(() => {
      const snapshot: PaneLayout = snapshotLayout();
      paneLayoutSave(projectId, snapshot).catch(() => {
        /* D6 may not have registered - swallow quietly */
      });
    }, 500);
    return () => window.clearTimeout(t);
  }, [projectId, panes, layout, activePaneId]);

  const openShell = useCallback(async () => {
    if (!projectPath) {
      pushToast("warn", "Select a project to open a shell");
      return;
    }
    try {
      const id = await terminalOpen({ kind: "shell", cwd: projectPath });
      addPane(
        makePane(id, "shell", projectPath, prettyCwd(projectPath), {
          ...(branch ? { branch } : {}),
          ...(projectId ? { projectId } : {}),
          ...(projectLabel ? { projectLabel } : {}),
        }),
      );
    } catch (err) {
      pushToast("error", `Open shell failed: ${String(err)}`);
    }
  }, [addPane, branch, projectId, projectLabel, projectPath, pushToast]);

  const closePane = useCallback(
    (id: string) => {
      closePaneLocal(id);
      terminalClose(id).catch(() => {});
    },
    [closePaneLocal],
  );

  // Respawn a script pane: kill the current PTY, open a fresh one with the
  const rerunPane = useCallback(
    async (pane: Pane) => {
      if (pane.kind === "claude-session") {
        pushToast("info", "Claude sessions can't be rerun");
        return;
      }
      const store = useTerminalStore.getState();
      try {
        await terminalClose(pane.id).catch(() => {});
        const newId = await terminalOpen({
          kind: pane.kind,
          cwd: pane.cwd,
          command: pane.command,
          args: pane.args,
        });
        store.replacePaneId(pane.id, newId, { status: "running" });
      } catch (err) {
        pushToast("error", `Rerun "${pane.title}" failed: ${String(err)}`);
      }
    },
    [pushToast],
  );

  const closeEverything = useCallback(() => {
    panes.forEach((p) => terminalClose(p.id).catch(() => {}));
    closeAll();
  }, [closeAll, panes]);

  // Hide entirely when there are no panes AND not maxed.
  if (panes.length === 0 && !maxed) return null;

  const stripStyle: CSSProperties = maxed
    ? {
        position: "fixed",
        inset: 0,
        zIndex: 300,
        background: "var(--surface)",
        display: "flex",
        flexDirection: "column",
      }
    : collapsed
      ? {
          // Only the top bar shows. No flex-grow so the parent grid row
          height: 30,
          borderTop: "1px solid var(--line)",
          background: "var(--surface)",
          display: "flex",
          flexDirection: "column",
          minWidth: 0,
          flexShrink: 0,
        }
      : {
          // Cap at 40% of available vertical space. We resolve percentage via
          height: "40vh",
          minHeight: 200,
          maxHeight: "60vh",
          borderTop: "1px solid var(--line)",
          background: "var(--surface)",
          display: "flex",
          flexDirection: "column",
          minWidth: 0,
        };

  return (
    <div style={stripStyle}>
      {/* Top bar */}
      <div
        style={{
          height: 30,
          display: "flex",
          alignItems: "center",
          borderBottom: "1px solid var(--line)",
          background: "var(--chrome)",
          flexShrink: 0,
          minWidth: 0,
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "stretch",
            overflow: "auto",
            flex: 1,
            minWidth: 0,
          }}
        >
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragEnd={handleDragEnd}
          >
            <SortableContext
              items={panes.map((p) => p.id)}
              strategy={horizontalListSortingStrategy}
            >
              {panes.map((p) => (
                <TabChip
                  key={p.id}
                  pane={p}
                  active={p.id === activePaneId}
                  canClose={panes.length > 1 || maxed}
                  onClick={() => setActive(p.id)}
                  onClose={() => closePane(p.id)}
                  onRerun={
                    p.kind === "claude-session"
                      ? undefined
                      : () => rerunPane(p)
                  }
                />
              ))}
            </SortableContext>
          </DndContext>
          <button
            onClick={openShell}
            title="New shell"
            aria-label="Open new shell pane"
            style={{
              padding: "0 10px",
              height: 30,
              background: "transparent",
              border: "none",
              borderRight: "1px solid var(--line)",
              color: "var(--text-dim)",
              cursor: "pointer",
              fontFamily: "var(--mono)",
              fontSize: 14,
              flexShrink: 0,
            }}
          >
            +
          </button>
        </div>

        <div
          style={{
            display: "flex",
            gap: 1,
            padding: "0 6px",
            flexShrink: 0,
          }}
        >
          <LayoutBtn
            active={layout === "tabs"}
            onClick={() => setLayout("tabs")}
            title="Tabs"
          >
            {/* A small tab on top, one content pane below. */}
            <rect x="2" y="5" width="12" height="9" rx="1" />
            <path d="M4 5V3h4v2" />
          </LayoutBtn>
          <LayoutBtn
            active={layout === "split-v"}
            onClick={() => setLayout("split-v")}
            title="Vertical split"
          >
            <rect x="2" y="3" width="12" height="10" rx="1" />
            <path d="M8 3v10" />
          </LayoutBtn>
          <LayoutBtn
            active={layout === "split-h"}
            onClick={() => setLayout("split-h")}
            title="Horizontal split"
          >
            <rect x="2" y="3" width="12" height="10" rx="1" />
            <path d="M2 8h12" />
          </LayoutBtn>
          <LayoutBtn
            active={layout === "grid"}
            onClick={() => setLayout("grid")}
            title="Grid"
          >
            <rect x="2" y="3" width="12" height="10" rx="1" />
            <path d="M2 8h12M8 3v10" />
          </LayoutBtn>
        </div>

        <button
          onClick={() => setCollapsed(!collapsed)}
          title={collapsed ? "Expand terminal" : "Collapse terminal"}
          aria-label={collapsed ? "Expand terminal" : "Collapse terminal"}
          aria-pressed={collapsed}
          style={iconBtn()}
        >
          <CollapseIcon collapsed={collapsed} />
        </button>
        <button
          onClick={() => setMaxed(!maxed)}
          title={maxed ? "Restore (⌃⌘F)" : "Maximize (⌃⌘F)"}
          aria-label={maxed ? "Restore terminal" : "Maximize terminal"}
          style={iconBtn()}
        >
          <MaxIcon maxed={maxed} />
        </button>
        <button
          onClick={closeEverything}
          title="Close all"
          aria-label="Close all terminal panes"
          style={iconBtn()}
        >
          ×
        </button>
      </div>

      {/* Pane area — hidden when collapsed. We keep `<PaneArea>` mounted
          behind `display:none` so xterm.js instances aren't torn down,
          which would otherwise drop scrollback on every collapse. */}
      <div
        style={{
          flex: 1,
          minHeight: 0,
          display: collapsed ? "none" : "flex",
          background: "var(--bg)",
        }}
      >
        <PaneArea
          panes={panes}
          layout={layout}
          activePaneId={activePaneId}
          onFocus={setActive}
          onClose={closePane}
          onRerun={rerunPane}
          sensors={sensors}
          onDragEnd={handleDragEnd}
        />
      </div>
    </div>
  );
}

// -----------------------------------------------------------------------------

interface PaneAreaProps {
  panes: Pane[];
  layout: LayoutMode;
  activePaneId: string | null;
  onFocus: (id: string) => void;
  onClose: (id: string) => void;
  onRerun: (pane: Pane) => void;
  sensors: ReturnType<typeof useSensors>;
  onDragEnd: (e: DragEndEvent) => void;
}

function PaneArea({
  panes,
  layout,
  activePaneId,
  onFocus,
  onClose,
  onRerun,
  sensors,
  onDragEnd,
}: PaneAreaProps) {
  if (panes.length === 0) return null;
  const active = panes.find((p) => p.id === activePaneId) ?? panes[0];

  // Every layout uses the SAME DOM shape: one `<div key={id}>` wrapper
  const containerStyle: CSSProperties =
    layout === "tabs"
      ? { position: "relative", flex: 1, minWidth: 0 }
      : layout === "split-v"
        ? {
            display: "grid",
            gridTemplateColumns: `repeat(${panes.length}, 1fr)`,
            gap: 1,
            background: "var(--line)",
            flex: 1,
            minWidth: 0,
          }
        : layout === "split-h"
          ? {
              display: "grid",
              gridTemplateRows: `repeat(${panes.length}, 1fr)`,
              gap: 1,
              background: "var(--line)",
              flex: 1,
              minWidth: 0,
            }
          : gridContainerStyle(panes.length);

  // Tabs layout shows a single pane with the others kept mounted at
  // `display:none`. There is no visible spatial relationship to drag, so
  // this layout opts out of pane-area DnD - reordering is still available
  // via the tab strip above.
  if (layout === "tabs") {
    return (
      <div style={containerStyle}>
        {panes.map((p) => {
          const isActive = p.id === active.id;
          return (
            <div
              key={p.id}
              style={{
                position: "absolute",
                inset: 0,
                display: isActive ? "flex" : "none",
                flexDirection: "column",
                background: "var(--bg)",
              }}
            >
              <div style={{ flex: 1, minHeight: 0, display: "flex" }}>
                <TerminalPane pane={p} focused={isActive} />
              </div>
            </div>
          );
        })}
      </div>
    );
  }

  const strategy: SortingStrategy =
    layout === "split-v"
      ? horizontalListSortingStrategy
      : layout === "split-h"
        ? verticalListSortingStrategy
        : rectSortingStrategy;

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={onDragEnd}
    >
      <SortableContext items={panes.map((p) => p.id)} strategy={strategy}>
        <div style={containerStyle}>
          {panes.map((p) => (
            <SortablePaneCell
              key={p.id}
              pane={p}
              active={p.id === active.id}
              onClose={() => onClose(p.id)}
              onRerun={() => onRerun(p)}
              onFocus={() => onFocus(p.id)}
            />
          ))}
        </div>
      </SortableContext>
    </DndContext>
  );
}

// One draggable pane in a split / grid layout. The pane header acts as
// the drag handle so users keep full pointer access to the terminal body
// (xterm.js needs unimpeded clicks for selection / focus).
function SortablePaneCell({
  pane,
  active,
  onClose,
  onRerun,
  onFocus,
}: {
  pane: Pane;
  active: boolean;
  onClose: () => void;
  onRerun: () => void;
  onFocus: () => void;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: pane.id });

  return (
    <div
      ref={setNodeRef}
      style={{
        display: "flex",
        flexDirection: "column",
        minWidth: 0,
        minHeight: 0,
        background: "var(--bg)",
        transform: CSS.Transform.toString(transform),
        transition,
        opacity: isDragging ? 0.6 : 1,
        zIndex: isDragging ? 10 : undefined,
      }}
    >
      <div
        {...attributes}
        {...listeners}
        style={{ touchAction: "none", cursor: "grab" }}
      >
        <PaneHeader
          pane={pane}
          active={active}
          onClose={onClose}
          onRerun={onRerun}
        />
      </div>
      <div style={{ flex: 1, minHeight: 0, display: "flex" }}>
        <TerminalPane pane={pane} focused={active} onFocus={onFocus} />
      </div>
    </div>
  );
}

/** 2-column grid, row count grows with pane count (1→1r, 2→1r, 3-4→2r, 5-6→3r, …). */
function gridContainerStyle(count: number): CSSProperties {
  const cols = count <= 1 ? 1 : 2;
  const rows = Math.max(1, Math.ceil(count / cols));
  return {
    display: "grid",
    gridTemplateColumns: `repeat(${cols}, 1fr)`,
    gridTemplateRows: `repeat(${rows}, 1fr)`,
    gap: 1,
    background: "var(--line)",
    flex: 1,
    minWidth: 0,
  };
}

// -----------------------------------------------------------------------------

function TabChip({
  pane,
  active,
  canClose,
  onClick,
  onClose,
  onRerun,
}: {
  pane: Pane;
  active: boolean;
  canClose: boolean;
  onClick: () => void;
  onClose: () => void;
  onRerun?: () => void;
}) {
  const dot = statusDot(pane);
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: pane.id });
  return (
    <div
      ref={setNodeRef}
      {...attributes}
      {...listeners}
      onClick={onClick}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        padding: "0 6px 0 10px",
        height: 30,
        cursor: isDragging ? "grabbing" : "pointer",
        borderRight: "1px solid var(--line)",
        background: active ? "var(--surface)" : "transparent",
        color: active ? "var(--text)" : "var(--text-dim)",
        fontFamily: "var(--mono)",
        fontSize: 11,
        maxWidth: 240,
        minWidth: 0,
        borderTop: "2px solid " + (active ? "var(--accent)" : "transparent"),
        marginTop: -1,
        transform: CSS.Transform.toString(transform),
        transition,
        opacity: isDragging ? 0.6 : 1,
        zIndex: isDragging ? 10 : undefined,
        touchAction: "none",
      }}
    >
      <span
        style={{
          width: 6,
          height: 6,
          borderRadius: "50%",
          background: dot.color,
          boxShadow: dot.glow,
          flexShrink: 0,
        }}
      />
      <span
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          display: "inline-flex",
          alignItems: "center",
          gap: 6,
          minWidth: 0,
        }}
      >
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
            minWidth: 0,
          }}
        >
          {pane.kind === "claude-session" ? "⎔ " : ""}
          {pane.title}
        </span>
      </span>
      {onRerun && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onRerun();
          }}
          title={pane.kind === "script" ? "Rerun script" : "Restart shell"}
          aria-label={`Rerun ${pane.title}`}
          style={tabActionBtn()}
        >
          <RerunIcon />
        </button>
      )}
      {canClose && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onClose();
          }}
          title="Kill"
          aria-label={`Close terminal pane ${pane.title}`}
          style={tabActionBtn()}
        >
          ×
        </button>
      )}
    </div>
  );
}

function tabActionBtn(): CSSProperties {
  return {
    width: 18,
    height: 18,
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    background: "transparent",
    border: "none",
    color: "var(--text-dimmer)",
    borderRadius: 3,
    padding: 0,
    cursor: "pointer",
    flexShrink: 0,
    fontSize: 13,
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

function statusDot(pane: Pane): { color: string; glow: string } {
  const kindHint: string =
    pane.kind === "claude-session" ? "oklch(0.7 0.18 300)" : "var(--text-dim)";
  switch (pane.status) {
    case "running":
      return { color: "var(--accent)", glow: "0 0 6px var(--accent)" };
    case "active":
      return { color: "var(--accent)", glow: "0 0 6px var(--accent)" };
    case "error":
      return { color: "var(--danger)", glow: "none" };
    case "idle":
    default:
      return { color: kindHint, glow: "none" };
  }
}

function LayoutBtn({
  active,
  onClick,
  title,
  children,
}: {
  active: boolean;
  onClick: () => void;
  title: string;
  children: ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      aria-label={title}
      aria-pressed={active}
      style={{
        width: 26,
        height: 22,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        background: active ? "var(--row-active)" : "transparent",
        border: "1px solid " + (active ? "var(--line)" : "transparent"),
        borderRadius: 3,
        cursor: "pointer",
        color: active ? "var(--accent)" : "var(--text-dim)",
        padding: 0,
      }}
    >
      <svg
        width="14"
        height="14"
        viewBox="0 0 16 16"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      >
        {children}
      </svg>
    </button>
  );
}

function CollapseIcon({ collapsed }: { collapsed: boolean }) {
  // Chevron pointing DOWN when expanded (click → collapses downward),
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {collapsed ? <path d="M4 10l4-4 4 4" /> : <path d="M4 6l4 4 4-4" />}
    </svg>
  );
}

function MaxIcon({ maxed }: { maxed: boolean }) {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
    >
      {maxed ? (
        <>
          <path d="M7 3v4H3" />
          <path d="M9 3v4h4" />
          <path d="M7 13V9H3" />
          <path d="M9 13V9h4" />
        </>
      ) : (
        <>
          <path d="M3 6V3h3" />
          <path d="M13 6V3h-3" />
          <path d="M3 10v3h3" />
          <path d="M13 10v3h-3" />
        </>
      )}
    </svg>
  );
}

function iconBtn(): CSSProperties {
  return {
    width: 30,
    height: 30,
    background: "transparent",
    border: "none",
    borderLeft: "1px solid var(--line)",
    color: "var(--text-dim)",
    cursor: "pointer",
    flexShrink: 0,
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    padding: 0,
    fontSize: 14,
    fontFamily: "var(--sans)",
  };
}

function prettyCwd(cwd: string): string {
  if (!cwd) return "~";
  const parts = cwd.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? "~";
}

// -----------------------------------------------------------------------------

// Spawn a pane for a `script` via the PTY and register it in the strip.
export async function spawnScriptPane(args: {
  projectId: string;
  projectLabel?: string;
  cwd: string;
  scriptId: string;
  scriptName: string;
  cmd: string;
  branch?: string | null;
}): Promise<string | null> {
  try {
    // Omit `command` so the backend uses the user's `$SHELL` and prepends
    // `-i -l`. Aliases and version-manager init from `.zshrc`/`.zprofile`
    // are then available inside the script.
    const cmdArgs = ["-c", args.cmd];
    const id = await terminalOpen({
      kind: "script",
      cwd: args.cwd,
      args: cmdArgs,
    });
    useTerminalStore.getState().addPane({
      id,
      kind: "script",
      title: args.scriptName,
      status: "running",
      cwd: args.cwd,
      scriptId: args.scriptId,
      branch: args.branch ?? undefined,
      projectId: args.projectId,
      projectLabel: args.projectLabel,
      args: cmdArgs,
    });
    return id;
  } catch (err) {
    // eslint-disable-next-line no-console
    console.error("[atlas] spawnScriptPane failed:", err);
    useUiStore
      .getState()
      .pushToast("error", `Run "${args.scriptName}" failed: ${String(err)}`);
    return null;
  }
}

// Spawn a `claude-session` pane. Title is the session id prefix so
export async function spawnSessionPane(args: {
  sessionId: string;
  cwd: string;
  command: string;
  cmdArgs: string[];
  env?: Array<[string, string]>;
  title?: string;
  branch?: string | null;
  projectId?: string;
  projectLabel?: string;
}): Promise<string | null> {
  try {
    const id = await terminalOpen({
      kind: "claude-session",
      cwd: args.cwd,
      command: args.command,
      args: args.cmdArgs,
      env: args.env,
    });
    const title =
      args.title ?? `session ${args.sessionId.slice(0, 8)}`;
    useTerminalStore.getState().addPane({
      id,
      kind: "claude-session",
      title,
      status: "active",
      cwd: args.cwd,
      sessionId: args.sessionId,
      branch: args.branch ?? undefined,
      projectId: args.projectId,
      projectLabel: args.projectLabel,
    });
    return id;
  } catch (err) {
    // eslint-disable-next-line no-console
    console.error("[atlas] spawnSessionPane failed:", err);
    useUiStore
      .getState()
      .pushToast("error", `Resume session failed: ${String(err)}`);
    return null;
  }
}

// Kind literal re-exported for external typing of call sites that want to
export type { PaneKind };
