import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
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
  arrayMove,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { Icon, LangDot } from "./Icon";
import { useUiStore } from "../state/store";
import {
  collectionsCreate,
  collectionsDelete,
  collectionsRename,
  collectionsReorder,
  collectionsUpdateColor,
  listCollections,
  listTags,
  projectsReorderPinned,
  systemDiskUsage,
} from "../ipc";
import type { SystemDiskUsage } from "../ipc";
import type { Project, Collection } from "../types";

// Atlas - sidebar.

interface SidebarProps {
  projects: Project[];
}

// 8-color palette shown in the color-dot picker. Kept in sync with the
export const COLLECTION_PALETTE = [
  "#7c7fee",
  "#d97757",
  "#78c98a",
  "#c77eff",
  "#3178C6",
  "#E0763C",
  "#ef4444",
  "#888888",
];

export function Sidebar({ projects }: SidebarProps) {
  const collection = useUiStore((s) => s.collection);
  const setCollection = useUiStore((s) => s.setCollection);
  const selectedProjectId = useUiStore((s) => s.selectedProjectId);
  const setSelectedProjectId = useUiStore((s) => s.setSelectedProjectId);
  const setCollectionsStore = useUiStore((s) => s.setCollections);
  const openSettings = useUiStore((s) => s.openSettings);
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();

  // U9 - local UI state: new-collection form, per-row menu, inline rename.
  const [creating, setCreating] = useState(false);
  const [rowMenu, setRowMenu] = useState<
    null | { x: number; y: number; collectionId: string }
  >(null);
  const [renamingId, setRenamingId] = useState<string | null>(null);

  // Pointer sensor with an 8px activation distance so plain clicks still
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
  );

  // Live collections from Rust. Tolerate P2 not-yet-shipping: `retry: false`
  const { data: collections = [] } = useQuery<Collection[]>({
    queryKey: ["collections"],
    queryFn: async () => {
      const list = await listCollections();
      setCollectionsStore(list);
      return list;
    },
    retry: false,
  });

  const pinned = useMemo(() => projects.filter((p) => p.pinned), [projects]);
  const archivedCount = useMemo(
    () => projects.filter((p) => p.archived).length,
    [projects],
  );
  const nonArchivedCount = useMemo(
    () => projects.filter((p) => !p.archived).length,
    [projects],
  );

  // Per-collection count derived from project membership - cheap, linear.
  const collectionCounts = useMemo(() => {
    const acc = new Map<string, number>();
    for (const p of projects) {
      if (p.archived) continue;
      for (const cid of p.collectionIds) {
        acc.set(cid, (acc.get(cid) ?? 0) + 1);
      }
    }
    return acc;
  }, [projects]);

  const activeRowMenuCollection = useMemo(
    () => collections.find((c) => c.id === rowMenu?.collectionId) ?? null,
    [collections, rowMenu?.collectionId],
  );

  return (
    <div className="border-r border-line bg-chrome overflow-y-auto flex flex-col">
      <SectionHeader
        title="Library"
        right={
          <button
            onClick={() => openSettings("watchers")}
            title="Manage folder watchers"
            aria-label="Manage folder watchers"
            className="inline-flex items-center gap-1 px-[6px] h-[18px] rounded-[4px] border border-line bg-surface-2 text-text-dim hover:text-text font-mono text-[10px]"
          >
            <Icon name="folder" size={9} />
            Watchers
          </button>
        }
      />
      <NavItem
        label="All Projects"
        icon={<Icon name="folder" size={13} />}
        count={nonArchivedCount}
        active={collection === "all"}
        onClick={() => setCollection("all")}
      />
      <NavItem
        label="Pinned"
        icon={<Icon name="pin" size={13} />}
        count={pinned.length}
        active={collection === "pinned"}
        onClick={() => setCollection("pinned")}
      />
      <NavItem
        label="Archive"
        icon={<Icon name="archive" size={13} />}
        count={archivedCount}
        active={collection === "archive"}
        onClick={() => setCollection("archive")}
      />

      <SectionHeader
        title="Collections"
        right={
          <button
            type="button"
            onClick={() => setCreating((v) => !v)}
            aria-label="New collection"
            title="New collection"
            className="inline-flex items-center justify-center w-[18px] h-[18px] rounded-[4px] border border-line bg-surface-2 text-text-dim hover:text-text"
          >
            <Icon name="plus" size={10} />
          </button>
        }
      />

      {creating && (
        <NewCollectionForm
          onCancel={() => setCreating(false)}
          onCreated={() => setCreating(false)}
        />
      )}

      {collections.length === 0 && !creating && (
        <div className="mx-[6px] px-[10px] py-2 font-mono text-[11px] text-text-dimmer">
          no collections
        </div>
      )}

      {collections.length > 0 && (
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragEnd={(e: DragEndEvent) => {
            const { active, over } = e;
            if (!over || active.id === over.id) return;

            const oldIdx = collections.findIndex((c) => c.id === active.id);
            const newIdx = collections.findIndex((c) => c.id === over.id);
            if (oldIdx < 0 || newIdx < 0) return;

            const reordered = arrayMove(collections, oldIdx, newIdx);

            // Optimistic update - keep the sidebar visually stable while
            queryClient.setQueryData<Collection[]>(["collections"], reordered);

            const orderedIds = reordered.map((c) => c.id);
            collectionsReorder(orderedIds)
              .catch((err) => {
                pushToast(
                  "warn",
                  `Collection order not saved: ${String(err)}`,
                );
              })
              .finally(() => {
                queryClient.invalidateQueries({ queryKey: ["collections"] });
              });
          }}
        >
          <SortableContext
            items={collections.map((c) => c.id)}
            strategy={verticalListSortingStrategy}
          >
            {collections.map((c) => {
              const active = collection === c.id;
              const count = collectionCounts.get(c.id) ?? 0;
              return (
                <SortableCollectionRow
                  key={c.id}
                  collection={c}
                  active={active}
                  count={count}
                  renaming={renamingId === c.id}
                  onSelect={() => setCollection(c.id)}
                  onContextMenu={(ev) => {
                    ev.preventDefault();
                    setRowMenu({
                      x: ev.clientX,
                      y: ev.clientY,
                      collectionId: c.id,
                    });
                  }}
                  onRenameDone={() => setRenamingId(null)}
                />
              );
            })}
          </SortableContext>
        </DndContext>
      )}

      <TagsSection projects={projects} />

      <SectionHeader title="Pinned" />
      {pinned.length === 0 && (
        <div className="mx-[6px] px-[10px] py-2 font-mono text-[11px] text-text-dimmer">
          nothing pinned
        </div>
      )}
      {pinned.length > 0 && (
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragEnd={(e: DragEndEvent) => {
            const { active, over } = e;
            if (!over || active.id === over.id) return;

            const oldIdx = pinned.findIndex((p) => p.id === active.id);
            const newIdx = pinned.findIndex((p) => p.id === over.id);
            if (oldIdx < 0 || newIdx < 0) return;

            const reordered = arrayMove(pinned, oldIdx, newIdx);

            // Optimistic update - splice the pinned portion of the cache
            queryClient.setQueryData<Project[]>(["projects"], (prev) => {
              if (!prev) return prev;
              const byId = new Map(reordered.map((p) => [p.id, p]));
              // Walk the old array and emit pinned rows in the new order
              let pinnedCursor = 0;
              return prev.map((p) => {
                if (!p.pinned) return p;
                const next = reordered[pinnedCursor++];
                return byId.get(next.id) ?? p;
              });
            });

            const orderedIds = reordered.map((p) => p.id);
            projectsReorderPinned(orderedIds)
              .catch((err) => {
                // D7 may not have registered the Rust command yet  -
                pushToast("warn", `Pin order not saved: ${String(err)}`);
              })
              .finally(() => {
                queryClient.invalidateQueries({ queryKey: ["projects"] });
              });
          }}
        >
          <SortableContext
            items={pinned.map((p) => p.id)}
            strategy={verticalListSortingStrategy}
          >
            {pinned.map((p) => (
              <SortablePinnedRow
                key={p.id}
                project={p}
                selected={selectedProjectId === p.id}
                onSelect={() => {
                  setSelectedProjectId(p.id);
                  setCollection("pinned");
                }}
              />
            ))}
          </SortableContext>
        </DndContext>
      )}

      <div className="flex-1" />

      {/* Home-volume disk bar. Renders nothing if `system_disk_usage`
          is unavailable. */}
      <DiskBar />

      {/* Collection row context menu. Rendered via portal so it
          escapes the sidebar's scroll container. */}
      {rowMenu && activeRowMenuCollection && (
        <CollectionContextMenu
          x={rowMenu.x}
          y={rowMenu.y}
          collection={activeRowMenuCollection}
          onClose={() => setRowMenu(null)}
          onRenameRequest={() => {
            setRenamingId(activeRowMenuCollection.id);
            setRowMenu(null);
          }}
        />
      )}
    </div>
  );
}

function SectionHeader({
  title,
  right,
}: {
  title: string;
  right?: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-[6px] px-[14px] pt-[10px] pb-1 font-mono text-[10px] text-text-dim uppercase tracking-[0.6px]">
      <span className="flex-1">{title}</span>
      {right}
    </div>
  );
}

// U9 - inline form shown directly under the Collections header. Tiny label
function NewCollectionForm({
  onCancel,
  onCreated,
}: {
  onCancel: () => void;
  onCreated: () => void;
}) {
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();
  const [label, setLabel] = useState("");
  const [color, setColor] = useState<string>(COLLECTION_PALETTE[0]);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const mut = useMutation({
    mutationFn: () => collectionsCreate(label.trim(), color),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["collections"] });
      pushToast("success", `Collection "${label.trim()}" created`);
      onCreated();
    },
    onError: (err) => {
      pushToast(
        "warn",
        `Create collection failed: ${String(err)}`,
      );
    },
  });

  const submit = () => {
    if (!label.trim()) return;
    mut.mutate();
  };

  return (
    <div
      className="mx-[6px] mb-1 p-2 rounded-[5px] border border-line bg-surface-2"
      onClick={(e) => e.stopPropagation()}
    >
      <input
        ref={inputRef}
        value={label}
        onChange={(e) => setLabel(e.target.value)}
        placeholder="collection name"
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            submit();
          }
          if (e.key === "Escape") {
            e.preventDefault();
            onCancel();
          }
        }}
        style={{
          width: "100%",
          padding: "4px 6px",
          fontSize: 12,
          background: "var(--bg)",
          border: "1px solid var(--line)",
          borderRadius: 3,
          color: "var(--text)",
          outline: "none",
          fontFamily: "var(--mono)",
        }}
      />
      {/* Palette on its own row — on a narrow sidebar the 8 swatches
          plus Cancel + Create collided and the Cancel label clipped
          against the edge. Stacking keeps everything legible at any
          sidebar width. */}
      <div className="flex items-center gap-[6px] mt-2 flex-wrap">
        {COLLECTION_PALETTE.map((c) => (
          <button
            type="button"
            key={c}
            onClick={() => setColor(c)}
            aria-label={`Color ${c}`}
            className="w-[14px] h-[14px] rounded-full shrink-0"
            style={{
              background: c,
              border:
                color === c
                  ? "2px solid var(--text)"
                  : "2px solid transparent",
              boxShadow: color === c ? "0 0 0 1px var(--line)" : "none",
              cursor: "pointer",
              padding: 0,
            }}
          />
        ))}
      </div>
      <div className="flex items-center justify-end gap-[6px] mt-2">
        <button
          type="button"
          onClick={onCancel}
          style={{
            padding: "2px 10px",
            height: 22,
            fontSize: 11,
            background: "transparent",
            border: "1px solid var(--line)",
            borderRadius: 4,
            color: "var(--text)",
            cursor: "pointer",
            fontFamily: "var(--sans)",
            flexShrink: 0,
          }}
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={submit}
          disabled={!label.trim() || mut.isPending}
          style={{
            padding: "2px 10px",
            height: 22,
            fontSize: 11,
            background: "var(--accent)",
            border: "none",
            borderRadius: 4,
            color: "var(--accent-fg, white)",
            cursor: label.trim() ? "pointer" : "not-allowed",
            opacity: label.trim() ? 1 : 0.5,
            fontFamily: "var(--sans)",
            fontWeight: 600,
            flexShrink: 0,
          }}
        >
          Create
        </button>
      </div>
    </div>
  );
}

// U9 - a draggable + right-clickable collection row. Mirrors the
function SortableCollectionRow({
  collection,
  active,
  count,
  renaming,
  onSelect,
  onContextMenu,
  onRenameDone,
}: {
  collection: Collection;
  active: boolean;
  count: number;
  renaming: boolean;
  onSelect: () => void;
  onContextMenu: (e: React.MouseEvent) => void;
  onRenameDone: () => void;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: collection.id });

  return (
    <div
      ref={setNodeRef}
      {...attributes}
      {...(renaming ? {} : listeners)}
      onClick={renaming ? undefined : onSelect}
      onContextMenu={onContextMenu}
      className="flex items-center gap-2 h-[var(--sidebar-row-h,28px)] px-[10px] mx-[6px] rounded-[5px] cursor-grab border-t-2 border-transparent touch-none"
      style={{
        background: active ? "var(--row-active)" : "transparent",
        color: active ? "var(--text)" : "var(--text-dim)",
        transform: CSS.Transform.toString(transform),
        transition,
        opacity: isDragging ? 0.6 : 1,
        zIndex: isDragging ? 10 : undefined,
      }}
    >
      <span
        className="w-[10px] h-[10px] rounded-[2px] shrink-0"
        style={{ background: collection.dot }}
      />
      {renaming ? (
        <InlineRenameInput
          initial={collection.label}
          collectionId={collection.id}
          onDone={onRenameDone}
        />
      ) : (
        <>
          <span className="flex-1 text-xs truncate">{collection.label}</span>
          <span className="font-mono text-[10px] text-text-dimmer">
            {count}
          </span>
        </>
      )}
    </div>
  );
}

function InlineRenameInput({
  initial,
  collectionId,
  onDone,
}: {
  initial: string;
  collectionId: string;
  onDone: () => void;
}) {
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();
  const [value, setValue] = useState(initial);
  const ref = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);

  const mut = useMutation({
    mutationFn: (label: string) => collectionsRename(collectionId, label),
    onSuccess: (_d, label) => {
      queryClient.invalidateQueries({ queryKey: ["collections"] });
      pushToast("success", `Renamed to "${label}"`);
      onDone();
    },
    onError: (err) => {
      pushToast("warn", `Rename failed: ${String(err)}`);
      onDone();
    },
  });

  const commit = () => {
    const next = value.trim();
    if (!next || next === initial) {
      onDone();
      return;
    }
    mut.mutate(next);
  };

  return (
    <input
      ref={ref}
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onClick={(e) => e.stopPropagation()}
      onBlur={commit}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") {
          e.preventDefault();
          commit();
        }
        if (e.key === "Escape") {
          e.preventDefault();
          onDone();
        }
      }}
      style={{
        flex: 1,
        minWidth: 0,
        padding: "2px 4px",
        fontSize: 12,
        background: "var(--bg)",
        border: "1px solid var(--line)",
        borderRadius: 3,
        color: "var(--text)",
        outline: "none",
        fontFamily: "var(--sans)",
      }}
    />
  );
}

// U9 - a purpose-built small context menu for collection rows. We do NOT
function CollectionContextMenu({
  x,
  y,
  collection,
  onClose,
  onRenameRequest,
}: {
  x: number;
  y: number;
  collection: Collection;
  onClose: () => void;
  onRenameRequest: () => void;
}) {
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();
  const [colorOpen, setColorOpen] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);

  useEffect(() => {
    const onDocClick = () => onClose();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    const id = window.setTimeout(() => {
      window.addEventListener("click", onDocClick);
      window.addEventListener("keydown", onKey);
    }, 0);
    return () => {
      window.clearTimeout(id);
      window.removeEventListener("click", onDocClick);
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  const colorMut = useMutation({
    mutationFn: (color: string) =>
      collectionsUpdateColor(collection.id, color),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["collections"] });
      pushToast("success", "Color updated");
    },
    onError: (err) => pushToast("warn", `Color update failed: ${String(err)}`),
  });

  const deleteMut = useMutation({
    mutationFn: () => collectionsDelete(collection.id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["collections"] });
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      pushToast("success", `Deleted "${collection.label}"`);
    },
    onError: (err) => pushToast("warn", `Delete failed: ${String(err)}`),
  });

  const MENU_W = 200;
  const MENU_H = 200;
  const pad = 8;
  const left = Math.min(x, window.innerWidth - MENU_W - pad);
  const top = Math.min(y, window.innerHeight - MENU_H - pad);

  return createPortal(
    <div
      onClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
      role="menu"
      aria-label="Collection actions"
      style={{
        position: "fixed",
        top,
        left,
        zIndex: 300,
        minWidth: 180,
        maxWidth: MENU_W,
        padding: 4,
        background: "var(--palette-bg)",
        border: "1px solid var(--line)",
        borderRadius: 8,
        backdropFilter: "blur(20px) saturate(180%)",
        WebkitBackdropFilter: "blur(20px) saturate(180%)",
        boxShadow: "0 20px 50px rgba(0,0,0,0.5)",
        fontFamily: "var(--sans)",
        color: "var(--text)",
      }}
    >
      <MenuRow
        icon="note"
        label="Rename"
        onClick={() => {
          onRenameRequest();
        }}
      />
      <MenuRow
        icon="dot"
        label="Change color"
        trailing="›"
        onClick={() => setColorOpen((v) => !v)}
      />
      {colorOpen && (
        <div
          className="flex items-center gap-[6px]"
          style={{
            padding: 6,
            marginTop: 2,
            background: "var(--surface-2)",
            border: "1px solid var(--line)",
            borderRadius: 5,
          }}
        >
          {COLLECTION_PALETTE.map((c) => (
            <button
              key={c}
              type="button"
              onClick={() => {
                colorMut.mutate(c);
                onClose();
              }}
              aria-label={`Set color ${c}`}
              className="w-[14px] h-[14px] rounded-full shrink-0"
              style={{
                background: c,
                border:
                  collection.dot === c
                    ? "2px solid var(--text)"
                    : "2px solid transparent",
                cursor: "pointer",
                padding: 0,
              }}
            />
          ))}
        </div>
      )}
      <div
        role="separator"
        style={{ height: 1, background: "var(--line)", margin: "4px 0" }}
      />
      <MenuRow
        icon="trash"
        label="Delete…"
        danger
        onClick={() => setConfirmOpen(true)}
      />

      {confirmOpen && (
        <div
          style={{
            marginTop: 6,
            padding: 8,
            background: "var(--surface-2)",
            border: "1px solid var(--line)",
            borderRadius: 5,
          }}
        >
          <div
            style={{
              fontSize: 11,
              color: "var(--text-dim)",
              marginBottom: 8,
              lineHeight: 1.45,
            }}
          >
            Delete "{collection.label}"? Projects keep their files — only the
            collection membership is removed.
          </div>
          <div style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
            <button
              onClick={() => setConfirmOpen(false)}
              style={{
                padding: "4px 10px",
                fontSize: 11,
                height: 24,
                background: "transparent",
                border: "1px solid var(--line)",
                borderRadius: 4,
                color: "var(--text)",
                cursor: "pointer",
                fontFamily: "var(--sans)",
              }}
            >
              Cancel
            </button>
            <button
              onClick={() => {
                deleteMut.mutate();
                setConfirmOpen(false);
                onClose();
              }}
              style={{
                padding: "4px 10px",
                fontSize: 11,
                height: 24,
                background: "var(--danger)",
                border: "none",
                borderRadius: 4,
                color: "white",
                cursor: "pointer",
                fontFamily: "var(--sans)",
                fontWeight: 600,
              }}
            >
              Delete
            </button>
          </div>
        </div>
      )}
    </div>,
    document.body,
  );
}

function MenuRow({
  icon,
  label,
  danger,
  trailing,
  onClick,
}: {
  icon: React.ComponentProps<typeof Icon>["name"];
  label: string;
  danger?: boolean;
  trailing?: string;
  onClick: () => void;
}) {
  return (
    <div
      onClick={onClick}
      role="menuitem"
      tabIndex={0}
      onMouseEnter={(e) =>
        (e.currentTarget.style.background = "var(--row-active)")
      }
      onMouseLeave={(e) =>
        (e.currentTarget.style.background = "transparent")
      }
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "5px 10px",
        height: 26,
        borderRadius: 5,
        cursor: "pointer",
        fontSize: 12,
        color: danger ? "var(--danger)" : "var(--text)",
      }}
    >
      <Icon name={icon} size={13} stroke="currentColor" />
      <span style={{ flex: 1 }}>{label}</span>
      {trailing && (
        <span
          style={{ color: "var(--text-dim)", fontSize: 11, marginLeft: 4 }}
        >
          {trailing}
        </span>
      )}
    </div>
  );
}

// -  a single draggable pinned row. Uses `useSortable` so dnd-kit
function SortablePinnedRow({
  project,
  selected,
  onSelect,
}: {
  project: Project;
  selected: boolean;
  onSelect: () => void;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: project.id });

  return (
    <div
      ref={setNodeRef}
      {...attributes}
      {...listeners}
      onClick={onSelect}
      className="flex items-center gap-2 h-[var(--sidebar-row-h,28px)] px-[10px] mx-[6px] rounded-[5px] cursor-grab border-t-2 border-transparent touch-none"
      style={{
        background: selected ? "var(--row-active)" : "transparent",
        transform: CSS.Transform.toString(transform),
        transition,
        opacity: isDragging ? 0.6 : 1,
        zIndex: isDragging ? 10 : undefined,
      }}
    >
      <LangDot color={project.color} />
      <span className="flex-1 text-xs text-text truncate">{project.name}</span>
      {project.dirty > 0 && (
        <span className="w-[6px] h-[6px] rounded-full bg-warn" />
      )}
    </div>
  );
}

// Tags filter chips.
function TagsSection({ projects }: { projects: Project[] }) {
  const selectedTag = useUiStore((s) => s.selectedTag);
  const setSelectedTag = useUiStore((s) => s.setSelectedTag);

  // Source the canonical tag set from the DB (projects across every
  const { data: allTags = [] } = useQuery<string[]>({
    queryKey: ["tags"],
    queryFn: listTags,
    staleTime: 30_000,
    retry: false,
  });

  // Count occurrences across every non-archived project - count badges
  const counts = useMemo(() => {
    const m = new Map<string, number>();
    for (const p of projects) {
      if (p.archived) continue;
      for (const t of p.tags) m.set(t, (m.get(t) ?? 0) + 1);
    }
    return m;
  }, [projects]);

  // Sort tags by (currently selected) > (descending count) > alpha. Keeps
  const ordered = useMemo(() => {
    const source = allTags.length
      ? allTags
      : Array.from(new Set(projects.flatMap((p) => p.tags)));
    return [...source].sort((a, b) => {
      if (a === selectedTag) return -1;
      if (b === selectedTag) return 1;
      const ca = counts.get(a) ?? 0;
      const cb = counts.get(b) ?? 0;
      if (ca !== cb) return cb - ca;
      return a.localeCompare(b);
    });
  }, [allTags, counts, projects, selectedTag]);

  const containerRef = useRef<HTMLDivElement | null>(null);
  const [visibleCount, setVisibleCount] = useState(ordered.length);
  const [moreOpen, setMoreOpen] = useState(false);
  const moreBtnRef = useRef<HTMLButtonElement | null>(null);
  const ROW_LIMIT = 3;

  // Reset visibleCount to "show all" whenever the source list or the
  useEffect(() => {
    setVisibleCount(ordered.length);
  }, [ordered]);

  // Measure against the natural flow: render all chips without the
  useEffect(() => {
    const el = containerRef.current;
    if (!el || ordered.length === 0) return;

    const measure = () => {
      const chips = Array.from(el.querySelectorAll<HTMLElement>("[data-chip]"));
      if (chips.length === 0) return;
      const chipH = chips[0].offsetHeight || 16;
      // +4px row gap between rows. The container has no top padding so
      const rowHeight = chipH + 4;
      const maxContent = ROW_LIMIT * rowHeight - 4; // last row has no gap below
      if (el.scrollHeight > maxContent + 2 && visibleCount > 0) {
        setVisibleCount((v) => Math.max(0, v - 1));
      }
    };

    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [ordered, visibleCount]);

  if (ordered.length === 0) return null;

  const hidden = ordered.length - visibleCount;
  const visible = hidden > 0 ? ordered.slice(0, visibleCount) : ordered;

  const toggleTag = (t: string) =>
    setSelectedTag(selectedTag === t ? null : t);

  return (
    <>
      <SectionHeader
        title="Tags"
        right={
          selectedTag ? (
            <button
              type="button"
              onClick={() => setSelectedTag(null)}
              className="font-mono text-[10px] text-text-dim hover:text-text"
              title="Clear tag filter"
            >
              clear
            </button>
          ) : undefined
        }
      />

      <div
        ref={containerRef}
        className="flex flex-wrap gap-[4px] px-[14px] pb-2"
        style={{ maxHeight: 16 * 3 + 4 * 2 + 4, overflow: "hidden" }}
      >
        {visible.map((t) => (
          <TagChip
            key={t}
            label={t}
            count={counts.get(t) ?? 0}
            active={selectedTag === t}
            onClick={() => toggleTag(t)}
          />
        ))}
        {hidden > 0 && (
          <button
            ref={moreBtnRef}
            type="button"
            onClick={() => setMoreOpen((v) => !v)}
            data-chip
            className="inline-flex items-center px-[5px] h-[16px] font-mono text-[9px] border"
            style={{
              borderColor: "var(--line)",
              background: moreOpen ? "var(--row-active)" : "transparent",
              color: "var(--text-dim)",
              cursor: "pointer",
              borderRadius: 2,
            }}
          >
            +{hidden} more
          </button>
        )}
      </div>

      {moreOpen && (
        <TagOverflowMenu
          anchor={moreBtnRef}
          tags={ordered}
          counts={counts}
          selected={selectedTag}
          onPick={(t) => {
            toggleTag(t);
            setMoreOpen(false);
          }}
          onClose={() => setMoreOpen(false)}
        />
      )}
    </>
  );
}

function TagChip({
  label,
  count,
  active,
  onClick,
}: {
  label: string;
  count: number;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      data-chip
      className="inline-flex items-center gap-[3px] px-[5px] h-[16px] font-mono text-[9px] border"
      title={`${count} project${count === 1 ? "" : "s"}`}
      style={{
        borderColor: active ? "var(--accent)" : "var(--line)",
        background: active ? "var(--row-active)" : "transparent",
        color: active ? "var(--text)" : "var(--text-dim)",
        cursor: "pointer",
        whiteSpace: "nowrap",
        borderRadius: 2,
      }}
    >
      <span>#{label}</span>
      {count > 0 && (
        <span
          style={{
            fontSize: 8,
            color: active ? "var(--accent)" : "var(--text-dimmer)",
          }}
        >
          {count}
        </span>
      )}
    </button>
  );
}

function TagOverflowMenu({
  anchor,
  tags,
  counts,
  selected,
  onPick,
  onClose,
}: {
  anchor: React.RefObject<HTMLButtonElement | null>;
  tags: string[];
  counts: Map<string, number>;
  selected: string | null;
  onPick: (t: string) => void;
  onClose: () => void;
}) {
  const [rect, setRect] = useState<DOMRect | null>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    const measure = () => {
      const el = anchor.current;
      setRect(el ? el.getBoundingClientRect() : null);
    };
    measure();
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, true);
    return () => {
      window.removeEventListener("resize", measure);
      window.removeEventListener("scroll", measure, true);
    };
  }, [anchor]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    const onClick = (e: MouseEvent) => {
      const t = e.target as Node;
      if (anchor.current && anchor.current.contains(t)) return;
      const popover = document.getElementById("atlas-tag-overflow");
      if (popover && popover.contains(t)) return;
      onClose();
    };
    const id = window.setTimeout(() => {
      window.addEventListener("mousedown", onClick);
      window.addEventListener("keydown", onKey);
    }, 0);
    return () => {
      window.clearTimeout(id);
      window.removeEventListener("mousedown", onClick);
      window.removeEventListener("keydown", onKey);
    };
  }, [anchor, onClose]);

  if (!rect) return null;

  const WIDTH = 240;
  const GAP = 6;
  const left = Math.max(8, Math.min(window.innerWidth - WIDTH - 8, rect.left));
  const top = rect.bottom + GAP;

  const q = query.trim().toLowerCase();
  const filtered = q ? tags.filter((t) => t.toLowerCase().includes(q)) : tags;

  return createPortal(
    <div
      id="atlas-tag-overflow"
      onClick={(e) => e.stopPropagation()}
      style={{
        position: "fixed",
        left,
        top,
        zIndex: 250,
        width: WIDTH,
        maxHeight: 320,
        display: "flex",
        flexDirection: "column",
        background: "var(--palette-bg)",
        border: "1px solid var(--line)",
        borderRadius: 6,
        boxShadow: "0 20px 50px rgba(0,0,0,0.5)",
        backdropFilter: "blur(20px) saturate(180%)",
        WebkitBackdropFilter: "blur(20px) saturate(180%)",
      }}
    >
      <div style={{ padding: 6, borderBottom: "1px solid var(--line)" }}>
        <input
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="filter tags…"
          style={{
            width: "100%",
            padding: "5px 8px",
            fontSize: 12,
            background: "var(--bg)",
            border: "1px solid var(--line)",
            borderRadius: 4,
            color: "var(--text)",
            outline: "none",
            fontFamily: "var(--mono)",
          }}
        />
      </div>
      <div style={{ overflowY: "auto", padding: 4 }}>
        {filtered.length === 0 ? (
          <div
            style={{
              padding: "6px 8px",
              fontSize: 11,
              color: "var(--text-dim)",
              fontFamily: "var(--mono)",
            }}
          >
            no matches
          </div>
        ) : (
          filtered.map((t) => {
            const active = t === selected;
            return (
              <div
                key={t}
                onClick={() => onPick(t)}
                onMouseEnter={(e) =>
                  (e.currentTarget.style.background = "var(--row-active)")
                }
                onMouseLeave={(e) =>
                  (e.currentTarget.style.background = active
                    ? "var(--row-active)"
                    : "transparent")
                }
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  padding: "4px 8px",
                  height: 24,
                  borderRadius: 4,
                  cursor: "pointer",
                  fontSize: 12,
                  fontFamily: "var(--mono)",
                  color: active ? "var(--text)" : "var(--text-dim)",
                  background: active ? "var(--row-active)" : "transparent",
                }}
              >
                <span style={{ flex: 1 }}>#{t}</span>
                <span
                  style={{
                    fontSize: 10,
                    color: "var(--text-dimmer)",
                  }}
                >
                  {counts.get(t) ?? 0}
                </span>
                {active && (
                  <Icon name="check" size={11} stroke="var(--accent)" />
                )}
              </div>
            );
          })
        )}
      </div>
    </div>,
    document.body,
  );
}

// Pass 3 - bottom-of-sidebar disk bar. Polls `system_disk_usage` once per
function DiskBar() {
  const { data: usage, isError } = useQuery<SystemDiskUsage>({
    queryKey: ["systemDiskUsage"],
    queryFn: systemDiskUsage,
    refetchInterval: 60_000,
    retry: false,
  });

  // Silent no-op when the Rust side isn't registered yet (or the query
  if (isError || !usage) return null;

  // `pctUsed` arrives as 0..1 from Rust; clamp defensively and derive the
  const pct = Math.max(0, Math.min(1, usage.pctUsed));
  const pctLabel = `${Math.round(pct * 100)}%`;

  return (
    <div
      className="px-[14px] py-3 border-t border-line"
      title={`${usage.used} used of ${usage.total} on ${usage.mountPoint}`}
    >
      <div className="flex items-center justify-between mb-[6px]">
        <span className="font-mono text-[10px] text-text-dim uppercase tracking-[0.6px]">
          Disk
        </span>
        <span className="font-mono text-[10px] text-text-dim">{pctLabel}</span>
      </div>
      <div
        className="h-[6px] rounded-[3px] overflow-hidden"
        style={{ background: "var(--line)" }}
        role="progressbar"
        aria-valuenow={Math.round(pct * 100)}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label="Disk usage"
      >
        <div
          className="h-full"
          style={{
            width: `${pct * 100}%`,
            background: "var(--accent)",
            transition: "width 200ms",
          }}
        />
      </div>
      <div className="mt-[6px] font-mono text-[11px] text-text-dim">
        {usage.used} / {usage.total}
      </div>
    </div>
  );
}

function NavItem({
  label,
  icon,
  count,
  active,
  onClick,
}: {
  label: string;
  icon: React.ReactNode;
  count: number;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={`${label} (${count})`}
      aria-pressed={active}
      className="flex items-center gap-2 h-[var(--sidebar-row-h,28px)] px-[10px] mx-[6px] rounded-[5px] cursor-pointer w-auto text-left"
      style={{
        background: active ? "var(--row-active)" : "transparent",
        color: active ? "var(--text)" : "var(--text-dim)",
        border: "none",
      }}
    >
      {icon}
      <span
        className="flex-1 text-xs"
        style={{ fontWeight: active ? 500 : 400 }}
      >
        {label}
      </span>
      <span className="font-mono text-[10px] text-text-dimmer">{count}</span>
    </button>
  );
}
