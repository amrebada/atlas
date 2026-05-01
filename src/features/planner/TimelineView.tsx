import { useEffect, useMemo, useState } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { Tooltip } from "react-tooltip";

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
  Routine,
  RoutineInstance,
  TimelineData,
  TimelineRow,
} from "../../types";

const LABEL_W = 168;
const TOOLTIP_ID = "atlas-timeline-tip";

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

  const gridTemplate = `${LABEL_W}px repeat(${dates.length}, minmax(36px, 1fr))`;

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
            <div
              className="grid items-stretch"
              style={{ gridTemplateColumns: gridTemplate, minWidth: LABEL_W + dates.length * 36 }}
            >
              <DateAxis dates={dates} today={data.today} />
              {data.rows.map((row) => (
                <RowView
                  key={row.projectId}
                  row={row}
                  dates={dates}
                  today={data.today}
                  onUnpin={() => unpinMut.mutate(row.projectId)}
                />
              ))}
            </div>
          </div>
        )}
      </div>

      {/* Single tooltip instance for every dot/bar in the grid. */}
      <Tooltip
        id={TOOLTIP_ID}
        place="top"
        delayShow={80}
        delayHide={50}
        opacity={1}
        style={{
          background: "var(--surface-2, #1f2937)",
          color: "var(--text, #e5e7eb)",
          border: "1px solid var(--line, #374151)",
          borderRadius: 6,
          padding: "8px 10px",
          fontSize: 11,
          maxWidth: 280,
          zIndex: 60,
          boxShadow: "0 8px 24px rgba(0,0,0,0.45)",
        }}
        render={({ content }) => {
          if (typeof content !== "string" || !content) return null;
          try {
            const payload = JSON.parse(content) as TipPayload;
            return <TipBody payload={payload} />;
          } catch {
            return <span>{content}</span>;
          }
        }}
      />

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
    <>
      <div
        className="border-r border-b border-line bg-surface-2/30 sticky top-0 z-10"
        style={{ gridColumn: "1 / span 1" }}
      />
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
            className="border-r border-b border-line py-[4px] text-center font-mono bg-surface-2/30 sticky top-0 z-10"
            style={{
              background: isToday ? "var(--accent)" : undefined,
              color: isToday ? "var(--accent-fg)" : "var(--text-dim)",
            }}
            title={d}
          >
            <div className="text-[9px] uppercase opacity-70">{dow}</div>
            <div className="text-[10px]">{dom}</div>
          </div>
        );
      })}
    </>
  );
}

// ----- row -----

function RowView({
  row,
  dates,
  today,
  onUnpin,
}: {
  row: TimelineRow;
  dates: string[];
  today: string;
  onUnpin: () => void;
}) {
  // Index routines + instances by date for O(1) per-cell lookup.
  const routineById = useMemo(() => {
    const map = new Map<string, Routine>();
    for (const r of row.routines) map.set(r.id, r);
    return map;
  }, [row.routines]);

  const instancesByDate = useMemo(() => {
    const map = new Map<string, RoutineInstance[]>();
    for (const inst of row.routineInstances) {
      const list = map.get(inst.scheduledFor);
      if (list) list.push(inst);
      else map.set(inst.scheduledFor, [inst]);
    }
    return map;
  }, [row.routineInstances]);

  // Milestones that intersect the visible window — split per cell
  // (one segment per day so it lines up with the grid).
  const milestoneSegments = useMemo(
    () => buildMilestoneSegments(row.milestones, dates),
    [row.milestones, dates],
  );

  const totalDots = row.routineInstances.length;
  const totalMilestones = row.milestones.length;

  return (
    <>
      <div
        className="border-r border-b border-line flex flex-col justify-center px-[10px] py-[6px] bg-bg/60"
        style={{ gridColumn: "1 / span 1" }}
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
          {totalMilestones}m · {totalDots} dots
        </div>
      </div>

      {dates.map((d) => {
        const isToday = d === today;
        const dayInstances = instancesByDate.get(d) ?? [];
        const segs = milestoneSegments.filter((s) => s.date === d);
        return (
          <div
            key={d}
            className="border-r border-b border-line relative flex flex-col justify-end items-center gap-[2px] py-[4px] px-[2px] min-h-[60px]"
            style={{
              background: isToday ? "var(--accent-mute, rgba(59,130,246,0.08))" : undefined,
            }}
          >
            {/* Milestone segment(s): one strip per milestone occupying this day. */}
            <div className="absolute left-0 right-0 top-[6px] flex flex-col gap-[2px] pointer-events-none">
              {segs.map((seg) => (
                <MilestoneSegment
                  key={seg.milestone.id}
                  segment={seg}
                  projectName={row.projectName}
                  projectColor={row.projectColor}
                  today={today}
                />
              ))}
            </div>

            {/* Routine dots: stacked, click target with rich tooltip. */}
            <div className="flex flex-row flex-wrap gap-[2px] justify-center mt-auto">
              {dayInstances.map((inst) => {
                const routine = routineById.get(inst.routineId);
                return (
                  <RoutineDot
                    key={inst.id}
                    instance={inst}
                    routine={routine}
                    projectName={row.projectName}
                    color={row.projectColor}
                    today={today}
                  />
                );
              })}
            </div>
          </div>
        );
      })}
    </>
  );
}

// ----- milestone bar segments -----

type MilestoneSegment = {
  date: string;
  milestone: Milestone;
  isStart: boolean;
  isEnd: boolean;
};

function buildMilestoneSegments(
  milestones: Milestone[],
  dates: string[],
): MilestoneSegment[] {
  if (dates.length === 0) return [];
  const winStart = dates[0]!;
  const winEnd = dates[dates.length - 1]!;
  const inWindow = (d: string) => d >= winStart && d <= winEnd;

  const out: MilestoneSegment[] = [];
  for (const m of milestones) {
    const created = m.createdAt.slice(0, 10);
    const startDate = created < winStart ? winStart : created;
    const endDate = m.deadline > winEnd ? winEnd : m.deadline;
    if (m.deadline < winStart) continue;
    if (startDate > winEnd) continue;
    for (const d of dates) {
      if (d < startDate || d > endDate) continue;
      if (!inWindow(d)) continue;
      out.push({
        date: d,
        milestone: m,
        isStart: d === startDate,
        isEnd: d === endDate,
      });
    }
  }
  return out;
}

function MilestoneSegment({
  segment,
  projectName,
  projectColor,
  today,
}: {
  segment: MilestoneSegment;
  projectName: string;
  projectColor: string;
  today: string;
}) {
  const { milestone, isStart, isEnd } = segment;
  const health = healthFor(milestone, today);
  const tip: TipPayload = {
    kind: "milestone",
    title: milestone.title,
    project: projectName,
    deadline: milestone.deadline,
    status: milestone.status,
    priority: milestone.priority,
    healthLabel: health.label,
    description: milestone.description ?? null,
  };
  const tipString = JSON.stringify(tip);
  return (
    <div
      data-tooltip-id={TOOLTIP_ID}
      data-tooltip-content={tipString}
      className="h-[14px] pointer-events-auto cursor-default flex items-center"
      style={{
        background: projectColor,
        opacity: milestone.status === "done" ? 0.4 : 0.85,
        borderTopLeftRadius: isStart ? 4 : 0,
        borderBottomLeftRadius: isStart ? 4 : 0,
        borderTopRightRadius: isEnd ? 4 : 0,
        borderBottomRightRadius: isEnd ? 4 : 0,
        marginLeft: isStart ? 1 : 0,
        marginRight: isEnd ? 1 : 0,
      }}
    >
      {isStart && (
        <span className="text-[9px] font-semibold text-white truncate px-[4px] drop-shadow whitespace-nowrap">
          {milestone.title}
        </span>
      )}
      {isEnd && (
        <span
          className="ml-auto w-[6px] h-[6px] rounded-full shrink-0 mr-[3px]"
          style={{ background: health.color }}
        />
      )}
    </div>
  );
}

// ----- routine dot -----

function RoutineDot({
  instance,
  routine,
  projectName,
  color,
  today,
}: {
  instance: RoutineInstance;
  routine: Routine | undefined;
  projectName: string;
  color: string;
  today: string;
}) {
  const filled = !!instance.doneAt;
  const skipped = !!instance.skipped;
  const missed = instance.failingPoints > 0;
  const overdue = !filled && !skipped && instance.scheduledFor < today;

  const status: TipStatus = filled
    ? "done"
    : skipped
      ? "skipped"
      : missed
        ? "missed"
        : overdue
          ? "overdue"
          : "upcoming";

  const ringColor = missed
    ? "var(--err, #ef4444)"
    : overdue
      ? "var(--warn, #f59e0b)"
      : color;

  const tip: TipPayload = {
    kind: "routine",
    title: routine?.title ?? "Routine instance",
    project: projectName,
    scheduledFor: instance.scheduledFor,
    status,
    priority: routine?.priority ?? "p2",
    description: routine?.description ?? null,
    rrule: routine?.rrule ?? null,
    doneAt: instance.doneAt ?? null,
  };
  const tipString = JSON.stringify(tip);

  return (
    <span
      data-tooltip-id={TOOLTIP_ID}
      data-tooltip-content={tipString}
      className="block rounded-full cursor-default"
      style={{
        width: 10,
        height: 10,
        background: filled ? color : "transparent",
        border: `1.5px solid ${ringColor}`,
        opacity: skipped ? 0.4 : 1,
      }}
    />
  );
}

// ----- tooltip body -----

type TipStatus = "done" | "skipped" | "missed" | "overdue" | "upcoming";

type TipPayload =
  | {
      kind: "routine";
      title: string;
      project: string;
      scheduledFor: string;
      status: TipStatus;
      priority: string;
      description: string | null;
      rrule: string | null;
      doneAt: string | null;
    }
  | {
      kind: "milestone";
      title: string;
      project: string;
      deadline: string;
      status: string;
      priority: string;
      healthLabel: string;
      description: string | null;
    };

function TipBody({ payload }: { payload: TipPayload }) {
  if (payload.kind === "routine") {
    return (
      <div className="flex flex-col gap-[4px] min-w-[180px]">
        <div className="text-[12px] font-semibold leading-tight">
          {payload.title}
        </div>
        <div className="text-[10px] text-text-dim font-mono uppercase tracking-[0.4px]">
          routine · {payload.priority} · {payload.status}
        </div>
        <div className="text-[11px]">
          <span className="text-text-dim">Project</span> · {payload.project}
        </div>
        <div className="text-[11px]">
          <span className="text-text-dim">Scheduled</span> ·{" "}
          {payload.scheduledFor}
        </div>
        {payload.doneAt && (
          <div className="text-[11px]">
            <span className="text-text-dim">Done</span> ·{" "}
            {payload.doneAt.slice(0, 16).replace("T", " ")}
          </div>
        )}
        {payload.rrule && (
          <div className="text-[10px] font-mono text-text-dim truncate max-w-[260px]">
            {payload.rrule}
          </div>
        )}
        {payload.description && (
          <div className="text-[11px] text-text-dim">{payload.description}</div>
        )}
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-[4px] min-w-[180px]">
      <div className="text-[12px] font-semibold leading-tight">
        {payload.title}
      </div>
      <div className="text-[10px] text-text-dim font-mono uppercase tracking-[0.4px]">
        milestone · {payload.priority} · {payload.healthLabel}
      </div>
      <div className="text-[11px]">
        <span className="text-text-dim">Project</span> · {payload.project}
      </div>
      <div className="text-[11px]">
        <span className="text-text-dim">Deadline</span> · {payload.deadline}
      </div>
      <div className="text-[11px]">
        <span className="text-text-dim">Status</span> · {payload.status}
      </div>
      {payload.description && (
        <div className="text-[11px] text-text-dim">{payload.description}</div>
      )}
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
