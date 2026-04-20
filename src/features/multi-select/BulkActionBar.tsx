import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Icon } from "../../components/Icon";
import { useUiStore } from "../../state/store";
import {
  archiveProject,
  addTag,
  collectionsAddProject,
  collectionsRemoveProject,
  listCollections,
  pinProject,
  projectsMoveToTrash,
} from "../../ipc";
import type { Collection, Project } from "../../types";

// Floating bulk-action toolbar.
type OpenMenu = null | "tag" | "add-collection" | "trash";

export function BulkActionBar({ projects }: { projects: Project[] }) {
  const multiSelect = useUiStore((s) => s.multiSelect);
  const clearMultiSelect = useUiStore((s) => s.clearMultiSelect);
  const pushToast = useUiStore((s) => s.pushToast);
  const collection = useUiStore((s) => s.collection);
  const queryClient = useQueryClient();

  const [openMenu, setOpenMenu] = useState<OpenMenu>(null);

  const pinRef = useRef<HTMLButtonElement | null>(null);
  const tagRef = useRef<HTMLButtonElement | null>(null);
  const addCollectionRef = useRef<HTMLButtonElement | null>(null);
  const removeCollectionRef = useRef<HTMLButtonElement | null>(null);
  const archiveRef = useRef<HTMLButtonElement | null>(null);
  const trashRef = useRef<HTMLButtonElement | null>(null);

  const selectedProjects = useMemo(
    () => projects.filter((p) => multiSelect.ids.includes(p.id)),
    [projects, multiSelect.ids],
  );
  const count = selectedProjects.length;
  const allPinned = count > 0 && selectedProjects.every((p) => p.pinned);
  const anyArchived = selectedProjects.some((p) => p.archived);
  const onCollectionView =
    collection !== "all" &&
    collection !== "pinned" &&
    collection !== "archive";

  // Close popovers whenever selection changes so a stale menu can't
  useEffect(() => {
    setOpenMenu(null);
  }, [multiSelect.ids.length]);

  const invalidateAll = () => {
    queryClient.invalidateQueries({ queryKey: ["projects"] });
    queryClient.invalidateQueries({ queryKey: ["collections"] });
    queryClient.invalidateQueries({ queryKey: ["tags"] });
  };

  const runBulk = async <T,>(
    label: string,
    fn: (p: Project) => Promise<T>,
  ) => {
    const results = await Promise.allSettled(selectedProjects.map(fn));
    const failed = results.filter((r) => r.status === "rejected").length;
    const ok = results.length - failed;
    invalidateAll();
    if (failed === 0) {
      pushToast("success", `${label}: ${ok} project${ok === 1 ? "" : "s"}`);
      clearMultiSelect();
    } else {
      pushToast(
        "warn",
        `${label}: ${ok} ok, ${failed} failed (selection kept)`,
      );
    }
  };

  const pinMut = useMutation({
    mutationFn: () =>
      runBulk(allPinned ? "Unpinned" : "Pinned", (p) =>
        pinProject(p.id, !allPinned),
      ),
  });
  const archiveMut = useMutation({
    mutationFn: () =>
      runBulk(anyArchived ? "Unarchived" : "Archived", (p) =>
        archiveProject(p.id, !anyArchived),
      ),
  });
  const trashMut = useMutation({
    mutationFn: () =>
      runBulk("Moved to Trash", (p) => projectsMoveToTrash(p.id)),
  });
  const removeFromCollectionMut = useMutation({
    mutationFn: () => {
      if (!onCollectionView) return Promise.resolve();
      return runBulk("Removed from collection", (p) =>
        collectionsRemoveProject(p.id, collection),
      );
    },
  });

  if (!multiSelect.active) return null;

  const toggleMenu = (next: OpenMenu) =>
    setOpenMenu((prev) => (prev === next ? null : next));

  return (
    <>
      <div
        role="toolbar"
        aria-label="Bulk actions"
        style={{
          position: "fixed",
          bottom: 20,
          left: "50%",
          transform: "translateX(-50%)",
          zIndex: 200,
          padding: "8px 10px",
          background: "var(--palette-bg)",
          border: "1px solid var(--line)",
          borderRadius: 10,
          boxShadow: "0 20px 50px rgba(0,0,0,0.5)",
          backdropFilter: "blur(20px) saturate(180%)",
          WebkitBackdropFilter: "blur(20px) saturate(180%)",
          display: "flex",
          alignItems: "center",
          gap: 6,
          fontFamily: "var(--sans)",
          color: "var(--text)",
          whiteSpace: "nowrap",
        }}
      >
        <span
          style={{
            padding: "0 10px",
            fontSize: 12,
            fontFamily: "var(--mono)",
            color: "var(--text-dim)",
            borderRight: "1px solid var(--line)",
          }}
        >
          {count} selected
        </span>

        <BulkButton
          refEl={pinRef}
          icon={allPinned ? "pin-fill" : "pin"}
          label={allPinned ? "Unpin" : "Pin"}
          disabled={count === 0 || pinMut.isPending}
          onClick={() => {
            setOpenMenu(null);
            pinMut.mutate();
          }}
        />
        <BulkButton
          refEl={tagRef}
          icon="tag"
          label="Tag"
          disabled={count === 0}
          active={openMenu === "tag"}
          onClick={() => toggleMenu("tag")}
        />
        <BulkButton
          refEl={addCollectionRef}
          icon="folder"
          label="Add to collection"
          disabled={count === 0}
          active={openMenu === "add-collection"}
          onClick={() => toggleMenu("add-collection")}
        />
        {onCollectionView && (
          <BulkButton
            refEl={removeCollectionRef}
            icon="archive"
            label="Remove from collection"
            disabled={count === 0 || removeFromCollectionMut.isPending}
            onClick={() => {
              setOpenMenu(null);
              removeFromCollectionMut.mutate();
            }}
          />
        )}
        <BulkButton
          refEl={archiveRef}
          icon="archive"
          label={anyArchived ? "Unarchive" : "Archive"}
          disabled={count === 0 || archiveMut.isPending}
          onClick={() => {
            setOpenMenu(null);
            archiveMut.mutate();
          }}
        />
        <BulkButton
          refEl={trashRef}
          icon="trash"
          label="Trash"
          danger
          disabled={count === 0 || trashMut.isPending}
          active={openMenu === "trash"}
          onClick={() => toggleMenu("trash")}
        />

        <span style={{ width: 1, background: "var(--line)", height: 20 }} />
        <button
          type="button"
          onClick={clearMultiSelect}
          title="Cancel (Esc)"
          style={{
            padding: "6px 12px",
            height: 28,
            fontSize: 12,
            borderRadius: 6,
            border: "1px solid var(--line)",
            background: "var(--surface-2)",
            color: "var(--text-dim)",
            cursor: "pointer",
            fontFamily: "var(--sans)",
          }}
        >
          Cancel
        </button>
      </div>

      {openMenu === "tag" && (
        <AnchoredPopover anchor={tagRef} onClose={() => setOpenMenu(null)}>
          <BulkTagPicker
            projects={selectedProjects}
            onDone={(applied) => {
              setOpenMenu(null);
              if (applied) clearMultiSelect();
            }}
          />
        </AnchoredPopover>
      )}

      {openMenu === "add-collection" && (
        <AnchoredPopover
          anchor={addCollectionRef}
          onClose={() => setOpenMenu(null)}
        >
          <BulkCollectionPicker
            projects={selectedProjects}
            onDone={(applied) => {
              setOpenMenu(null);
              if (applied) clearMultiSelect();
            }}
          />
        </AnchoredPopover>
      )}

      {openMenu === "trash" && (
        <AnchoredPopover anchor={trashRef} onClose={() => setOpenMenu(null)}>
          <TrashConfirmPopover
            count={count}
            onCancel={() => setOpenMenu(null)}
            onConfirm={() => {
              setOpenMenu(null);
              trashMut.mutate();
            }}
          />
        </AnchoredPopover>
      )}
    </>
  );
}

function BulkButton({
  refEl,
  icon,
  label,
  onClick,
  disabled,
  danger,
  active,
}: {
  refEl?: React.Ref<HTMLButtonElement>;
  icon: React.ComponentProps<typeof Icon>["name"];
  label: string;
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
  active?: boolean;
}) {
  return (
    <button
      ref={refEl}
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={label}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        padding: "6px 10px",
        height: 28,
        fontSize: 12,
        borderRadius: 6,
        border: `1px solid ${active ? "var(--accent)" : "var(--line)"}`,
        background: active ? "var(--row-active)" : "var(--surface-2)",
        color: danger ? "var(--danger)" : "var(--text)",
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.5 : 1,
        fontFamily: "var(--sans)",
        whiteSpace: "nowrap",
        lineHeight: 1,
      }}
    >
      <Icon
        name={icon}
        size={12}
        stroke={danger ? "var(--danger)" : "currentColor"}
      />
      {label}
    </button>
  );
}

// Portal-mounted popover anchored above its `anchor` button. Re-measures
function AnchoredPopover({
  anchor,
  onClose,
  children,
}: {
  anchor: React.RefObject<HTMLButtonElement | null>;
  onClose: () => void;
  children: React.ReactNode;
}) {
  const [rect, setRect] = useState<DOMRect | null>(null);

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
      // Clicks inside the anchor toggle close from the toolbar's handler.
      if (anchor.current && anchor.current.contains(t)) return;
      // Click inside the popover itself shouldn't dismiss it.
      const popover = document.getElementById("atlas-bulk-popover");
      if (popover && popover.contains(t)) return;
      onClose();
    };
    // `setTimeout` so the click that opened the popover doesn't also close it.
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

  // Default width budget for popover content. Center over the anchor, clamp
  const POPOVER_MIN = 240;
  const GAP = 8;
  const left = Math.max(
    8,
    Math.min(
      window.innerWidth - POPOVER_MIN - 8,
      rect.left + rect.width / 2 - POPOVER_MIN / 2,
    ),
  );
  const bottom = window.innerHeight - rect.top + GAP;

  return createPortal(
    <div
      id="atlas-bulk-popover"
      onClick={(e) => e.stopPropagation()}
      style={{
        position: "fixed",
        left,
        bottom,
        zIndex: 250,
        minWidth: POPOVER_MIN,
      }}
    >
      {children}
    </div>,
    document.body,
  );
}

function BulkTagPicker({
  projects,
  onDone,
}: {
  projects: Project[];
  onDone: (applied: boolean) => void;
}) {
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();
  const [value, setValue] = useState("");

  const mut = useMutation({
    mutationFn: async (tag: string) => {
      const results = await Promise.allSettled(
        projects.map((p) => addTag(p.id, tag)),
      );
      return {
        tag,
        ok: results.filter((r) => r.status === "fulfilled").length,
        failed: results.filter((r) => r.status === "rejected").length,
      };
    },
    onSuccess: ({ tag, ok, failed }) => {
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      queryClient.invalidateQueries({ queryKey: ["tags"] });
      if (failed === 0) {
        pushToast("success", `Tagged "${tag}" on ${ok}`);
        onDone(true);
      } else {
        pushToast("warn", `Tagged "${tag}": ${ok} ok, ${failed} failed`);
        onDone(false);
      }
    },
    onError: (err) => pushToast("error", `Tag failed: ${String(err)}`),
  });

  return (
    <div
      style={{
        padding: 6,
        background: "var(--palette-bg)",
        border: "1px solid var(--line)",
        borderRadius: 6,
        boxShadow: "0 20px 50px rgba(0,0,0,0.5)",
        backdropFilter: "blur(20px) saturate(180%)",
        WebkitBackdropFilter: "blur(20px) saturate(180%)",
      }}
    >
      <input
        autoFocus
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder="new-tag"
        onKeyDown={(e) => {
          if (e.key === "Enter" && value.trim()) mut.mutate(value.trim());
          if (e.key === "Escape") onDone(false);
        }}
        style={{
          width: "100%",
          padding: "6px 8px",
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
  );
}

function BulkCollectionPicker({
  projects,
  onDone,
}: {
  projects: Project[];
  onDone: (applied: boolean) => void;
}) {
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();
  const { data: collections = [] } = useQuery<Collection[]>({
    queryKey: ["collections"],
    queryFn: listCollections,
    staleTime: 30_000,
  });

  const mut = useMutation({
    mutationFn: async (collectionId: string) => {
      const results = await Promise.allSettled(
        projects.map((p) => collectionsAddProject(p.id, collectionId)),
      );
      return {
        label:
          collections.find((c) => c.id === collectionId)?.label ??
          "collection",
        ok: results.filter((r) => r.status === "fulfilled").length,
        failed: results.filter((r) => r.status === "rejected").length,
      };
    },
    onSuccess: ({ label, ok, failed }) => {
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      queryClient.invalidateQueries({ queryKey: ["collections"] });
      if (failed === 0) {
        pushToast("success", `Added ${ok} to ${label}`);
        onDone(true);
      } else {
        pushToast("warn", `${label}: ${ok} ok, ${failed} failed`);
        onDone(false);
      }
    },
    onError: (err) => pushToast("error", `Add failed: ${String(err)}`),
  });

  return (
    <div
      style={{
        padding: 4,
        background: "var(--palette-bg)",
        border: "1px solid var(--line)",
        borderRadius: 6,
        maxHeight: 260,
        overflowY: "auto",
        boxShadow: "0 20px 50px rgba(0,0,0,0.5)",
        backdropFilter: "blur(20px) saturate(180%)",
        WebkitBackdropFilter: "blur(20px) saturate(180%)",
      }}
    >
      {collections.length === 0 ? (
        <div
          style={{
            padding: "6px 8px",
            fontSize: 11,
            color: "var(--text-dim)",
            fontFamily: "var(--mono)",
          }}
        >
          no collections
        </div>
      ) : (
        collections.map((c) => (
          <div
            key={c.id}
            onClick={() => mut.mutate(c.id)}
            onMouseEnter={(e) =>
              (e.currentTarget.style.background = "var(--row-active)")
            }
            onMouseLeave={(e) =>
              (e.currentTarget.style.background = "transparent")
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
            }}
          >
            <span
              style={{
                width: 10,
                height: 10,
                borderRadius: 2,
                background: c.dot,
                flexShrink: 0,
              }}
            />
            <span style={{ flex: 1 }}>{c.label}</span>
          </div>
        ))
      )}
    </div>
  );
}

function TrashConfirmPopover({
  count,
  onCancel,
  onConfirm,
}: {
  count: number;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div
      style={{
        padding: 10,
        background: "var(--palette-bg)",
        border: "1px solid var(--line)",
        borderRadius: 6,
        display: "flex",
        flexDirection: "column",
        gap: 8,
        boxShadow: "0 20px 50px rgba(0,0,0,0.5)",
      }}
    >
      <div style={{ fontSize: 12, color: "var(--text)" }}>
        Move {count} project{count === 1 ? "" : "s"} to Trash?
      </div>
      <div style={{ fontSize: 11, color: "var(--text-dim)" }}>
        Folders move to the system trash. The index rows are removed.
      </div>
      <div style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
        <button
          type="button"
          onClick={onCancel}
          style={{
            padding: "4px 10px",
            fontSize: 12,
            borderRadius: 4,
            border: "1px solid var(--line)",
            background: "var(--surface-2)",
            color: "var(--text)",
            cursor: "pointer",
          }}
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={onConfirm}
          style={{
            padding: "4px 10px",
            fontSize: 12,
            borderRadius: 4,
            border: "1px solid var(--danger)",
            background: "var(--danger)",
            color: "var(--accent-fg, #fff)",
            cursor: "pointer",
          }}
        >
          Move to Trash
        </button>
      </div>
    </div>
  );
}
