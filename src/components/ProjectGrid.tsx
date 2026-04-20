import { useMemo } from "react";
import clsx from "clsx";
import { Icon, LangDot } from "./Icon";
import { useUiStore } from "../state/store";
import type { Project } from "../types";
import type { SortKey } from "../state/store";

interface ProjectGridProps {
  projects: Project[];
  onContextMenu?: (e: React.MouseEvent, p: Project) => void;
  // Pass 3 - accepted but unused in grid mode (too crowded for
  showAuthor?: boolean;
}

// Atlas - project grid (dense card layout).
export function ProjectGrid({
  projects,
  onContextMenu,
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  showAuthor: _showAuthor,
}: ProjectGridProps) {
  const selected = useUiStore((s) => s.selectedProjectId);
  const setSelected = useUiStore((s) => s.setSelectedProjectId);
  const sort = useUiStore((s) => s.sort);
  const multiSelect = useUiStore((s) => s.multiSelect);
  const toggleMultiSelect = useUiStore((s) => s.toggleMultiSelect);
  const multiSelectIds = useMemo(
    () => new Set(multiSelect.ids),
    [multiSelect.ids],
  );

  const sorted = useMemo(() => sortProjects(projects, sort), [projects, sort]);

  return (
    <div
      className="flex-1 min-h-0 overflow-y-auto p-4 grid gap-3 content-start"
      style={{ gridTemplateColumns: "repeat(auto-fill, minmax(200px, 1fr))" }}
    >
      {sorted.map((p) => {
        const isMultiSelected = multiSelect.active && multiSelectIds.has(p.id);
        const isSelected = !multiSelect.active && selected === p.id;
        const highlighted = isSelected || isMultiSelected;
        return (
          <div
            key={p.id}
            onClick={() => {
              if (multiSelect.active) toggleMultiSelect(p.id);
              else setSelected(p.id);
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              onContextMenu?.(e, p);
            }}
            className={clsx(
              "rounded-md p-3 cursor-pointer flex flex-col gap-[10px] border",
            )}
            style={{
              borderColor: highlighted ? "var(--accent)" : "var(--line)",
              background: highlighted ? "var(--row-active)" : "var(--surface)",
            }}
          >
            <div className="flex items-center gap-2">
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
                <LangDot color={p.color} size={10} />
              )}
              <span className="flex-1 font-medium text-[13px] truncate">
                {p.name}
              </span>
              {p.pinned && (
                <Icon name="pin-fill" size={11} stroke="var(--accent)" />
              )}
            </div>
            <div className="font-mono text-[10px] text-text-dimmer flex items-center gap-[6px]">
              <Icon name="branch" size={10} />
              <span className="truncate">{p.branch}</span>
            </div>
            <div className="flex gap-[6px] flex-wrap">
              {p.dirty > 0 && (
                <StatChip color="warn" icon="git">
                  {p.dirty}M
                </StatChip>
              )}
              {p.ahead > 0 && (
                <StatChip color="accent" icon="arrow-up">
                  {p.ahead}
                </StatChip>
              )}
              {p.behind > 0 && (
                <StatChip color="info" icon="arrow-down">
                  {p.behind}
                </StatChip>
              )}
              {p.todosCount > 0 && (
                <StatChip icon="square-check">{p.todosCount}</StatChip>
              )}
            </div>
            <div className="flex justify-between font-mono text-[10px] text-text-dim">
              <span>{p.size}</span>
              <span>{formatLastOpened(p.lastOpened)}</span>
            </div>
          </div>
        );
      })}
    </div>
  );
}

function StatChip({
  children,
  icon,
  color,
}: {
  children: React.ReactNode;
  icon: React.ComponentProps<typeof Icon>["name"];
  color?: "warn" | "accent" | "info";
}) {
  const c =
    color === "warn"
      ? "var(--warn)"
      : color === "accent"
        ? "var(--accent)"
        : color === "info"
          ? "var(--info)"
          : "var(--text-dim)";
  return (
    <span
      className="inline-flex items-center gap-[3px] px-[5px] py-[1px] rounded-[3px] bg-surface-2 font-mono text-[10px]"
      style={{ color: c }}
    >
      <Icon name={icon} size={9} stroke={c} />
      {children}
    </span>
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
  const hours = Math.floor((now.getTime() - d.getTime()) / 3_600_000);
  if (hours < 1) return "now";
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  return `${Math.floor(days / 7)}w`;
}
