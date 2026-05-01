import { useQuery } from "@tanstack/react-query";

import { Icon } from "../../components/Icon";
import { plannerToday } from "../../ipc";
import { useUiStore } from "../../state/store";
import type { PlannerToday, TodayItem } from "../../types";
import {
  PRIORITY_COLOR,
  itemKindLabel,
  itemSubtitle,
  itemTitle,
} from "./TodayShared";

/** TitleBar badge: small button showing the must-do count or a green
 *  check when the day is clear. Click opens the full Today modal.
 *  Lives in the title bar so it never collides with sidebar content. */
export function TodayBadge() {
  const setTodayOpen = useUiStore((s) => s.setTodayOpen);

  const { data, refetch } = useQuery<PlannerToday>({
    queryKey: ["planner-today"],
    queryFn: plannerToday,
    refetchInterval: 60_000,
    refetchOnWindowFocus: true,
    retry: false,
  });

  const mustCount = data?.mustDo.length ?? 0;
  const tomorrowCount = data?.deadlinesTomorrow.length ?? 0;
  const paused = data?.pausedAll ?? false;
  const clear = !!data && mustCount === 0 && tomorrowCount === 0 && !paused;

  const label =
    !data
      ? "Today"
      : paused
        ? "Today · paused"
        : mustCount > 0
          ? `${mustCount}`
          : tomorrowCount > 0
            ? `${tomorrowCount}`
            : "Today";

  const tone =
    paused
      ? "var(--warn, #f59e0b)"
      : mustCount > 0
        ? "var(--err, #ef4444)"
        : tomorrowCount > 0
          ? "var(--warn, #f59e0b)"
          : "var(--accent)";

  return (
    <button
      data-tauri-drag-region="false"
      onClick={() => {
        setTodayOpen(true);
        void refetch();
      }}
      title="Today (⌘T)"
      aria-label="Open Today"
      className="inline-flex items-center gap-[6px] px-[8px] h-6 bg-surface-2 border border-line rounded-[5px] text-[11px] whitespace-nowrap shrink-0 hover:text-text transition-colors"
      style={{ color: clear ? "var(--text-dim)" : tone }}
    >
      <Icon
        name={clear ? "check" : "clock"}
        size={11}
        stroke={clear ? "var(--accent)" : tone}
      />
      <span style={{ color: clear ? "var(--text-dim)" : tone }}>{label}</span>
    </button>
  );
}

/** Row renderer reused by `TodayView`. Shows priority chip + title +
 *  subtitle + an optional "done" button. */
export function TodayItemRow({
  item,
  onAction,
  actionLabel = "done",
  showSubtitle = true,
}: {
  item: TodayItem;
  onAction?: () => void;
  actionLabel?: string;
  showSubtitle?: boolean;
}) {
  return (
    <div className="flex items-center gap-2 px-[10px] py-[7px] border-b border-line-soft hover:bg-surface-2/40">
      <span
        className="font-mono text-[9px] uppercase tracking-[0.5px] w-[18px] shrink-0"
        style={{ color: PRIORITY_COLOR[item.priority] }}
      >
        {item.priority}
      </span>
      <div className="flex flex-col flex-1 min-w-0">
        <span className="text-[12px] truncate">{itemTitle(item)}</span>
        {showSubtitle && (
          <span className="text-[10px] text-text-dim truncate">
            {itemSubtitle(item)}
          </span>
        )}
      </div>
      <span className="font-mono text-[9px] uppercase text-text-dim shrink-0">
        {itemKindLabel(item)}
      </span>
      {onAction && (
        <button
          type="button"
          onClick={onAction}
          className="text-[10px] font-mono uppercase px-[6px] py-[2px] border border-line rounded-[3px] text-accent hover:bg-surface-2"
        >
          {actionLabel}
        </button>
      )}
    </div>
  );
}
