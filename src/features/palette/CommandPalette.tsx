import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Icon, Kbd, LangDot } from "../../components/Icon";
import {
  paletteQuery,
  pushRecent,
  revealInFinder,
  type PaletteItem,
} from "../../ipc";
import { useUiStore } from "../../state/store";
import type { Project } from "../../types";

// Atlas - ⌘K command palette.

const STATIC_ACTIONS: Array<{
  id: string;
  icon:
    | "plus"
    | "clone"
    | "import"
    | "gear"
    | "term";
  label: string;
  keys?: string[];
}> = [
  { id: "new", icon: "plus", label: "New project…", keys: ["⌘", "N"] },
  {
    id: "clone",
    icon: "clone",
    label: "Clone from git URL…",
    keys: ["⌘", "⇧", "N"],
  },
  { id: "import", icon: "import", label: "Import existing folder…" },
  { id: "settings", icon: "gear", label: "Open settings", keys: ["⌘", ","] },
  { id: "term", icon: "term", label: "Open terminal here", keys: ["⌃", "`"] },
];

interface FlatRow {
  key: string;
  section: "project" | "action";
  onEnter: (opts: { reveal: boolean }) => void;
  // Project variant.
  project?: Project;
  // Action variant.
  action?: (typeof STATIC_ACTIONS)[number];
  // Note variant (shown when palette_query returns note matches).
  note?: { projectId: string; noteId: string; title: string; snippet: string };
}

export function CommandPalette() {
  const open = useUiStore((s) => s.paletteOpen);
  const setOpen = useUiStore((s) => s.setPaletteOpen);
  const setSelectedProjectId = useUiStore((s) => s.setSelectedProjectId);
  const pushToast = useUiStore((s) => s.pushToast);
  const openNewProject = useUiStore((s) => s.openNewProject);
  const openSettings = useUiStore((s) => s.openSettings);
  const queryClient = useQueryClient();

  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  // Reset state + focus input on open.
  useEffect(() => {
    if (!open) return;
    setQuery("");
    setSel(0);
    // Defer focus until after the portal mounts.
    requestAnimationFrame(() => inputRef.current?.focus());
  }, [open]);

  // `palette_query` is owned by D5. If it hasn't shipped, fall back to a
  const { data: items = [] } = useQuery<PaletteItem[]>({
    queryKey: ["palette", query],
    queryFn: () => paletteQuery(query, 6),
    enabled: open,
    retry: false,
    staleTime: 0,
  });

  // Client-side fallback. Pulls from the `['projects']` cache; produces
  const fallback = useMemo((): PaletteItem[] => {
    if (items.length > 0) return [];
    const cached =
      queryClient.getQueryData<Project[]>(["projects"]) ?? [];
    const q = query.trim().toLowerCase();
    const matches = cached
      .filter((p) => !p.archived)
      .filter((p) => {
        if (!q) return true;
        if (p.name.toLowerCase().includes(q)) return true;
        if (p.tags.some((t) => t.toLowerCase().includes(q))) return true;
        return false;
      })
      .slice(0, 6)
      .map<PaletteItem>((project) => ({
        kind: "project",
        project,
        score: 0,
      }));
    return matches;
  }, [items, query, queryClient]);

  const effectiveItems = items.length > 0 ? items : fallback;

  // Build the two sections: projects (live) + actions (static, filtered).
  const flat = useMemo<FlatRow[]>(() => {
    const rows: FlatRow[] = [];

    for (const it of effectiveItems) {
      if (it.kind === "project" || it.kind === "recent") {
        const project = it.project;
        rows.push({
          key: `proj-${project.id}`,
          section: "project",
          project,
          onEnter: ({ reveal }) => {
            if (reveal) {
              revealInFinder(project.id).catch((err) => {
                pushToast("error", `Reveal failed: ${String(err)}`);
              });
            } else {
              pushRecent(project.id).catch(() => {
                // recents_push is; silently ignore until it ships.
              });
              setSelectedProjectId(project.id);
            }
          },
        });
      } else if (it.kind === "note") {
        rows.push({
          key: `note-${it.projectId}-${it.noteId}`,
          section: "project",
          note: {
            projectId: it.projectId,
            noteId: it.noteId,
            title: it.title,
            snippet: it.snippet,
          },
          onEnter: () => {
            setSelectedProjectId(it.projectId);
            // → notes flow; we just select the project.
            pushToast("info", `Opened ${it.title}`);
          },
        });
      }
      // Palette_query `action` rows are superseded by the static list below.
    }

    const q = query.trim().toLowerCase();
    const visibleActions = STATIC_ACTIONS.filter(
      (a) => !q || a.label.toLowerCase().includes(q),
    );
    for (const a of visibleActions) {
      rows.push({
        key: `act-${a.id}`,
        section: "action",
        action: a,
        onEnter: () => {
          switch (a.id) {
            case "new":
              openNewProject("new");
              break;
            case "clone":
              openNewProject("clone");
              break;
            case "import":
              openNewProject("import");
              break;
            case "settings":
              openSettings("general");
              break;
            case "term":
              pushToast("info", "Open a terminal from the inspector");
              break;
          }
        },
      });
    }
    return rows;
  }, [
    effectiveItems,
    query,
    openNewProject,
    openSettings,
    pushToast,
    setSelectedProjectId,
  ]);

  // Clamp selection whenever the flat list shrinks (e.g. typing narrows
  useEffect(() => {
    if (sel > flat.length - 1) setSel(Math.max(0, flat.length - 1));
  }, [flat.length, sel]);

  // Keyboard navigation - only while open.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        setOpen(false);
        return;
      }
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSel((i) => Math.min(flat.length - 1, i + 1));
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setSel((i) => Math.max(0, i - 1));
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        const row = flat[sel];
        if (!row) return;
        row.onEnter({ reveal: e.metaKey || e.ctrlKey });
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, flat, sel, setOpen]);

  if (!open) return null;

  // Split for section headings while preserving the flat index for keyboard
  const projectRows = flat.filter((r) => r.section === "project");
  const actionRows = flat.filter((r) => r.section === "action");
  const renderRow = (row: FlatRow, flatIdx: number) => {
    const active = flatIdx === sel;
    return (
      <div
        key={row.key}
        onMouseEnter={() => setSel(flatIdx)}
        onClick={(e) => {
          row.onEnter({ reveal: e.metaKey || e.ctrlKey });
          setOpen(false);
        }}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "0 14px",
          height: 28,
          background: active ? "var(--row-active)" : "transparent",
          borderLeft: active
            ? "2px solid var(--accent)"
            : "2px solid transparent",
          cursor: "pointer",
          fontSize: 13,
        }}
      >
        {row.project ? (
          <>
            <LangDot color={row.project.color} />
            <span style={{ flex: 1 }}>{row.project.name}</span>
            <span
              style={{
                fontSize: 10,
                fontFamily: "var(--mono)",
                color: "var(--text-dim)",
              }}
            >
              {row.project.branch}
              {row.project.lastOpened
                ? ` · ${formatRelative(row.project.lastOpened)}`
                : ""}
            </span>
          </>
        ) : row.note ? (
          <>
            <Icon
              name="note"
              size={13}
              stroke={active ? "var(--accent)" : "var(--text-dim)"}
            />
            <span style={{ flex: 1 }}>
              {row.note.title}
              <span
                style={{
                  marginLeft: 8,
                  fontSize: 10,
                  fontFamily: "var(--mono)",
                  color: "var(--text-dimmer)",
                }}
              >
                {row.note.snippet}
              </span>
            </span>
          </>
        ) : row.action ? (
          <>
            <Icon
              name={row.action.icon}
              size={13}
              stroke={active ? "var(--accent)" : "var(--text-dim)"}
            />
            <span style={{ flex: 1 }}>{row.action.label}</span>
            {row.action.keys && (
              <span style={{ display: "flex", gap: 3 }}>
                {row.action.keys.map((k, j) => (
                  <Kbd key={j}>{k}</Kbd>
                ))}
              </span>
            )}
          </>
        ) : null}
      </div>
    );
  };

  let cursor = 0;

  return createPortal(
    <div
      onClick={() => setOpen(false)}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 500,
        background: "rgba(0,0,0,0.45)",
        backdropFilter: "blur(4px)",
        WebkitBackdropFilter: "blur(4px)",
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        paddingTop: "10vh",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        style={{
          width: 580,
          maxHeight: "70vh",
          background: "var(--palette-bg)",
          border: "1px solid var(--line)",
          borderRadius: 10,
          backdropFilter: "blur(30px) saturate(180%)",
          WebkitBackdropFilter: "blur(30px) saturate(180%)",
          boxShadow: "0 30px 80px rgba(0,0,0,0.5)",
          overflow: "hidden",
          display: "flex",
          flexDirection: "column",
          fontFamily: "var(--sans)",
          color: "var(--text)",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            padding: "12px 14px",
            borderBottom: "1px solid var(--line)",
          }}
        >
          <Icon name="search" size={14} stroke="var(--text-dim)" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSel(0);
            }}
            placeholder="Jump to project, file, or action…"
            style={{
              flex: 1,
              background: "none",
              border: "none",
              outline: "none",
              color: "var(--text)",
              fontSize: 14,
              fontFamily: "var(--sans)",
            }}
          />
          <Kbd>esc</Kbd>
        </div>
        <div style={{ overflowY: "auto", padding: "6px 0", flex: 1 }}>
          {flat.length === 0 && (
            <div
              style={{
                padding: 28,
                textAlign: "center",
                color: "var(--text-dimmer)",
                fontSize: 12,
              }}
            >
              No matches.
            </div>
          )}
          {projectRows.length > 0 && (
            <>
              <SectionLabel>Projects</SectionLabel>
              {projectRows.map((r) => renderRow(r, cursor++))}
            </>
          )}
          {actionRows.length > 0 && (
            <>
              <SectionLabel>Actions</SectionLabel>
              {actionRows.map((r) => renderRow(r, cursor++))}
            </>
          )}
        </div>
        <div
          style={{
            display: "flex",
            gap: 14,
            padding: "8px 14px",
            borderTop: "1px solid var(--line)",
            fontSize: 10,
            fontFamily: "var(--mono)",
            color: "var(--text-dim)",
          }}
        >
          <span>
            <Kbd>↵</Kbd> open
          </span>
          <span>
            <Kbd>⌘</Kbd>
            <Kbd>↵</Kbd> reveal
          </span>
          <span style={{ flex: 1 }} />
          <span>
            ↑↓ navigate · <Kbd>esc</Kbd> close
          </span>
        </div>
      </div>
    </div>,
    document.body,
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        padding: "8px 14px 4px",
        fontSize: 10,
        fontFamily: "var(--mono)",
        color: "var(--text-dim)",
        textTransform: "uppercase",
        letterSpacing: 0.6,
      }}
    >
      {children}
    </div>
  );
}

// Compact relative-time label; shared copy of ProjectList's helper
function formatRelative(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const diffMs = Date.now() - d.getTime();
  const hours = Math.floor(diffMs / 3_600_000);
  if (hours < 1) return "now";
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  const weeks = Math.floor(days / 7);
  if (weeks < 5) return `${weeks}w`;
  return d.toISOString().slice(0, 10);
}
