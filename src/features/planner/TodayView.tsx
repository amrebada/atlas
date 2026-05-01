import { useEffect, useMemo, useState } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { Icon } from "../../components/Icon";
import {
  completeRoutineInstance,
  plannerPauseAll,
  plannerToday,
  toggleTodo,
} from "../../ipc";
import { useUiStore } from "../../state/store";
import type { PlannerToday, TodayItem } from "../../types";
import { TodayItemRow } from "./TodayPanel";
import { itemKey, itemTitle } from "./TodayShared";
import { SuccessRateDial } from "./SuccessRateDial";
import { ExtensionLogPeek } from "./ExtensionLogPeek";
import { ShutdownFlow } from "./ShutdownFlow";

const DAILY_CAPACITY_MIN = 360;

/** Full-screen Today modal. Mounted unconditionally — renders null
 *  when `todayOpen` is false. Esc closes it. */
export function TodayView() {
  const open = useUiStore((s) => s.todayOpen);
  const setOpen = useUiStore((s) => s.setTodayOpen);
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();
  const [shutdownOpen, setShutdownOpen] = useState(false);

  useEffect(() => {
    if (!open) return undefined;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, setOpen]);

  const { data, isLoading, refetch } = useQuery<PlannerToday>({
    queryKey: ["planner-today"],
    queryFn: plannerToday,
    enabled: open,
    staleTime: 5_000,
    retry: false,
  });

  const completeTodoMut = useMutation({
    mutationFn: (vars: { projectId: string; todoId: string }) =>
      toggleTodo(vars.projectId, vars.todoId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["planner-today"] });
    },
    onError: (e) => pushToast("error", `Couldn't mark done: ${String(e)}`),
  });

  const completeRoutineMut = useMutation({
    mutationFn: (id: string) => completeRoutineInstance(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["planner-today"] });
    },
    onError: (e) => pushToast("error", `Couldn't mark done: ${String(e)}`),
  });

  const pauseMut = useMutation({
    mutationFn: (paused: boolean) => plannerPauseAll(paused),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["planner-today"] });
      void refetch();
    },
    onError: (e) => pushToast("error", `Couldn't toggle pause: ${String(e)}`),
  });

  const completeAction = (item: TodayItem) => {
    if (item.kind === "todo") {
      completeTodoMut.mutate({ projectId: item.projectId, todoId: item.id });
    } else if (item.kind === "routine-instance") {
      completeRoutineMut.mutate(item.id);
    }
    // Milestones aren't completed from the Today view — the user owns
    // that flow inside the milestone detail page.
  };

  const totalEst = data?.totalEstimateMinutes ?? 0;
  const capPct = useMemo(
    () => Math.min(100, Math.round((totalEst / DAILY_CAPACITY_MIN) * 100)),
    [totalEst],
  );

  const dateLabel = useMemo(() => {
    return new Date().toLocaleDateString(undefined, {
      weekday: "long",
      month: "short",
      day: "numeric",
    });
  }, []);

  if (!open) return null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Today"
      className="fixed inset-0 z-40 bg-bg/80 backdrop-blur-sm flex items-start justify-center pt-12 pb-8 overflow-auto"
      onClick={(e) => {
        if (e.target === e.currentTarget) setOpen(false);
      }}
    >
      <div className="w-[640px] max-w-[92vw] bg-bg border border-line rounded-[10px] shadow-2xl flex flex-col">
        <header className="flex items-center gap-3 p-[12px] border-b border-line">
          <Icon name="square-check" size={14} stroke="var(--accent)" />
          <h1 className="text-[14px] font-semibold flex-1">Today · {dateLabel}</h1>
          {data?.pausedAll && (
            <span className="font-mono text-[10px] uppercase text-warn">
              paused-all
            </span>
          )}
          <button
            type="button"
            onClick={() => pauseMut.mutate(!(data?.pausedAll ?? false))}
            className="text-[10px] font-mono uppercase tracking-[0.5px] px-[8px] py-[3px] border border-line rounded-[4px] text-text-dim hover:text-text"
            title="Pause-all suspends failing-point accrual"
          >
            {data?.pausedAll ? "resume" : "pause-all"}
          </button>
          <button
            type="button"
            onClick={() => setShutdownOpen(true)}
            disabled={(data?.mustDo.length ?? 0) === 0}
            className="text-[10px] font-mono uppercase tracking-[0.5px] px-[8px] py-[3px] border border-line rounded-[4px] text-text-dim hover:text-text disabled:opacity-40"
            title="Walk through unfinished must-do items"
          >
            shutdown
          </button>
          <button
            type="button"
            onClick={() => setOpen(false)}
            aria-label="Close"
            className="w-[24px] h-[24px] inline-flex items-center justify-center text-text-dim hover:text-text"
          >
            ✕
          </button>
        </header>

        {isLoading && !data ? (
          <div className="px-[18px] py-[16px] text-text-dim text-[12px]">
            Loading…
          </div>
        ) : !data ? null : (
          <>
            {data.topPriority && (
              <section className="px-[18px] py-[12px] border-b border-line bg-surface-2/40">
                <div className="text-[9px] font-mono uppercase tracking-[0.5px] text-text-dim mb-1">
                  ★ Top priority
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-[13px] font-semibold flex-1 truncate">
                    {itemTitle(data.topPriority)}
                  </span>
                  {(data.topPriority.kind === "todo" ||
                    data.topPriority.kind === "routine-instance") && (
                    <button
                      type="button"
                      onClick={() => completeAction(data.topPriority!)}
                      className="text-[11px] font-semibold px-[10px] py-[3px] bg-accent text-accent-fg rounded-[4px]"
                    >
                      Mark done
                    </button>
                  )}
                </div>
              </section>
            )}

            {data.soonestMilestone && data.soonestMilestoneTodos.length > 0 && (
              <section className="border-b border-line">
                <div className="px-[18px] py-[6px] flex items-center gap-2 bg-surface-2/30 border-b border-line-soft">
                  <span className="text-[10px] font-mono uppercase tracking-[0.5px] text-text-dim flex-1">
                    Closing soon · {data.soonestMilestone.title}
                  </span>
                  <span
                    className="font-mono text-[10px]"
                    style={{
                      color:
                        data.soonestMilestone.daysLeft < 0
                          ? "var(--err, #ef4444)"
                          : data.soonestMilestone.daysLeft <= 2
                            ? "var(--warn, #f59e0b)"
                            : "var(--text-dim)",
                    }}
                    title={`Deadline: ${data.soonestMilestone.deadline}`}
                  >
                    {data.soonestMilestone.daysLeft < 0
                      ? `${-data.soonestMilestone.daysLeft}d overdue`
                      : data.soonestMilestone.daysLeft === 0
                        ? "today"
                        : `${data.soonestMilestone.daysLeft}d left`}
                  </span>
                  <span className="font-mono text-[10px] text-text-dim">
                    {data.soonestMilestoneTodos.length}
                  </span>
                </div>
                <div>
                  {data.soonestMilestoneTodos.map((item) => (
                    <TodayItemRow
                      key={itemKey(item)}
                      item={item}
                      onAction={
                        item.kind === "todo" ? () => completeAction(item) : undefined
                      }
                    />
                  ))}
                </div>
              </section>
            )}

            <Section title="Must do today" count={data.mustDo.length}>
              {data.mustDo.length === 0 ? (
                <Empty message="Nothing locked in. Clean slate." />
              ) : (
                data.mustDo.map((item) => (
                  <TodayItemRow
                    key={itemKey(item)}
                    item={item}
                    onAction={
                      item.kind === "todo" || item.kind === "routine-instance"
                        ? () => completeAction(item)
                        : undefined
                    }
                  />
                ))
              )}
            </Section>

            <Section title="Could do today" count={data.couldDo.length}>
              {data.couldDo.length === 0 ? (
                <Empty message="Nothing in the bench." />
              ) : (
                data.couldDo
                  .slice(0, 12)
                  .map((item) => (
                    <TodayItemRow
                      key={itemKey(item)}
                      item={item}
                      onAction={
                        item.kind === "todo" ||
                        item.kind === "routine-instance"
                          ? () => completeAction(item)
                          : undefined
                      }
                    />
                  ))
              )}
            </Section>

            <section className="px-[18px] py-[10px] border-t border-line">
              <div className="text-[9px] font-mono uppercase tracking-[0.5px] text-text-dim mb-1">
                Workload
              </div>
              <div className="flex items-center gap-2">
                <div className="flex-1 h-[6px] bg-surface-2 rounded-full overflow-hidden">
                  <div
                    className="h-full rounded-full transition-all"
                    style={{
                      width: `${capPct}%`,
                      background:
                        capPct < 80
                          ? "var(--accent)"
                          : capPct < 100
                            ? "var(--warn, #f59e0b)"
                            : "var(--err, #ef4444)",
                    }}
                  />
                </div>
                <span className="font-mono text-[10px] text-text-dim">
                  {fmtMin(totalEst)} / {fmtMin(DAILY_CAPACITY_MIN)}
                </span>
              </div>
            </section>

            <SuccessRateDial />

            <ExtensionLogPeek />

            {data.deadlinesTomorrow.length > 0 && (
              <section className="px-[18px] py-[10px] border-t border-line bg-warn/10">
                <div className="text-[9px] font-mono uppercase tracking-[0.5px] text-warn mb-1">
                  ▲ Deadlines tomorrow
                </div>
                {data.deadlinesTomorrow.map((item) => (
                  <div
                    key={itemKey(item)}
                    className="text-[12px] flex items-center gap-2 py-[2px]"
                  >
                    <span className="flex-1 truncate">{itemTitle(item)}</span>
                    <span className="font-mono text-[10px] text-text-dim">
                      {item.kind === "milestone-deadline"
                        ? item.deadline
                        : "tomorrow"}
                    </span>
                  </div>
                ))}
              </section>
            )}
          </>
        )}
      </div>

      <ShutdownFlow
        open={shutdownOpen}
        today={data ?? null}
        onClose={() => setShutdownOpen(false)}
      />
    </div>
  );
}

function Section({
  title,
  count,
  children,
}: {
  title: string;
  count: number;
  children: React.ReactNode;
}) {
  return (
    <section className="border-b border-line">
      <div className="px-[18px] py-[6px] flex items-center gap-2 bg-surface-2/30 border-b border-line-soft">
        <span className="text-[10px] font-mono uppercase tracking-[0.5px] text-text-dim flex-1">
          {title}
        </span>
        <span className="font-mono text-[10px] text-text-dim">{count}</span>
      </div>
      <div>{children}</div>
    </section>
  );
}

function Empty({ message }: { message: string }) {
  return (
    <div className="px-[18px] py-[10px] text-[11px] text-text-dim italic">
      {message}
    </div>
  );
}

function fmtMin(min: number): string {
  if (min <= 0) return "0m";
  const h = Math.floor(min / 60);
  const m = min % 60;
  if (h <= 0) return `${m}m`;
  if (m === 0) return `${h}h`;
  return `${h}h ${m}m`;
}
