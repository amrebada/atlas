import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  closestCenter,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
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
  getActiveGroup,
  makePane,
  snapshotLayout,
  useTerminalStore,
  type LayoutMode,
  type PaneLayout,
  type TerminalGroup,
} from "./layout";
import { TerminalPane } from "./TerminalPane";
import { useTerminalEvents } from "../../hooks/useTerminalEvents";
import { PaneHeader } from "./PaneHeader";

// Atlas - multi-pane terminal strip.

// Distinguishes group chips from pane tabs in the shared DndContext id
// namespace. Pane ids are arbitrary uuids assigned by the PTY backend, so
// any prefix that doesn't appear there is fine.
const GROUP_DROP_PREFIX = "group:";

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
  const allPanes = useTerminalStore((s) => s.panes);
  const groups = useTerminalStore((s) => s.groups);
  const activeGroupId = useTerminalStore((s) => s.activeGroupId);
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
  const addGroup = useTerminalStore((s) => s.addGroup);
  const removeGroup = useTerminalStore((s) => s.removeGroup);
  const setActiveGroup = useTerminalStore((s) => s.setActiveGroup);
  const renameGroup = useTerminalStore((s) => s.renameGroup);
  const movePaneToGroup = useTerminalStore((s) => s.movePaneToGroup);
  const pushToast = useUiStore((s) => s.pushToast);

  // Pane being dragged — used to (a) draw a DragOverlay so the tab keeps
  // following the cursor when it leaves the SortableContext, (b) hide
  // self-drop affordances on the active group chip.
  const [draggingPaneId, setDraggingPaneId] = useState<string | null>(null);

  // Index panes by id once per render so each group can resolve its
  // ordered pane list without an O(n²) scan.
  const panesById = useMemo(() => {
    const m = new Map<string, Pane>();
    for (const p of allPanes) m.set(p.id, p);
    return m;
  }, [allPanes]);

  const activeGroup = useMemo(
    () => getActiveGroup({ groups, activeGroupId }),
    [groups, activeGroupId],
  );
  const activePanes = useMemo(
    () =>
      activeGroup.paneIds
        .map((id) => panesById.get(id))
        .filter((p): p is Pane => p != null),
    [activeGroup.paneIds, panesById],
  );
  const activePaneId = activeGroup.activePaneId;
  const layout = activeGroup.layout;

  // 8 px activation distance keeps plain clicks (focus tab, close, rerun)
  // from being swallowed as drag starts.
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
  );

  const handleDragStart = useCallback((e: DragStartEvent) => {
    setDraggingPaneId(String(e.active.id));
  }, []);

  const handleDragEnd = useCallback(
    (e: DragEndEvent) => {
      setDraggingPaneId(null);
      const { active, over } = e;
      if (!over) return;
      const overId = String(over.id);
      // Drops on the GroupsBar are encoded as "group:<id>" so they don't
      // collide with the pane-id namespace used for tab reordering.
      if (overId.startsWith(GROUP_DROP_PREFIX)) {
        const targetGroupId = overId.slice(GROUP_DROP_PREFIX.length);
        movePaneToGroup(String(active.id), targetGroupId);
        return;
      }
      if (active.id === over.id) return;
      movePane(String(active.id), String(over.id));
    },
    [movePane, movePaneToGroup],
  );

  const handleDragCancel = useCallback(() => {
    setDraggingPaneId(null);
  }, []);

  const draggingPane = useMemo(
    () =>
      draggingPaneId ? panesById.get(draggingPaneId) ?? null : null,
    [draggingPaneId, panesById],
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

  // Persist layout with a 500 ms debounce whenever panes/layout/group state
  // changes. Only the active group's snapshot is saved — see
  // `snapshotLayout` for the constraint and the no-op restore hook.
  useEffect(() => {
    if (!projectId) return;
    const t = window.setTimeout(() => {
      const snapshot: PaneLayout = snapshotLayout();
      paneLayoutSave(projectId, snapshot).catch(() => {
        /* D6 may not have registered - swallow quietly */
      });
    }, 500);
    return () => window.clearTimeout(t);
  }, [projectId, allPanes, groups, activeGroupId]);

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
    allPanes.forEach((p) => terminalClose(p.id).catch(() => {}));
    closeAll();
  }, [closeAll, allPanes]);

  const closeGroup = useCallback(
    (groupId: string) => {
      const target = useTerminalStore
        .getState()
        .groups.find((g) => g.id === groupId);
      if (!target) return;
      target.paneIds.forEach((pid) =>
        terminalClose(pid).catch(() => {}),
      );
      removeGroup(groupId);
    },
    [removeGroup],
  );

  const createGroup = useCallback(() => {
    addGroup();
  }, [addGroup]);

  // Hide entirely when there are no panes AND not maxed.
  if (allPanes.length === 0 && !maxed) return null;

  const stripStyle: CSSProperties = maxed
    ? {
        position: "fixed",
        // Leave the TitleBar (h-9 = 36px) visible so the user can still
        // drag the window, hit the palette, and see context while the
        // strip is maxed.
        top: 36,
        right: 0,
        bottom: 0,
        left: 0,
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
      {/* The DndContext spans GroupsBar + tab strip so dragging a pane tab
          onto a group chip moves the pane between groups. PaneArea owns
          its own DndContext (split-layout reorders) — those drags never
          escape the area. */}
      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
        onDragCancel={handleDragCancel}
      >
        {/* Groups bar - hidden while collapsed (to keep the strip at 30px)
            and while there's only a single group (nothing to switch
            between, so the bar would just be visual noise). The toolbar
            "+ Group" button below is the canonical create-group entry
            in that case. */}
        {!collapsed && groups.length > 1 && (
          <GroupsBar
            groups={groups}
            activeGroupId={activeGroupId}
            panesById={panesById}
            draggingPaneId={draggingPaneId}
            onSelect={setActiveGroup}
            onCreate={createGroup}
            onClose={closeGroup}
            onRename={renameGroup}
          />
        )}

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
            <SortableContext
              items={activePanes.map((p) => p.id)}
              strategy={horizontalListSortingStrategy}
            >
              {activePanes.map((p) => (
                <TabChip
                  key={p.id}
                  pane={p}
                  active={p.id === activePaneId}
                  canClose={activePanes.length > 1 || maxed}
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

        <button
          onClick={createGroup}
          title="New group"
          aria-label="Create new terminal group"
          style={{
            height: 22,
            padding: "0 8px",
            margin: "0 2px",
            background: "transparent",
            border: "1px solid var(--line)",
            borderRadius: 3,
            color: "var(--text-dim)",
            cursor: "pointer",
            fontFamily: "var(--mono)",
            fontSize: 10,
            display: "inline-flex",
            alignItems: "center",
            gap: 4,
            flexShrink: 0,
          }}
        >
          <NewGroupIcon />
          <span>Group</span>
        </button>

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

        {/* Floating ghost of the dragged tab — keeps a visual following the
            cursor when it crosses out of the SortableContext (e.g. onto a
            group chip). */}
        <DragOverlay dropAnimation={null}>
          {draggingPane ? <TabDragGhost pane={draggingPane} /> : null}
        </DragOverlay>
      </DndContext>

      {/* Pane area — every group is mounted so PTYs in inactive groups
          keep streaming. We toggle `visibility` (not `display`) so each
          xterm.js renderer keeps its layout box and continues drawing to
          its canvas in the background; switching groups then just
          composites a different layer instead of waking a paused
          renderer. The same trick is used by the per-group `tabs`
          sub-layout for non-active panes. */}
      <div
        style={{
          flex: 1,
          minHeight: 0,
          position: "relative",
          display: collapsed ? "none" : "block",
          background: "var(--bg)",
        }}
      >
        {groups.map((g) => {
          const visible = g.id === activeGroupId;
          const groupPanes = g.paneIds
            .map((id) => panesById.get(id))
            .filter((p): p is Pane => p != null);
          if (groupPanes.length === 0) return null;
          return (
            <div
              key={g.id}
              aria-hidden={!visible}
              style={{
                position: "absolute",
                inset: 0,
                display: "flex",
                flexDirection: "column",
                visibility: visible ? "visible" : "hidden",
                zIndex: visible ? 1 : 0,
              }}
            >
              <PaneArea
                panes={groupPanes}
                layout={g.layout}
                activePaneId={g.activePaneId}
                onFocus={setActive}
                onClose={closePane}
                onRerun={rerunPane}
                sensors={sensors}
                onDragEnd={handleDragEnd}
              />
            </div>
          );
        })}
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

  // Tabs layout shows a single pane on top with the others kept mounted
  // and laid out underneath; we toggle `visibility` so xterm's renderer
  // keeps painting in the background and the new tab is current the
  // moment it's composited. There is no visible spatial relationship to
  // drag, so this layout opts out of pane-area DnD — reordering is still
  // available via the tab strip above.
  if (layout === "tabs") {
    return (
      <div style={containerStyle}>
        {panes.map((p) => {
          const isActive = p.id === active.id;
          return (
            <div
              key={p.id}
              aria-hidden={!isActive}
              style={{
                position: "absolute",
                inset: 0,
                display: "flex",
                flexDirection: "column",
                background: "var(--bg)",
                visibility: isActive ? "visible" : "hidden",
                zIndex: isActive ? 1 : 0,
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

function GroupsBar({
  groups,
  activeGroupId,
  panesById,
  draggingPaneId,
  onSelect,
  onCreate,
  onClose,
  onRename,
}: {
  groups: TerminalGroup[];
  activeGroupId: string;
  panesById: Map<string, Pane>;
  draggingPaneId: string | null;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onClose: (id: string) => void;
  onRename: (id: string, name: string) => void;
}) {
  return (
    <div
      style={{
        height: 26,
        display: "flex",
        alignItems: "stretch",
        borderBottom: "1px solid var(--line)",
        background: "var(--surface)",
        flexShrink: 0,
        minWidth: 0,
        overflow: "auto",
      }}
    >
      {groups.map((g) => {
        const groupPanes = g.paneIds
          .map((id) => panesById.get(id))
          .filter((p): p is Pane => p != null);
        // Suppress the "drop here" highlight on the chip the dragged
        // pane already lives in — moving onto its current group is a
        // no-op in the store anyway.
        const ownsDragged =
          draggingPaneId != null && g.paneIds.includes(draggingPaneId);
        return (
          <GroupChip
            key={g.id}
            group={g}
            active={g.id === activeGroupId}
            paneCount={groupPanes.length}
            anyRunning={groupPanes.some(
              (p) => p.status === "running" || p.status === "active",
            )}
            anyError={groupPanes.some((p) => p.status === "error")}
            canClose={groups.length > 1}
            dropEnabled={draggingPaneId != null && !ownsDragged}
            onSelect={() => onSelect(g.id)}
            onClose={() => onClose(g.id)}
            onRename={(name) => onRename(g.id, name)}
          />
        );
      })}
      <button
        onClick={onCreate}
        title="New group"
        aria-label="Create new terminal group"
        style={{
          padding: "0 10px",
          height: 26,
          background: "transparent",
          border: "none",
          borderRight: "1px solid var(--line)",
          color: "var(--text-dim)",
          cursor: "pointer",
          fontFamily: "var(--mono)",
          fontSize: 13,
          flexShrink: 0,
        }}
      >
        +
      </button>
    </div>
  );
}

function GroupChip({
  group,
  active,
  paneCount,
  anyRunning,
  anyError,
  canClose,
  dropEnabled,
  onSelect,
  onClose,
  onRename,
}: {
  group: TerminalGroup;
  active: boolean;
  paneCount: number;
  anyRunning: boolean;
  anyError: boolean;
  canClose: boolean;
  dropEnabled: boolean;
  onSelect: () => void;
  onClose: () => void;
  onRename: (name: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(group.name);
  const inputRef = useRef<HTMLInputElement | null>(null);

  // Droppable target — pane tabs can be dragged here to move them between
  // groups. The id is namespaced so the shared DndContext can tell pane
  // ids from group ids in `handleDragEnd`.
  const { setNodeRef, isOver } = useDroppable({
    id: `${GROUP_DROP_PREFIX}${group.id}`,
    disabled: !dropEnabled,
  });

  useEffect(() => {
    if (editing && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [editing]);

  // Reset the local draft whenever the underlying group name changes from
  // outside the chip (e.g. another renamer or a restore).
  useEffect(() => {
    if (!editing) setDraft(group.name);
  }, [group.name, editing]);

  const commit = () => {
    setEditing(false);
    const trimmed = draft.trim();
    if (trimmed && trimmed !== group.name) onRename(trimmed);
    else setDraft(group.name);
  };

  const dotColor = anyError
    ? "var(--danger)"
    : anyRunning
      ? "var(--accent)"
      : "var(--text-dimmer)";

  const highlight = dropEnabled && isOver;

  return (
    <div
      ref={setNodeRef}
      onClick={editing ? undefined : onSelect}
      onDoubleClick={() => setEditing(true)}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        padding: "0 6px 0 10px",
        height: 26,
        cursor: editing ? "text" : "pointer",
        borderRight: "1px solid var(--line)",
        background: highlight
          ? "var(--row-active)"
          : active
            ? "var(--chrome)"
            : "transparent",
        outline: highlight ? "1px dashed var(--accent)" : "none",
        outlineOffset: -1,
        color: active ? "var(--text)" : "var(--text-dim)",
        fontFamily: "var(--mono)",
        fontSize: 11,
        maxWidth: 220,
        minWidth: 0,
        borderBottom:
          "2px solid " + (active ? "var(--accent)" : "transparent"),
        marginBottom: -1,
        transition: "background 80ms linear",
      }}
      title={
        editing
          ? undefined
          : `${group.name} (${paneCount}) — double-click to rename`
      }
    >
      <span
        style={{
          width: 6,
          height: 6,
          borderRadius: "50%",
          background: dotColor,
          flexShrink: 0,
        }}
      />
      {editing ? (
        <input
          ref={inputRef}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit();
            else if (e.key === "Escape") {
              setDraft(group.name);
              setEditing(false);
            }
          }}
          onClick={(e) => e.stopPropagation()}
          style={{
            background: "var(--surface)",
            border: "1px solid var(--line)",
            color: "var(--text)",
            fontFamily: "var(--mono)",
            fontSize: 11,
            padding: "1px 4px",
            width: 110,
            outline: "none",
          }}
        />
      ) : (
        <span
          style={{
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            minWidth: 0,
          }}
        >
          {group.name}
        </span>
      )}
      <span
        style={{
          fontSize: 9,
          fontWeight: 600,
          letterSpacing: 0.3,
          color: active ? "var(--accent)" : "var(--text-dimmer)",
          background: "var(--row-active)",
          border: "1px solid var(--line)",
          padding: "1px 5px",
          borderRadius: 3,
          flexShrink: 0,
        }}
      >
        {paneCount}
      </span>
      {!editing && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            setEditing(true);
          }}
          title="Rename group"
          aria-label={`Rename group ${group.name}`}
          style={tabActionBtn()}
        >
          <PencilIcon />
        </button>
      )}
      {canClose && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onClose();
          }}
          title="Close group (kills its terminals)"
          aria-label={`Close group ${group.name}`}
          style={tabActionBtn()}
        >
          ×
        </button>
      )}
    </div>
  );
}

// Compact ghost rendered in the DragOverlay. Mirrors the TabChip look so
// the dragged tab keeps following the cursor visibly when it crosses out
// of the SortableContext (e.g. onto a group chip). Stays inside the
// shared DndContext so dnd-kit positions it correctly.
function TabDragGhost({ pane }: { pane: Pane }) {
  const dot = statusDot(pane);
  return (
    <div
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        padding: "0 10px",
        height: 26,
        background: "var(--chrome)",
        border: "1px solid var(--line)",
        borderRadius: 3,
        color: "var(--text)",
        fontFamily: "var(--mono)",
        fontSize: 11,
        boxShadow: "0 4px 14px rgba(0,0,0,0.3)",
        opacity: 0.95,
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
          maxWidth: 200,
        }}
      >
        {pane.kind === "claude-session" ? "⎔ " : ""}
        {pane.title}
      </span>
    </div>
  );
}

function NewGroupIcon() {
  return (
    <svg
      width="10"
      height="10"
      viewBox="0 0 10 10"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
    >
      <line x1="5" y1="1.5" x2="5" y2="8.5" />
      <line x1="1.5" y1="5" x2="8.5" y2="5" />
    </svg>
  );
}

function PencilIcon() {
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
      <path d="M11 2.5l2.5 2.5" />
      <path d="M3 13l1-3 7.5-7.5 2.5 2.5L6.5 12.5l-3 1z" />
    </svg>
  );
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
