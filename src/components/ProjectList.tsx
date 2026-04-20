import { useMemo, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Virtuoso } from "react-virtuoso";
import { Icon, LangDot } from "./Icon";
import { useUiStore } from "../state/store";
import { archiveProject, renameProject } from "../ipc";
import type { Project } from "../types";
import type { SortKey } from "../state/store";

interface ProjectListProps {
  projects: Project[];
  onContextMenu?: (e: React.MouseEvent, p: Project) => void;
  // Pass 3 - when true, an extra AUTHOR column is rendered between
  showAuthor?: boolean;
}

// Atlas - project list (dense table).
export function ProjectList({
  projects,
  onContextMenu,
  showAuthor = false,
}: ProjectListProps) {
  const selected = useUiStore((s) => s.selectedProjectId);
  const setSelected = useUiStore((s) => s.setSelectedProjectId);
  const sort = useUiStore((s) => s.sort);
  const collection = useUiStore((s) => s.collection);
  const renamingProjectId = useUiStore((s) => s.renamingProjectId);
  const setRenamingProjectId = useUiStore((s) => s.setRenamingProjectId);
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();
  const multiSelect = useUiStore((s) => s.multiSelect);
  const toggleMultiSelect = useUiStore((s) => s.toggleMultiSelect);
  const multiSelectIds = useMemo(
    () => new Set(multiSelect.ids),
    [multiSelect.ids],
  );

  // Pass 3 - grid template flips based on whether the AUTHOR column is
  // shown. The SIZE column is wider than it was (was 70px) so that rows
  // with significant on-disk bloat can render both the source size and
  // a compact `+16G` delta without crowding neighbouring columns.
  const gridTemplate = showAuthor
    ? "24px 1.6fr 1fr 120px 80px 110px 70px 80px"
    : "24px 1.6fr 1fr 80px 110px 70px 80px";

  const renameMut = useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) =>
      renameProject(id, name),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["projects"] }),
    onError: (err) => pushToast("error", `Rename failed: ${String(err)}`),
  });

  // so the row stays the same visual width everywhere else.
  const unarchiveMut = useMutation({
    mutationFn: (id: string) => archiveProject(id, false),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      pushToast("success", "Unarchived");
    },
    onError: (err) => pushToast("error", `Unarchive failed: ${String(err)}`),
  });

  const isArchiveView = collection === "archive";

  const sorted = useMemo(() => sortProjects(projects, sort), [projects, sort]);

  return (
    <div className="flex-1 min-h-0 flex flex-col">
      {/* Column headers — sticky above the virtualized rows. */}
      <div
        className="grid gap-3 px-4 h-6 items-center border-b border-line bg-chrome font-mono text-[10px] text-text-dim uppercase tracking-[0.6px] shrink-0"
        style={{
          gridTemplateColumns: gridTemplate,
        }}
      >
        <span />
        <span>name</span>
        <span>branch</span>
        {showAuthor && <span>author</span>}
        <span className="text-right">loc</span>
        <span className="text-right">size</span>
        <span className="text-right">todos</span>
        <span className="text-right">opened</span>
      </div>

      <Virtuoso
        data={sorted}
        className="flex-1"
        computeItemKey={(_, p) => p.id}
        itemContent={(_index, p) => (
          <div
            onClick={() => {
              if (multiSelect.active) toggleMultiSelect(p.id);
              else setSelected(p.id);
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              // overlay (wired in `<App />`).
              onContextMenu?.(e, p);
            }}
            className="group relative grid gap-3 px-4 items-center border-b border-line-soft cursor-pointer text-xs"
            style={{
              gridTemplateColumns: gridTemplate,
              height: "var(--row-h, 26px)",
              background:
                multiSelect.active && multiSelectIds.has(p.id)
                  ? "var(--row-active)"
                  : selected === p.id && !multiSelect.active
                    ? "var(--row-active)"
                    : "transparent",
              boxShadow:
                (multiSelect.active && multiSelectIds.has(p.id)) ||
                (selected === p.id && !multiSelect.active)
                  ? "inset 2px 0 0 var(--accent)"
                  : "none",
            }}
          >
            {multiSelect.active ? (
              <input
                type="checkbox"
                checked={multiSelectIds.has(p.id)}
                onChange={(e) => {
                  e.stopPropagation();
                  toggleMultiSelect(p.id);
                }}
                onClick={(e) => e.stopPropagation()}
                className="m-0 w-[14px] h-[14px] accent-accent"
                aria-label={`Select ${p.name}`}
              />
            ) : (
              <LangDot color={p.color} />
            )}
            <div className="flex items-center gap-2 min-w-0">
              {p.pinned && (
                <Icon
                  name="pin-fill"
                  size={10}
                  stroke="var(--accent)"
                  style={{ flexShrink: 0 }}
                />
              )}
              {/* Name never truncates - path takes remaining space.
                  Renaming swaps in an inline input. */}
              {renamingProjectId === p.id ? (
                <RenameInput
                  initial={p.name}
                  onCommit={(next) => {
                    setRenamingProjectId(null);
                    const trimmed = next.trim();
                    if (trimmed && trimmed !== p.name) {
                      renameMut.mutate({ id: p.id, name: trimmed });
                    }
                  }}
                  onCancel={() => setRenamingProjectId(null)}
                />
              ) : (
                <span
                  className="font-medium text-text whitespace-nowrap"
                  style={{ flexShrink: 0 }}
                  title={p.name}
                >
                  {p.name}
                </span>
              )}
              <span
                className="font-mono text-[10px] text-text-dimmer truncate min-w-0"
                title={p.path}
              >
                {p.path.replace(/^~\//, "")}
              </span>
            </div>
            <div className="flex items-center gap-[6px] text-text-dim font-mono text-[11px] min-w-0">
              <Icon name="branch" size={10} />
              <span className="truncate">{p.branch}</span>
              {p.dirty > 0 && (
                <span className="text-warn">&#9679;{p.dirty}</span>
              )}
              {p.ahead > 0 && (
                <span className="text-accent">&uarr;{p.ahead}</span>
              )}
              {p.behind > 0 && (
                <span className="text-info">&darr;{p.behind}</span>
              )}
            </div>
            {showAuthor && (
              <span
                className="font-mono text-[11px] text-text-dim truncate min-w-0"
                title={p.author ?? "Unknown author"}
              >
                {p.author ?? "—"}
              </span>
            )}
            <span
              className="text-right font-mono text-[11px] text-text-dim"
              title={`${p.loc.toLocaleString()} lines of code`}
            >
              {p.loc.toLocaleString()}
            </span>
            <SizeCell project={p} />
            <span
              className="text-right font-mono text-[11px]"
              style={{
                color: p.todosCount > 0 ? "var(--text)" : "var(--text-dimmer)",
              }}
            >
              {p.todosCount > 0 ? p.todosCount : "—"}
            </span>
            <span className="text-right font-mono text-[11px] text-text-dim">
              {formatLastOpened(p.lastOpened)}
            </span>

            {/*
              Iter 7 — Archive-view hover action. Absolute-positioned so the
              grid template above stays pixel-identical between archived and
              non-archived views. Only visible on row hover (opacity-0 →
              group-hover:opacity-100) to keep the scanning list clean.
            */}
            {isArchiveView && (
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  unarchiveMut.mutate(p.id);
                }}
                title="Unarchive"
                className="absolute right-3 top-1/2 -translate-y-1/2 inline-flex items-center gap-[4px] px-[8px] h-[20px] rounded-[4px] border border-line bg-surface-2 text-text-dim hover:text-text font-mono text-[10px] opacity-0 group-hover:opacity-100 focus:opacity-100"
              >
                <Icon name="archive" size={10} />
                Unarchive
              </button>
            )}
          </div>
        )}
      />
    </div>
  );
}

// Size cell: shows source size, plus a `+16G` bloat chip when the on-disk
// footprint meaningfully exceeds the gitignored source. The `title`
// attribute carries the full breakdown (source / on-disk / excess) so
// users who hover get the exact numbers without widening the column.
function SizeCell({ project }: { project: Project }) {
  const excess = Math.max(0, project.diskBytes - project.sizeBytes);
  // Threshold: don't flag < 50 MB of extra data; those are lockfiles,
  // small caches, pre-commit hooks — nothing worth warning about.
  const showBloat = excess > 50 * 1024 * 1024;
  const title = showBloat
    ? `Source: ${project.size} (${project.sizeBytes.toLocaleString()} bytes)\n` +
      `On disk: ${project.diskSize} (${project.diskBytes.toLocaleString()} bytes)\n` +
      `Excess: ${formatBytesVerbose(excess)} — gitignored files (node_modules, build outputs, caches)`
    : `${project.sizeBytes.toLocaleString()} bytes`;

  return (
    <span
      className="text-right font-mono text-[11px] text-text-dim flex items-center justify-end gap-[4px]"
      title={title}
    >
      <span>{project.size}</span>
      {showBloat && (
        <span
          className="inline-flex items-center font-mono text-[10px] px-[3px] rounded-[2px]"
          style={{
            color: "var(--warn, #d97757)",
            background: "color-mix(in oklch, var(--warn, #d97757) 12%, transparent)",
          }}
        >
          +{formatBytesCompact(excess)}
        </span>
      )}
    </span>
  );
}

function formatBytesCompact(n: number): string {
  if (n < 1024) return `${n}B`;
  const units = ["K", "M", "G", "T"] as const;
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return v >= 10 ? `${Math.round(v)}${units[i]}` : `${v.toFixed(1)}${units[i]}`;
}

function formatBytesVerbose(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"] as const;
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(1)} ${units[i]}`;
}

// Tiny controlled input for inline row renames. Auto-focuses + selects on
function RenameInput({
  initial,
  onCommit,
  onCancel,
}: {
  initial: string;
  onCommit: (next: string) => void;
  onCancel: () => void;
}) {
  const [v, setV] = useState(initial);
  return (
    <input
      autoFocus
      value={v}
      onClick={(e) => e.stopPropagation()}
      onChange={(e) => setV(e.target.value)}
      onBlur={() => onCommit(v)}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          onCommit(v);
        } else if (e.key === "Escape") {
          e.preventDefault();
          onCancel();
        }
      }}
      className="px-[4px] py-[1px] rounded-[3px] border border-accent bg-surface-2 text-text text-xs font-medium outline-none"
      style={{ minWidth: 0, maxWidth: 180 }}
    />
  );
}

function sortProjects(projects: Project[], sort: SortKey): Project[] {
  const copy = [...projects];
  switch (sort) {
    case "name":
      return copy.sort((a, b) => a.name.localeCompare(b.name));
    case "size":
      return copy.sort((a, b) => b.sizeBytes - a.sizeBytes);
    case "branch":
      return copy.sort((a, b) => a.branch.localeCompare(b.branch));
    case "recent":
    default:
      return copy.sort((a, b) => {
        const aa = a.lastOpened ?? "";
        const bb = b.lastOpened ?? "";
        return bb.localeCompare(aa);
      });
  }
}

function formatLastOpened(iso: string | null): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const now = new Date();
  const diffMs = now.getTime() - d.getTime();
  const hours = Math.floor(diffMs / 3_600_000);
  if (hours < 1) return "now";
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  const weeks = Math.floor(days / 7);
  if (weeks < 5) return `${weeks}w`;
  return d.toISOString().slice(0, 10);
}

// Center header above the list/grid - sort pills + view toggle.
export function CenterHeader({
  projectCount,
  collectionLabel,
}: {
  projectCount: number;
  collectionLabel: string;
}) {
  const sort = useUiStore((s) => s.sort);
  const setSort = useUiStore((s) => s.setSort);
  const viewMode = useUiStore((s) => s.viewMode);
  const setViewMode = useUiStore((s) => s.setViewMode);
  const multiSelectActive = useUiStore((s) => s.multiSelect.active);
  const startMultiSelect = useUiStore((s) => s.startMultiSelect);
  const clearMultiSelect = useUiStore((s) => s.clearMultiSelect);
  const currentSelection = useUiStore((s) => s.selectedProjectId);

  const pills: SortKey[] = ["recent", "name", "size", "branch"];

  return (
    <div className="flex items-center gap-[14px] px-4 h-[42px] border-b border-line bg-chrome shrink-0">
      <span className="text-[13px] font-semibold">{collectionLabel}</span>
      <span className="font-mono text-[11px] text-text-dim">
        {projectCount} projects
      </span>
      <div className="flex-1" />

      <div className="flex gap-1">
        {pills.map((s) => {
          const active = sort === s;
          return (
            <button
              key={s}
              onClick={() => setSort(s)}
              className="relative px-2 py-[3px] font-mono text-[11px] rounded-[4px]"
              style={{
                background: active ? "var(--surface-2)" : "transparent",
                color: active ? "var(--text)" : "var(--text-dim)",
                // Accent underline on the active pill. Absolute-positioned
                boxShadow: active
                  ? "inset 0 -2px 0 var(--accent)"
                  : "none",
              }}
            >
              {active ? "↓ " : ""}
              {s}
            </button>
          );
        })}
      </div>

      <div className="w-px h-4 bg-line" />

      <button
        type="button"
        onClick={() =>
          multiSelectActive
            ? clearMultiSelect()
            : startMultiSelect(currentSelection ? [currentSelection] : [])
        }
        title={multiSelectActive ? "Exit multi-select (Esc)" : "Multi-select (⇧⌘A)"}
        className="flex items-center gap-1 px-2 py-[3px] font-mono text-[11px] rounded-[4px]"
        style={{
          background: multiSelectActive ? "var(--row-active)" : "transparent",
          color: multiSelectActive ? "var(--text)" : "var(--text-dim)",
          border: `1px solid ${multiSelectActive ? "var(--accent)" : "var(--line)"}`,
        }}
      >
        <Icon name="square-check" size={12} />
        select
      </button>

      <div className="w-px h-4 bg-line" />

      <div className="flex border border-line rounded-[4px] overflow-hidden">
        <button
          onClick={() => setViewMode("list")}
          className="px-2 py-[3px]"
          style={{
            background:
              viewMode === "list" ? "var(--surface-2)" : "transparent",
            color: viewMode === "list" ? "var(--text)" : "var(--text-dim)",
          }}
        >
          <Icon name="list" size={12} />
        </button>
        <button
          onClick={() => setViewMode("grid")}
          className="px-2 py-[3px] border-l border-line"
          style={{
            background:
              viewMode === "grid" ? "var(--surface-2)" : "transparent",
            color: viewMode === "grid" ? "var(--text)" : "var(--text-dim)",
          }}
        >
          <Icon name="grid" size={12} />
        </button>
      </div>
    </div>
  );
}
