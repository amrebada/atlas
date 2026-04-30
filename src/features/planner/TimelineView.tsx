import { useEffect, useMemo, useState } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { Icon } from "../../components/Icon";
import {
  icsExportAll,
  icsRevealDir,
  listProjects,
  timelinePinProject,
  timelineQuery,
  timelineSetRange,
  timelineUnpinProject,
} from "../../ipc";
import { useUiStore } from "../../state/store";
import type {
  Milestone,
  Project,
  RoutineInstance,
  TimelineData,
  TimelineRow,
} from "../../types";

const LABEL_W = 160;

/** Full-screen timeline modal. Mounts unconditionally; renders null
 *  when `timelineOpen` is false. Esc closes. */
export function TimelineView() {
  const open = useUiStore((s) => s.timelineOpen);
  const setOpen = useUiStore((s) => s.setTimelineOpen);
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();
  const [pickerOpen, setPickerOpen] = useState(false);

  useEffect(() => {
    if (!open) return undefined;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        if (pickerOpen) {
          setPickerOpen(false);
          return;
        }
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, setOpen, pickerOpen]);

  const { data, isLoading } = useQuery<TimelineData>({
    queryKey: ["timeline-query"],
    queryFn: () => timelineQuery(),
    enabled: open,
    staleTime: 5_000,
    retry: false,
  });

  const setRangeMut = useMutation({
    mutationFn: (range: "week" | "month") => timelineSetRange(range),
    onSuccess: async () => {
      await queryClient.refetchQueries({ queryKey: ["timeline-query"] });
    },
    onError: (e) => pushToast("error", `Couldn't change range: ${String(e)}`),
  });

  const unpinMut = useMutation({
    mutationFn: (projectId: string) => timelineUnpinProject(projectId),
    onSuccess: async () => {
      await queryClient.refetchQueries({ queryKey: ["timeline-query"] });
    },
    onError: (e) => pushToast("error", `Couldn't unpin: ${String(e)}`),
  });

  const exportMut = useMutation({
    mutationFn: async () => {
      await icsExportAll();
      await icsRevealDir();
    },
    onSuccess: () => pushToast("success", "ICS files written and revealed in Finder"),
    onError: (e) => pushToast("error", `ICS export failed: ${String(e)}`),
  });

  const dates = useMemo(() => (data ? buildDates(data.start, data.end) : []), [data]);

  if (!open) return null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Timeline"
      className="fixed inset-0 z-40 bg-bg/80 backdrop-blur-sm flex items-stretch justify-center pt-8 pb-6 px-6"
      onClick={(e) => {
        if (e.target === e.currentTarget) setOpen(false);
      }}
    >
      <div className="w-[1100px] max-w-full bg-bg border border-line rounded-[10px] shadow-2xl flex flex-col overflow-hidden min-h-0">
        <header className="flex items-center gap-3 p-[12px] border-b border-line shrink-0">
          <Icon name="grid" size={14} stroke="var(--accent)" />
          <h1 className="text-[14px] font-semibold flex-1">Timeline</h1>

          <div className="inline-flex border border-line rounded-[5px] overflow-hidden">
            {(["week", "month"] as const).map((r) => {
              const active = data?.config.visibleRange === r;
              return (
                <button
                  key={r}
                  type="button"
                  onClick={() => setRangeMut.mutate(r)}
                  disabled={active}
                  className="px-[10px] py-[3px] text-[10px] font-mono uppercase tracking-[0.5px]"
                  style={{
                    background: active ? "var(--accent)" : "transparent",
                    color: active ? "var(--accent-fg)" : "var(--text-dim)",
                  }}
                >
                  {r}
                </button>
              );
            })}
          </div>

          <button
            type="button"
            onClick={() => setPickerOpen(true)}
            className="text-[10px] font-mono uppercase tracking-[0.5px] px-[8px] py-[3px] border border-line rounded-[4px] text-text-dim hover:text-text"
          >
            + pin project
          </button>

          <button
            type="button"
            onClick={() => exportMut.mutate()}
            disabled={exportMut.isPending}
            title="Write .ics files for every project + a combined feed, then reveal them in Finder"
            className="text-[10px] font-mono uppercase tracking-[0.5px] px-[8px] py-[3px] border border-line rounded-[4px] text-text-dim hover:text-text disabled:opacity-50"
          >
            {exportMut.isPending ? "exporting…" : "export ics"}
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
          <div className="flex-1 flex items-center justify-center text-text-dim text-[12px]">
            Loading…
          </div>
        ) : !data ? null : data.rows.length === 0 ? (
          <div className="flex-1 flex flex-col items-center justify-center gap-2 text-text-dim text-[12px]">
            <Icon name="grid" size={20} stroke="var(--text-dim)" />
            No projects pinned yet.
            <button
              type="button"
              onClick={() => setPickerOpen(true)}
              className="mt-2 px-[10px] py-[4px] bg-accent text-accent-fg rounded-[5px] text-[11px] font-semibold"
            >
              Pin a project
            </button>
          </div>
        ) : (
          <div className="flex-1 min-h-0 overflow-auto">
            <DateAxis dates={dates} today={data.today} />
            <div className="flex flex-col">
              {data.rows.map((row) => (
                <RowView
                  key={row.projectId}
                  row={row}
                  dates={dates}
                  start={data.start}
                  end={data.end}
                  today={data.today}
                  onUnpin={() => unpinMut.mutate(row.projectId)}
                />
              ))}
            </div>
          </div>
        )}
      </div>

      {pickerOpen && (
        <ProjectPicker
          pinnedIds={new Set(data?.rows.map((r) => r.projectId) ?? [])}
          onClose={() => setPickerOpen(false)}
          onPinned={async () => {
            // Force a fresh fetch *before* closing so the new row is on
            // screen the moment the picker disappears.
            await queryClient.refetchQueries({ queryKey: ["timeline-query"] });
            setPickerOpen(false);
          }}
        />
      )}
    </div>
  );
}

// ----- date axis -----

function buildDates(start: string, end: string): string[] {
  const out: string[] = [];
  const from = new Date(start + "T00:00:00Z");
  const to = new Date(end + "T00:00:00Z");
  for (
    let d = new Date(from);
    d.getTime() <= to.getTime();
    d.setUTCDate(d.getUTCDate() + 1)
  ) {
    out.push(d.toISOString().slice(0, 10));
  }
  return out;
}

function DateAxis({ dates, today }: { dates: string[]; today: string }) {
  return (
    <div
      className="flex border-b border-line bg-surface-2/30 sticky top-0 z-10"
      style={{ minWidth: LABEL_W + dates.length * 32 }}
    >
      <div
        className="shrink-0 border-r border-line"
        style={{ width: LABEL_W }}
      />
      <div className="flex flex-1">
        {dates.map((d) => {
          const isToday = d === today;
          const day = new Date(d + "T00:00:00Z");
          const dow = day.toLocaleDateString(undefined, {
            weekday: "narrow",
            timeZone: "UTC",
          });
          const dom = day.getUTCDate();
          return (
            <div
              key={d}
              className="flex-1 min-w-[32px] py-[4px] text-center font-mono"
              style={{
                background: isToday ? "var(--accent)" : "transparent",
                color: isToday ? "var(--accent-fg)" : "var(--text-dim)",
              }}
              title={d}
            >
              <div className="text-[9px] uppercase opacity-70">{dow}</div>
              <div className="text-[10px]">{dom}</div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ----- row -----

function RowView({
  row,
  dates,
  start,
  end,
  today,
  onUnpin,
}: {
  row: TimelineRow;
  dates: string[];
  start: string;
  end: string;
  today: string;
  onUnpin: () => void;
}) {
  const totalDays = dates.length;

  return (
    <div
      className="flex border-b border-line"
      style={{ minWidth: LABEL_W + totalDays * 32, height: 60 }}
    >
      <div
        className="shrink-0 border-r border-line flex flex-col justify-center px-[10px]"
        style={{ width: LABEL_W }}
      >
        <div className="flex items-center gap-2">
          <span
            className="w-[8px] h-[8px] rounded-full shrink-0"
            style={{ background: row.projectColor }}
          />
          <span className="text-[12px] font-semibold flex-1 truncate">
            {row.projectName}
          </span>
          <button
            type="button"
            onClick={onUnpin}
            title="Remove from timeline"
            aria-label="Unpin project"
            className="text-text-dimmer hover:text-err"
          >
            <Icon name="trash" size={11} stroke="currentColor" />
          </button>
        </div>
        <div className="text-[10px] font-mono text-text-dim mt-[2px]">
          {row.milestones.length}m · {row.routineInstances.length} dots
        </div>
      </div>

      <div className="relative flex-1 bg-bg/40">
        {/* Day grid lines */}
        <div className="absolute inset-0 flex pointer-events-none">
          {dates.map((d) => (
            <div
              key={d}
              className="flex-1 border-r border-line-soft last:border-r-0"
              style={{
                background: d === today ? "var(--accent-mute, rgba(59,130,246,0.08))" : "transparent",
              }}
            />
          ))}
        </div>

        {/* Milestone bars */}
        {row.milestones.map((m) => (
          <MilestoneBar
            key={m.id}
            milestone={m}
            color={row.projectColor}
            start={start}
            end={end}
            today={today}
          />
        ))}

        {/* Routine instance dots */}
        {row.routineInstances.map((inst) => (
          <RoutineDot
            key={inst.id}
            instance={inst}
            color={row.projectColor}
            start={start}
            end={end}
            today={today}
          />
        ))}
      </div>
    </div>
  );
}

// ----- bar -----

function MilestoneBar({
  milestone,
  color,
  start,
  end,
  today,
}: {
  milestone: Milestone;
  color: string;
  start: string;
  end: string;
  today: string;
}) {
  // Bar: from creation (clamped to window start) to deadline (clamped
  // to window end). Skip rendering if the milestone ends before the
  // window starts.
  const winStart = isoToTs(start);
  const winEnd = isoToTs(end);
  const dl = isoToTs(milestone.deadline);
  const created = isoToTs(milestone.createdAt.slice(0, 10));
  if (dl < winStart) return null;

  const barStart = Math.max(created, winStart);
  const barEnd = Math.min(dl, winEnd);
  if (barEnd <= barStart) return null;

  const totalSpan = winEnd - winStart;
  const leftPct = ((barStart - winStart) / totalSpan) * 100;
  const widthPct = ((barEnd - barStart) / totalSpan) * 100;

  const health = healthFor(milestone, today);

  return (
    <div
      className="absolute rounded-[4px] cursor-default group"
      style={{
        left: `${leftPct}%`,
        width: `calc(${widthPct}% + 2px)`,
        top: 8,
        height: 20,
        background: color,
        opacity: milestone.status === "done" ? 0.4 : 0.85,
      }}
      title={`${milestone.title} — ${milestone.deadline} (${health.label})`}
    >
      <div className="absolute inset-0 flex items-center px-[6px] gap-1 overflow-hidden">
        <span className="text-[10px] font-semibold truncate text-white drop-shadow">
          {milestone.title}
        </span>
        <span className="flex-1" />
        <span
          className="w-[6px] h-[6px] rounded-full shrink-0"
          style={{ background: health.color }}
        />
      </div>
    </div>
  );
}

// ----- dot -----

function RoutineDot({
  instance,
  color,
  start,
  end,
  today,
}: {
  instance: RoutineInstance;
  color: string;
  start: string;
  end: string;
  today: string;
}) {
  const winStart = isoToTs(start);
  const winEnd = isoToTs(end);
  const sched = isoToTs(instance.scheduledFor);
  if (sched < winStart || sched > winEnd) return null;

  const totalSpan = winEnd - winStart;
  const leftPct = ((sched - winStart) / totalSpan) * 100;

  const filled = !!instance.doneAt;
  const skipped = !!instance.skipped;
  const missed = instance.failingPoints > 0;
  const overdue = !filled && !skipped && instance.scheduledFor < today;

  const ringColor = missed
    ? "var(--err, #ef4444)"
    : overdue
      ? "var(--warn, #f59e0b)"
      : color;

  return (
    <div
      className="absolute"
      style={{
        left: `calc(${leftPct}% + 1px)`,
        bottom: 6,
        transform: "translateX(-50%)",
      }}
      title={`${instance.scheduledFor} — ${
        filled ? "done" : skipped ? "skipped" : missed ? "missed" : overdue ? "overdue" : "upcoming"
      }`}
    >
      <span
        className="block rounded-full"
        style={{
          width: 10,
          height: 10,
          background: filled ? color : "transparent",
          border: `1.5px solid ${ringColor}`,
          opacity: skipped ? 0.4 : 1,
        }}
      />
    </div>
  );
}

// ----- project picker (modal-in-modal) -----

function ProjectPicker({
  pinnedIds,
  onClose,
  onPinned,
}: {
  pinnedIds: Set<string>;
  onClose: () => void;
  onPinned: () => void | Promise<void>;
}) {
  const { data: projects = [] } = useQuery<Project[]>({
    queryKey: ["projects"],
    queryFn: listProjects,
  });
  const [filter, setFilter] = useState("");
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);

  const pinMut = useMutation({
    mutationFn: (id: string) => timelinePinProject(id),
    onSuccess: async () => {
      // refetch first so the parent's timeline data already includes
      // the new row by the time `onPinned` closes the picker.
      await queryClient.refetchQueries({ queryKey: ["timeline-query"] });
      await onPinned();
    },
    onError: (e) => pushToast("error", `Couldn't pin: ${String(e)}`),
  });

  const visible = projects
    .filter((p) => !pinnedIds.has(p.id))
    .filter((p) => {
      if (!filter) return true;
      const q = filter.toLowerCase();
      return (
        p.name.toLowerCase().includes(q) ||
        p.path.toLowerCase().includes(q)
      );
    })
    .slice(0, 50);

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Pin project to timeline"
      className="fixed inset-0 z-50 bg-bg/85 backdrop-blur-sm flex items-start justify-center pt-24"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-[420px] bg-bg border border-line rounded-[10px] shadow-2xl flex flex-col overflow-hidden">
        <header className="flex items-center gap-2 p-[12px] border-b border-line">
          <Icon name="search" size={12} stroke="var(--text-dim)" />
          <input
            autoFocus
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Filter projects…"
            className="flex-1 bg-transparent outline-none text-[12px]"
          />
          <button
            type="button"
            onClick={onClose}
            className="text-text-dim hover:text-text text-[11px]"
          >
            Cancel
          </button>
        </header>
        <ul className="max-h-[320px] overflow-y-auto">
          {visible.length === 0 ? (
            <li className="px-[12px] py-[10px] text-[11px] text-text-dim italic">
              No matching projects.
            </li>
          ) : (
            visible.map((p) => (
              <li key={p.id}>
                <button
                  type="button"
                  onClick={() => pinMut.mutate(p.id)}
                  className="w-full flex items-center gap-2 px-[12px] py-[6px] hover:bg-surface-2 text-left"
                >
                  <span
                    className="w-[8px] h-[8px] rounded-full shrink-0"
                    style={{ background: p.color }}
                  />
                  <span className="text-[12px] flex-1 truncate">{p.name}</span>
                  <span
                    className="font-mono text-[10px] text-text-dim truncate max-w-[55%]"
                    title={p.path}
                  >
                    {tildify(p.path)}
                  </span>
                </button>
              </li>
            ))
          )}
        </ul>
      </div>
    </div>
  );
}

// ----- helpers -----

function isoToTs(iso: string): number {
  return new Date(iso + "T00:00:00Z").getTime();
}

/** Replace `$HOME` (best-effort detected from any project path) with `~`
 *  so long absolute paths fit in the picker row. Falls back to the raw
 *  path if we can't infer the home dir. */
function tildify(path: string): string {
  // Best-effort: a typical macOS path starts with /Users/<name>/...
  const m = path.match(/^(\/Users\/[^/]+)/);
  if (m) return path.replace(m[1], "~");
  return path;
}

function healthFor(m: Milestone, today: string): { color: string; label: string } {
  if (m.status === "done") return { color: "var(--accent)", label: "done" };
  if (m.status === "missed") return { color: "var(--err, #ef4444)", label: "missed" };
  if (m.status === "cancelled")
    return { color: "var(--text-dim)", label: "cancelled" };
  const todayDate = new Date(today + "T00:00:00Z").getTime();
  const dl = new Date(m.deadline + "T00:00:00Z").getTime();
  const days = Math.round((dl - todayDate) / 86_400_000);
  if (days < 0) return { color: "var(--err, #ef4444)", label: `${-days}d overdue` };
  if (days === 0) return { color: "#f97316", label: "today" };
  if (days <= 2) return { color: "#3b82f6", label: `${days}d left` };
  return { color: "var(--accent)", label: `${days}d left` };
}
