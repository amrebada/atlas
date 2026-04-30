import type { Priority, TodayItem } from "../../types";

export const PRIORITY_COLOR: Record<Priority, string> = {
  p0: "var(--err, #ef4444)",
  p1: "var(--warn, #f59e0b)",
  p2: "var(--accent, #3b82f6)",
  p3: "var(--text-dim, #6b7280)",
};

export function itemKey(item: TodayItem): string {
  return `${item.kind}:${item.id}`;
}

export function itemTitle(item: TodayItem): string {
  switch (item.kind) {
    case "todo":
      return item.text;
    case "milestone-deadline":
      return item.title;
    case "routine-instance":
      return item.title;
  }
}

export function itemSubtitle(item: TodayItem): string {
  switch (item.kind) {
    case "todo": {
      if (item.daysOverdue > 0) return `${item.daysOverdue}d overdue · ${item.projectName}`;
      if (item.deadline) return `due ${item.deadline} · ${item.projectName}`;
      return item.projectName;
    }
    case "milestone-deadline":
      return `milestone · ${item.deadline} · ${item.projectName}`;
    case "routine-instance":
      return [
        "routine",
        item.scheduledFor,
        item.projectName ?? "global",
      ].join(" · ");
  }
}

export function itemPriority(item: TodayItem): Priority {
  return item.priority;
}

export function itemKindLabel(item: TodayItem): string {
  return item.kind === "milestone-deadline"
    ? "milestone"
    : item.kind === "routine-instance"
      ? "routine"
      : "todo";
}
