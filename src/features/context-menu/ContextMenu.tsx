import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Icon, Kbd } from "../../components/Icon";
import {
  archiveProject,
  collectionsAddProject,
  collectionsRemoveProject,
  gitBranchList,
  gitCheckout,
  listCollections,
  openInEditor,
  pinProject,
  projectsMoveToTrash,
  revealInFinder,
  setProjectTags,
  terminalOpen,
} from "../../ipc";
import { useUiStore } from "../../state/store";
import { useTerminalStore, makePane } from "../../features/terminal/layout";
import type { BranchInfo } from "../../ipc";
import type { Collection, Project } from "../../types";

// Atlas - right-click context menu.

type ActionId =
  | "open-editor"
  | "open-term"
  | "reveal"
  | "pin"
  | "rename"
  | "tag"
  | "checkout"
  | "add-collection"
  | "remove-collection"
  | "select-multi"
  | "arch"
  | "trash";

interface MenuItem {
  id?: ActionId;
  sep?: true;
  icon?: React.ComponentProps<typeof Icon>["name"];
  label?: string;
  keys?: string[];
  danger?: boolean;
  /** Chevron-hinted submenu (currently only "Checkout branch…"). */
  submenu?: true;
}

export function ContextMenu({ projects }: { projects: Project[] }) {
  const menu = useUiStore((s) => s.contextMenu);
  const close = useUiStore((s) => s.closeContextMenu);
  const pushToast = useUiStore((s) => s.pushToast);
  const setRenamingProjectId = useUiStore((s) => s.setRenamingProjectId);
  const collection = useUiStore((s) => s.collection);
  const startMultiSelect = useUiStore((s) => s.startMultiSelect);
  const queryClient = useQueryClient();

  const [tagPickerOpen, setTagPickerOpen] = useState(false);
  const [branchMenuOpen, setBranchMenuOpen] = useState(false);
  const [collectionMenuOpen, setCollectionMenuOpen] = useState(false);
  const [trashConfirmOpen, setTrashConfirmOpen] = useState(false);

  const project = useMemo(
    () => projects.find((p) => p.id === menu?.projectId) ?? null,
    [projects, menu?.projectId],
  );

  // Reset nested UI whenever the menu opens for a new project. Prevents a
  useEffect(() => {
    if (!menu) {
      setTagPickerOpen(false);
      setBranchMenuOpen(false);
      setCollectionMenuOpen(false);
      setTrashConfirmOpen(false);
    }
  }, [menu?.projectId, menu]);

  // Close on any click outside (we rely on the backdrop-less capture pattern
  useEffect(() => {
    if (!menu) return;
    const onClick = () => close();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close();
      }
    };
    // Register async to avoid catching the right-click itself.
    const id = window.setTimeout(() => {
      window.addEventListener("click", onClick);
      window.addEventListener("keydown", onKey);
    }, 0);
    return () => {
      window.clearTimeout(id);
      window.removeEventListener("click", onClick);
      window.removeEventListener("keydown", onKey);
    };
  }, [menu, close]);

  const pinMut = useMutation({
    mutationFn: ({ id, pinned }: { id: string; pinned: boolean }) =>
      pinProject(id, pinned),
    onSuccess: (_d, vars) => {
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      pushToast("success", vars.pinned ? "Pinned" : "Unpinned");
    },
    onError: (err) => pushToast("error", `Pin failed: ${String(err)}`),
  });
  const archiveMut = useMutation({
    mutationFn: ({ id, archived }: { id: string; archived: boolean }) =>
      archiveProject(id, archived),
    onSuccess: (_d, vars) => {
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      pushToast("success", vars.archived ? "Archived" : "Unarchived");
    },
    onError: (err) => pushToast("error", `Archive failed: ${String(err)}`),
  });
  const trashMut = useMutation({
    mutationFn: (id: string) => projectsMoveToTrash(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      pushToast("success", "Moved to Trash");
    },
    onError: (err) => pushToast("error", `Move to Trash failed: ${String(err)}`),
  });

  // U9 - remove the project from the currently-filtered collection. Only
  const removeFromCollectionMut = useMutation({
    mutationFn: ({
      projectId,
      collectionId,
    }: {
      projectId: string;
      collectionId: string;
    }) => collectionsRemoveProject(projectId, collectionId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      queryClient.invalidateQueries({ queryKey: ["collections"] });
      pushToast("success", "Removed from collection");
    },
    onError: (err) =>
      pushToast("warn", `Remove from collection failed: ${String(err)}`),
  });

  if (!menu || !project) return null;

  // True when the sidebar is filtering to a real collection (not the
  const onCollectionView =
    collection !== "all" &&
    collection !== "pinned" &&
    collection !== "archive";

  const items: MenuItem[] = [
    {
      id: "open-editor",
      icon: "code",
      label: "Open in editor",
      keys: ["⌘", "E"],
    },
    {
      id: "open-term",
      icon: "term",
      label: "Open terminal",
      keys: ["⌃", "`"],
    },
    { id: "reveal", icon: "folder", label: "Reveal in Finder" },
    { sep: true },
    {
      id: "pin",
      icon: project.pinned ? "pin-fill" : "pin",
      label: project.pinned ? "Unpin" : "Pin to sidebar",
    },
    { id: "rename", icon: "note", label: "Rename" },
    { id: "tag", icon: "tag", label: "Add tag…", keys: ["T"] },
    { id: "checkout", icon: "branch", label: "Checkout branch…", submenu: true },
    {
      id: "add-collection",
      icon: "folder",
      label: "Add to collection…",
      submenu: true,
    },
    ...(onCollectionView
      ? ([
          {
            id: "remove-collection",
            icon: "archive",
            label: "Remove from this collection",
          },
        ] as MenuItem[])
      : []),
    { sep: true },
    {
      id: "select-multi",
      icon: "square-check",
      label: "Select multiple",
      keys: ["⇧", "⌘", "A"],
    },
    { sep: true },
    {
      id: "arch",
      icon: "archive",
      label: project.archived ? "Unarchive" : "Archive",
    },
    {
      id: "trash",
      icon: "trash",
      label: "Move to Trash",
      danger: true,
      keys: ["⌫"],
    },
  ];

  const onAction = (actionId: ActionId) => {
    switch (actionId) {
      case "open-editor":
        openInEditor(project.id)
          .then(() => pushToast("success", "Opened in editor"))
          .catch((err) =>
            pushToast("error", `Open failed: ${String(err)}`),
          );
        break;
      case "open-term":
        (async () => {
          try {
            const id = await terminalOpen({
              kind: "shell",
              cwd: project.path,
            });
            useTerminalStore
              .getState()
              .addPane(
                makePane(id, "shell", project.path, project.name, {
                  branch: project.branch,
                  projectId: project.id,
                  projectLabel: project.name,
                }),
              );
            pushToast("success", "Terminal opened");
          } catch (err) {
            pushToast("error", `Open terminal failed: ${String(err)}`);
          }
        })();
        break;
      case "reveal":
        revealInFinder(project.id)
          .then(() => pushToast("success", "Revealed in Finder"))
          .catch((err) =>
            pushToast("error", `Reveal failed: ${String(err)}`),
          );
        break;
      case "pin":
        pinMut.mutate({ id: project.id, pinned: !project.pinned });
        break;
      case "rename":
        setRenamingProjectId(project.id);
        break;
      case "tag":
        setTagPickerOpen(true);
        return; // don't close menu yet
      case "checkout":
        setBranchMenuOpen(true);
        return; // don't close menu yet
      case "add-collection":
        setCollectionMenuOpen(true);
        return; // don't close menu yet
      case "remove-collection":
        if (onCollectionView) {
          removeFromCollectionMut.mutate({
            projectId: project.id,
            collectionId: collection,
          });
        }
        break;
      case "select-multi":
        startMultiSelect([project.id]);
        break;
      case "arch":
        archiveMut.mutate({ id: project.id, archived: !project.archived });
        break;
      case "trash":
        setTrashConfirmOpen(true);
        return; // don't close menu yet — confirmation step first
    }
    close();
  };

  // Clamp coords so the menu stays on-screen. 240px wide × ~340px tall is a
  const MENU_W = 240;
  const MENU_H = 340;
  const pad = 8;
  const left = Math.min(menu.x, window.innerWidth - MENU_W - pad);
  const top = Math.min(menu.y, window.innerHeight - MENU_H - pad);

  return createPortal(
    <div
      onClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => {
        // Swallow right-click on the menu itself so the page's default doesn't
        e.preventDefault();
      }}
      role="menu"
      aria-label="Project actions"
      style={{
        position: "fixed",
        top,
        left,
        zIndex: 300,
        minWidth: 220,
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
      {items.map((it, i) =>
        it.sep ? (
          <div
            key={`sep-${i}`}
            role="separator"
            style={{
              height: 1,
              background: "var(--line)",
              margin: "4px 0",
            }}
          />
        ) : (
          <div
            key={it.id ?? `row-${i}`}
            onClick={() => it.id && onAction(it.id)}
            role="menuitem"
            tabIndex={0}
            aria-label={it.label}
            onKeyDown={(e) => {
              if ((e.key === "Enter" || e.key === " ") && it.id) {
                e.preventDefault();
                onAction(it.id);
              }
            }}
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
              color: it.danger ? "var(--danger)" : "var(--text)",
            }}
          >
            {it.icon && <Icon name={it.icon} size={13} stroke="currentColor" />}
            <span style={{ flex: 1 }}>{it.label}</span>
            {it.submenu && (
              <span
                style={{ color: "var(--text-dim)", fontSize: 11, marginLeft: 4 }}
              >
                ›
              </span>
            )}
            {it.keys && (
              <span style={{ display: "flex", gap: 2 }}>
                {it.keys.map((k, j) => (
                  <Kbd key={j}>{k}</Kbd>
                ))}
              </span>
            )}
          </div>
        ),
      )}

      {tagPickerOpen && (
        <TagPicker
          project={project}
          onDone={() => {
            setTagPickerOpen(false);
            close();
          }}
        />
      )}

      {branchMenuOpen && (
        <BranchSubmenu
          project={project}
          onDone={() => {
            setBranchMenuOpen(false);
            close();
          }}
        />
      )}

      {collectionMenuOpen && (
        <AddToCollectionSubmenu
          project={project}
          onDone={() => {
            setCollectionMenuOpen(false);
            close();
          }}
        />
      )}

      {trashConfirmOpen && (
        <TrashConfirm
          onCancel={() => setTrashConfirmOpen(false)}
          onConfirm={() => {
            trashMut.mutate(project.id);
            setTrashConfirmOpen(false);
            close();
          }}
        />
      )}
    </div>,
    document.body,
  );
}

/** Small popover that collects a new tag + persists via `projects_set_tags`. */
function TagPicker({
  project,
  onDone,
}: {
  project: Project;
  onDone: () => void;
}) {
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();
  const [value, setValue] = useState("");

  const mut = useMutation({
    mutationFn: (tag: string) =>
      setProjectTags(project.id, Array.from(new Set([...project.tags, tag]))),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      queryClient.invalidateQueries({ queryKey: ["tags"] });
      pushToast("success", `Tagged with "${value}"`);
      onDone();
    },
    onError: (err) => pushToast("error", `Tag failed: ${String(err)}`),
  });

  return (
    <div
      onClick={(e) => e.stopPropagation()}
      style={{
        marginTop: 6,
        padding: 6,
        background: "var(--surface-2)",
        border: "1px solid var(--line)",
        borderRadius: 5,
      }}
    >
      <input
        autoFocus
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder="new-tag"
        onKeyDown={(e) => {
          if (e.key === "Enter" && value.trim()) {
            mut.mutate(value.trim());
          }
          if (e.key === "Escape") onDone();
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
    </div>
  );
}

// Branch checkout submenu - lists branches from `git_branch_list`, each
function BranchSubmenu({
  project,
  onDone,
}: {
  project: Project;
  onDone: () => void;
}) {
  const pushToast = useUiStore((s) => s.pushToast);
  const [branches, setBranches] = useState<BranchInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    gitBranchList(project.id)
      .then((list) => {
        if (!cancelled) setBranches(list);
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [project.id]);

  const onPick = async (branch: string) => {
    try {
      const preview = await gitCheckout(project.id, branch);
      const parts: string[] = [
        `Preview "${preview.branch}": ${preview.filesWouldChange} file(s) would change`,
      ];
      if (preview.isDirty) parts.push("working tree is dirty");
      if (preview.warning) parts.push(preview.warning);
      pushToast(preview.isDirty ? "warn" : "info", parts.join(" · "));
    } catch (err) {
      pushToast("error", `Checkout preview failed: ${String(err)}`);
    }
    onDone();
  };

  return (
    <div
      onClick={(e) => e.stopPropagation()}
      style={{
        marginTop: 6,
        padding: 4,
        background: "var(--surface-2)",
        border: "1px solid var(--line)",
        borderRadius: 5,
        maxHeight: 220,
        overflowY: "auto",
      }}
    >
      {error && (
        <div
          style={{
            padding: "6px 8px",
            fontSize: 11,
            color: "var(--danger)",
            fontFamily: "var(--mono)",
          }}
        >
          {error}
        </div>
      )}
      {!error && branches === null && (
        <div
          style={{
            padding: "6px 8px",
            fontSize: 11,
            color: "var(--text-dim)",
            fontFamily: "var(--mono)",
          }}
        >
          loading…
        </div>
      )}
      {!error && branches && branches.length === 0 && (
        <div
          style={{
            padding: "6px 8px",
            fontSize: 11,
            color: "var(--text-dim)",
            fontFamily: "var(--mono)",
          }}
        >
          no branches
        </div>
      )}
      {!error &&
        branches?.map((b) => (
          <div
            key={b.name}
            onClick={() => onPick(b.name)}
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
              color: b.isRemote ? "var(--text-dim)" : "var(--text)",
              fontFamily: "var(--mono)",
            }}
          >
            <Icon name="branch" size={10} stroke="currentColor" />
            <span style={{ flex: 1 }}>{b.name}</span>
            {b.isHead && (
              <span
                style={{
                  fontSize: 10,
                  color: "var(--accent)",
                  letterSpacing: 0.4,
                }}
              >
                HEAD
              </span>
            )}
          </div>
        ))}
    </div>
  );
}

// U9 - "Add to collection" submenu. Lists all known collections (via
function AddToCollectionSubmenu({
  project,
  onDone,
}: {
  project: Project;
  onDone: () => void;
}) {
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();

  const { data: collections = [], isLoading, error } = useQuery<Collection[]>({
    queryKey: ["collections"],
    queryFn: listCollections,
    retry: false,
  });

  const mut = useMutation({
    mutationFn: ({
      projectId,
      collectionId,
    }: {
      projectId: string;
      collectionId: string;
    }) => collectionsAddProject(projectId, collectionId),
    onSuccess: (_d, vars) => {
      const label =
        collections.find((c) => c.id === vars.collectionId)?.label ??
        "collection";
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      queryClient.invalidateQueries({ queryKey: ["collections"] });
      pushToast("success", `Added to ${label}`);
      onDone();
    },
    onError: (err) => {
      pushToast("warn", `Add to collection failed: ${String(err)}`);
      onDone();
    },
  });

  return (
    <div
      onClick={(e) => e.stopPropagation()}
      style={{
        marginTop: 6,
        padding: 4,
        background: "var(--surface-2)",
        border: "1px solid var(--line)",
        borderRadius: 5,
        maxHeight: 220,
        overflowY: "auto",
      }}
    >
      {error && (
        <div
          style={{
            padding: "6px 8px",
            fontSize: 11,
            color: "var(--danger)",
            fontFamily: "var(--mono)",
          }}
        >
          {String(error)}
        </div>
      )}
      {!error && isLoading && (
        <div
          style={{
            padding: "6px 8px",
            fontSize: 11,
            color: "var(--text-dim)",
            fontFamily: "var(--mono)",
          }}
        >
          loading…
        </div>
      )}
      {!error && !isLoading && collections.length === 0 && (
        <div
          style={{
            padding: "6px 8px",
            fontSize: 11,
            color: "var(--text-dim)",
            fontFamily: "var(--mono)",
          }}
        >
          no collections — create one from the sidebar
        </div>
      )}
      {!error &&
        collections.map((c) => {
          const already = project.collectionIds.includes(c.id);
          return (
            <div
              key={c.id}
              onClick={() =>
                mut.mutate({
                  projectId: project.id,
                  collectionId: c.id,
                })
              }
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
                color: "var(--text)",
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
              {already && (
                <Icon name="check" size={11} stroke="var(--text-dim)" />
              )}
            </div>
          );
        })}
    </div>
  );
}

// Two-button confirm shown inline before firing `projects_move_to_trash`.
function TrashConfirm({
  onCancel,
  onConfirm,
}: {
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div
      onClick={(e) => e.stopPropagation()}
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
        This will move the project folder to your system trash.
      </div>
      <div style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
        <button
          onClick={onCancel}
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
          onClick={onConfirm}
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
          Move to Trash
        </button>
      </div>
    </div>
  );
}
