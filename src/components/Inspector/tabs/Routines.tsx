import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { Icon } from "../../Icon";
import { TabEmpty, TabError, TabSkeleton } from "../TabStates";
import { useUiStore } from "../../../state/store";
import {
  completeRoutineInstance,
  createRoutine,
  deleteRoutine,
  listRoutineInstances,
  listRoutines,
  pauseRoutine,
  projectedCompletion,
  skipRoutineInstance,
  updateRoutine,
} from "../../../ipc";
import type {
  Goal,
  Priority,
  Project,
  Routine,
  RoutineInstance,
} from "../../../types";

interface RoutinesProps {
  project: Project;
}

type Mode = { kind: "list" } | { kind: "detail"; id: string } | { kind: "create" };

const PRIORITY_OPTIONS: Array<{ value: Priority; label: string }> = [
  { value: "p0", label: "P0" },
  { value: "p1", label: "P1" },
  { value: "p2", label: "P2" },
  { value: "p3", label: "P3" },
];

const PRIORITY_COLOR: Record<Priority, string> = {
  p0: "var(--err, #ef4444)",
  p1: "var(--warn, #f59e0b)",
  p2: "var(--accent, #3b82f6)",
  p3: "var(--text-dim, #6b7280)",
};

type CadenceKind = "daily" | "every-n" | "weekdays";
type GoalKind = "count" | "deadline" | "indefinite";

const WEEKDAYS: Array<{ code: string; label: string }> = [
  { code: "MO", label: "Mon" },
  { code: "TU", label: "Tue" },
  { code: "WE", label: "Wed" },
  { code: "TH", label: "Thu" },
  { code: "FR", label: "Fri" },
  { code: "SA", label: "Sat" },
  { code: "SU", label: "Sun" },
];

export function Routines({ project }: RoutinesProps) {
  const [mode, setMode] = useState<Mode>({ kind: "list" });
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);

  useEffect(() => {
    setMode({ kind: "list" });
  }, [project.id]);

  const queryKey = useMemo(
    () => ["routines", project.id] as const,
    [project.id],
  );

  const routinesQ = useQuery<Routine[]>({
    queryKey: [...queryKey],
    queryFn: () => listRoutines(project.id),
    staleTime: 5_000,
    retry: false,
  });

  const routines = routinesQ.data ?? [];

  const createMut = useMutation({
    mutationFn: (r: Routine) => createRoutine(r),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: [...queryKey] });
      setMode({ kind: "list" });
    },
    onError: (e) => pushToast("error", `Couldn't create routine: ${String(e)}`),
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => deleteRoutine(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: [...queryKey] });
      setMode({ kind: "list" });
    },
    onError: (e) => pushToast("error", `Couldn't delete routine: ${String(e)}`),
  });

  const pauseMut = useMutation({
    mutationFn: (vars: { id: string; paused: boolean }) =>
      pauseRoutine(vars.id, vars.paused),
    onSuccess: () =>
      void queryClient.invalidateQueries({ queryKey: [...queryKey] }),
    onError: (e) => pushToast("error", `Couldn't pause routine: ${String(e)}`),
  });

  const updateMut = useMutation({
    mutationFn: (vars: {
      id: string;
      patch: Parameters<typeof updateRoutine>[1];
    }) => updateRoutine(vars.id, vars.patch),
    onSuccess: () =>
      void queryClient.invalidateQueries({ queryKey: [...queryKey] }),
    onError: (e) => pushToast("error", `Couldn't update routine: ${String(e)}`),
  });

  if (mode.kind === "create") {
    return (
      <CreateForm
        projectId={project.id}
        onCancel={() => setMode({ kind: "list" })}
        onSubmit={(r) => createMut.mutate(r)}
        pending={createMut.isPending}
      />
    );
  }

  if (mode.kind === "detail") {
    const r = routines.find((rr) => rr.id === mode.id);
    return (
      <Detail
        routine={r ?? null}
        onBack={() => setMode({ kind: "list" })}
        onPause={(paused) =>
          r && pauseMut.mutate({ id: r.id, paused })
        }
        onUpdate={(patch) => r && updateMut.mutate({ id: r.id, patch })}
        onDelete={() => r && deleteMut.mutate(r.id)}
      />
    );
  }

  return (
    <div className="p-[14px] overflow-y-auto h-full">
      <div className="flex items-center justify-between mb-2">
        <div className="text-[12px] text-text-dim">
          {routines.length} routine{routines.length === 1 ? "" : "s"}
        </div>
        <button
          type="button"
          onClick={() => setMode({ kind: "create" })}
          title="New routine"
          aria-label="New routine"
          className="inline-flex items-center gap-1 px-[8px] py-[3px] bg-accent text-accent-fg rounded-[4px] text-[10px] font-mono uppercase tracking-[0.5px]"
        >
          <Icon name="plus" size={11} stroke="var(--accent-fg)" />
          new
        </button>
      </div>

      {routinesQ.isLoading && !routinesQ.data && <TabSkeleton rows={3} />}
      {routinesQ.error && (
        <TabError
          message={String(routinesQ.error)}
          onRetry={() => void routinesQ.refetch()}
        />
      )}
      {!routinesQ.isLoading && !routinesQ.error && routines.length === 0 && (
        <TabEmpty
          icon="branch"
          title="No routines yet"
          hint="Press + to set up a recurring task — e.g. 'every 2 days, until 100'"
        />
      )}

      <div className="flex flex-col gap-2 mt-3">
        {routines.map((r) => (
          <RoutineCard
            key={r.id}
            routine={r}
            onClick={() => setMode({ kind: "detail", id: r.id })}
          />
        ))}
      </div>
    </div>
  );
}

// ----- subcomponents -----

function RoutineCard({
  routine,
  onClick,
}: {
  routine: Routine;
  onClick: () => void;
}) {
  const rate = routine.successPoints + routine.failingPoints <= 0
    ? 1
    : routine.successPoints / (routine.successPoints + routine.failingPoints);

  const goalLabel =
    routine.goal.kind === "count"
      ? `${routine.goal.completed}/${routine.goal.target}`
      : routine.goal.kind === "deadline"
        ? `until ${routine.goal.until}`
        : "ongoing";

  return (
    <button
      type="button"
      onClick={onClick}
      className="text-left p-[10px] bg-surface-2 border border-line rounded-[6px] hover:border-accent transition-colors"
    >
      <div className="flex items-center gap-2 mb-1">
        <Icon
          name="branch"
          size={11}
          stroke={routine.paused ? "var(--text-dim)" : "var(--accent)"}
        />
        <span
          className="font-mono text-[9px] uppercase tracking-[0.5px]"
          style={{ color: PRIORITY_COLOR[routine.priority] }}
        >
          {routine.priority}
        </span>
        <span
          className="font-semibold text-[12px] flex-1 truncate"
          style={{ color: routine.paused ? "var(--text-dim)" : "var(--text)" }}
        >
          {routine.title}
        </span>
        {routine.paused && (
          <span className="font-mono text-[9px] uppercase text-warn">paused</span>
        )}
      </div>
      <div className="flex items-center gap-3 text-[11px] text-text-dim">
        <span className="font-mono">{describeCadence(routine.rrule)}</span>
        <span className="text-text-dimmer">·</span>
        <span className="font-mono">{goalLabel}</span>
        <span className="flex-1" />
        <span
          className="font-mono"
          style={{
            color: rate >= 0.85 ? "var(--accent)" : rate >= 0.5 ? "var(--warn, #f59e0b)" : "var(--err, #ef4444)",
          }}
        >
          {Math.round(rate * 100)}%
        </span>
      </div>
    </button>
  );
}

function CreateForm({
  projectId,
  onCancel,
  onSubmit,
  pending,
}: {
  projectId: string;
  onCancel: () => void;
  onSubmit: (r: Routine) => void;
  pending: boolean;
}) {
  const [title, setTitle] = useState("");
  const [cadenceKind, setCadenceKind] = useState<CadenceKind>("daily");
  const [interval, setInterval] = useState(2);
  const [weekdays, setWeekdays] = useState<string[]>(["MO", "WE", "FR"]);
  const [startDate, setStartDate] = useState(() =>
    new Date().toISOString().slice(0, 10),
  );
  const [priority, setPriority] = useState<Priority>("p2");
  const [goalKind, setGoalKind] = useState<GoalKind>("count");
  const [target, setTarget] = useState(100);
  const [until, setUntil] = useState(() => {
    const d = new Date();
    d.setMonth(d.getMonth() + 3);
    return d.toISOString().slice(0, 10);
  });

  const rrule = useMemo(() => {
    if (cadenceKind === "daily") {
      return interval === 1
        ? "FREQ=DAILY"
        : `FREQ=DAILY;INTERVAL=${interval}`;
    }
    if (cadenceKind === "every-n") {
      return `FREQ=DAILY;INTERVAL=${Math.max(1, interval)}`;
    }
    return weekdays.length > 0
      ? `FREQ=WEEKLY;BYDAY=${weekdays.join(",")}`
      : "FREQ=DAILY";
  }, [cadenceKind, interval, weekdays]);

  const goal: Goal = useMemo(() => {
    if (goalKind === "count")
      return { kind: "count", target, completed: 0 };
    if (goalKind === "deadline") return { kind: "deadline", until };
    return { kind: "indefinite" };
  }, [goalKind, target, until]);

  const valid = title.trim().length > 0 && rrule.length > 0;

  return (
    <div className="p-[14px] flex flex-col gap-3 h-full overflow-y-auto">
      <div className="flex items-center justify-between">
        <span className="font-semibold text-[13px]">New routine</span>
        <button
          type="button"
          onClick={onCancel}
          className="text-text-dim hover:text-text text-[11px]"
        >
          Cancel
        </button>
      </div>

      <Field label="Title">
        <input
          autoFocus
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="e.g. ship a video for feature X"
          className="bg-surface-2 border border-line rounded-[5px] px-[8px] py-[5px] text-[12px] outline-none focus:border-accent"
        />
      </Field>

      <Field label="Cadence">
        <div className="flex gap-1 mb-2">
          <ChipBtn
            active={cadenceKind === "daily"}
            onClick={() => setCadenceKind("daily")}
          >
            Daily
          </ChipBtn>
          <ChipBtn
            active={cadenceKind === "every-n"}
            onClick={() => setCadenceKind("every-n")}
          >
            Every N days
          </ChipBtn>
          <ChipBtn
            active={cadenceKind === "weekdays"}
            onClick={() => setCadenceKind("weekdays")}
          >
            Weekdays
          </ChipBtn>
        </div>
        {cadenceKind === "every-n" && (
          <div className="flex items-center gap-2">
            <span className="text-[11px] text-text-dim">every</span>
            <input
              type="number"
              min={1}
              max={365}
              value={interval}
              onChange={(e) => setInterval(parseInt(e.target.value) || 1)}
              className="w-[60px] bg-surface-2 border border-line rounded-[4px] px-[6px] py-[3px] text-[12px] outline-none focus:border-accent font-mono"
            />
            <span className="text-[11px] text-text-dim">days</span>
          </div>
        )}
        {cadenceKind === "weekdays" && (
          <div className="flex gap-1 flex-wrap">
            {WEEKDAYS.map((w) => {
              const on = weekdays.includes(w.code);
              return (
                <ChipBtn
                  key={w.code}
                  active={on}
                  onClick={() =>
                    setWeekdays((prev) =>
                      prev.includes(w.code)
                        ? prev.filter((c) => c !== w.code)
                        : [...prev, w.code],
                    )
                  }
                >
                  {w.label}
                </ChipBtn>
              );
            })}
          </div>
        )}
        <div className="text-[10px] font-mono text-text-dim mt-2">
          {rrule}
        </div>
      </Field>

      <Field label="Start date">
        <input
          type="date"
          value={startDate}
          onChange={(e) => setStartDate(e.target.value)}
          className="bg-surface-2 border border-line rounded-[5px] px-[8px] py-[5px] text-[12px] outline-none focus:border-accent font-mono w-fit"
        />
      </Field>

      <Field label="Goal">
        <div className="flex gap-1 mb-2">
          <ChipBtn
            active={goalKind === "count"}
            onClick={() => setGoalKind("count")}
          >
            Count
          </ChipBtn>
          <ChipBtn
            active={goalKind === "deadline"}
            onClick={() => setGoalKind("deadline")}
          >
            Until
          </ChipBtn>
          <ChipBtn
            active={goalKind === "indefinite"}
            onClick={() => setGoalKind("indefinite")}
          >
            Ongoing
          </ChipBtn>
        </div>
        {goalKind === "count" && (
          <div className="flex items-center gap-2">
            <span className="text-[11px] text-text-dim">stop after</span>
            <input
              type="number"
              min={1}
              value={target}
              onChange={(e) => setTarget(parseInt(e.target.value) || 1)}
              className="w-[80px] bg-surface-2 border border-line rounded-[4px] px-[6px] py-[3px] text-[12px] outline-none focus:border-accent font-mono"
            />
            <span className="text-[11px] text-text-dim">occurrences</span>
          </div>
        )}
        {goalKind === "deadline" && (
          <input
            type="date"
            value={until}
            onChange={(e) => setUntil(e.target.value)}
            className="bg-surface-2 border border-line rounded-[5px] px-[8px] py-[5px] text-[12px] outline-none focus:border-accent font-mono w-fit"
          />
        )}
      </Field>

      <Field label="Priority">
        <select
          value={priority}
          onChange={(e) => setPriority(e.target.value as Priority)}
          className="bg-surface-2 border border-line rounded-[5px] px-[8px] py-[5px] text-[12px] outline-none focus:border-accent w-fit"
        >
          {PRIORITY_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </Field>

      <button
        type="button"
        disabled={!valid || pending}
        onClick={() => {
          const now = new Date().toISOString();
          onSubmit({
            id: "",
            projectId,
            title: title.trim(),
            description: undefined,
            rrule,
            startDate,
            goal,
            priority,
            estimate: undefined,
            paused: false,
            pausedFrom: undefined,
            successPoints: 0,
            failingPoints: 0,
            extensions: [],
            createdAt: now,
          });
        }}
        className="self-start mt-1 px-[12px] py-[5px] bg-accent text-accent-fg rounded-[5px] text-[11px] font-semibold disabled:opacity-50"
      >
        {pending ? "Creating…" : "Create routine"}
      </button>
    </div>
  );
}

function Detail({
  routine,
  onBack,
  onPause,
  onUpdate,
  onDelete,
}: {
  routine: Routine | null;
  onBack: () => void;
  onPause: (paused: boolean) => void;
  onUpdate: (patch: Parameters<typeof updateRoutine>[1]) => void;
  onDelete: () => void;
}) {
  if (!routine) {
    return (
      <div className="p-[14px] text-text-dim text-[12px]">
        Routine gone.{" "}
        <button onClick={onBack} className="underline">
          Back to list
        </button>
      </div>
    );
  }

  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);

  // Look 14 days back, 14 days forward — gives the user history + horizon.
  const today = new Date();
  const from = useMemo(() => {
    const d = new Date(today);
    d.setDate(d.getDate() - 14);
    return d.toISOString().slice(0, 10);
  }, [routine.id]);
  const to = useMemo(() => {
    const d = new Date(today);
    d.setDate(d.getDate() + 14);
    return d.toISOString().slice(0, 10);
  }, [routine.id]);

  const instancesQ = useQuery<RoutineInstance[]>({
    queryKey: ["routine-instances", routine.id, from, to],
    queryFn: () => listRoutineInstances(routine.id, from, to),
    staleTime: 5_000,
    retry: false,
  });

  const projectionQ = useQuery<string | null>({
    queryKey: ["routine-projection", routine.id],
    queryFn: () => projectedCompletion(routine.id),
    staleTime: 5_000,
    retry: false,
  });

  const completeMut = useMutation({
    mutationFn: (id: string) => completeRoutineInstance(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: ["routine-instances", routine.id, from, to],
      });
      void queryClient.invalidateQueries({ queryKey: ["routines", routine.projectId ?? ""] });
      void queryClient.invalidateQueries({ queryKey: ["routine-projection", routine.id] });
    },
    onError: (e) => pushToast("error", `Mark done failed: ${String(e)}`),
  });

  const skipMut = useMutation({
    mutationFn: (id: string) => skipRoutineInstance(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: ["routine-instances", routine.id, from, to],
      });
    },
    onError: (e) => pushToast("error", `Skip failed: ${String(e)}`),
  });

  const instances = instancesQ.data ?? [];
  const upcoming = instances.filter(
    (i) => i.scheduledFor >= todayIso(),
  );
  const past = instances.filter((i) => i.scheduledFor < todayIso());
  const rate =
    routine.successPoints + routine.failingPoints <= 0
      ? 1
      : routine.successPoints / (routine.successPoints + routine.failingPoints);

  const completed =
    routine.goal.kind === "count" ? routine.goal.completed : null;
  const target = routine.goal.kind === "count" ? routine.goal.target : null;

  const projection = projectionQ.data;

  return (
    <div className="p-[14px] overflow-y-auto h-full flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={onBack}
          className="text-text-dim hover:text-text text-[11px] inline-flex items-center gap-1"
        >
          <Icon name="chevron" size={11} stroke="currentColor" />
          back
        </button>
      </div>

      <div>
        <input
          value={routine.title}
          onChange={(e) => onUpdate({ title: e.target.value })}
          className="w-full bg-transparent border-none outline-none text-[15px] font-semibold focus:border-b focus:border-accent"
        />
        <div className="text-[11px] font-mono text-text-dim mt-1">
          {describeCadence(routine.rrule)} · starts {routine.startDate}
        </div>
      </div>

      <div className="grid grid-cols-2 gap-3 p-[10px] bg-surface-2 border border-line rounded-[6px]">
        <Stat label="success" value={`${Math.round(rate * 100)}%`} color={
          rate >= 0.85 ? "var(--accent)" : rate >= 0.5 ? "var(--warn, #f59e0b)" : "var(--err, #ef4444)"
        } />
        {target !== null ? (
          <Stat label="progress" value={`${completed}/${target}`} />
        ) : (
          <Stat label="goal" value={routine.goal.kind} />
        )}
        <Stat label="success pts" value={Math.round(routine.successPoints).toString()} />
        <Stat label="fail pts" value={Math.round(routine.failingPoints).toString()} />
        {projection && (
          <div className="col-span-2 text-[11px] font-mono text-text-dim">
            projected completion: <span className="text-text">{projection}</span>
            {routine.extensions.length > 0 && (
              <span className="text-warn">
                {" "}— extended {routine.extensions.length}× from misses
              </span>
            )}
          </div>
        )}
      </div>

      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={() => onPause(!routine.paused)}
          className="px-[8px] py-[3px] text-[10px] font-mono uppercase tracking-[0.5px] border border-line rounded-[4px] text-text-dim hover:text-text"
        >
          {routine.paused ? "resume" : "pause"}
        </button>
        <span className="flex-1" />
        <button
          type="button"
          onClick={() => {
            if (
              confirm(
                `Delete routine "${routine.title}" and all its instances?`,
              )
            ) {
              onDelete();
            }
          }}
          className="px-[8px] py-[3px] text-[10px] font-mono uppercase tracking-[0.5px] border border-line rounded-[4px] text-text-dim hover:text-err"
        >
          delete
        </button>
      </div>

      <Section title={`Upcoming (${upcoming.length})`}>
        {upcoming.length === 0 ? (
          <div className="text-[11px] text-text-dim italic">
            None in the next 14 days.
          </div>
        ) : (
          upcoming.map((i) => (
            <InstanceRow
              key={i.id}
              instance={i}
              onComplete={() => completeMut.mutate(i.id)}
              onSkip={() => skipMut.mutate(i.id)}
            />
          ))
        )}
      </Section>

      <Section title={`History (${past.length})`}>
        {past.length === 0 ? (
          <div className="text-[11px] text-text-dim italic">
            No prior occurrences.
          </div>
        ) : (
          past
            .slice()
            .reverse()
            .map((i) => (
              <InstanceRow
                key={i.id}
                instance={i}
                onComplete={() => completeMut.mutate(i.id)}
                onSkip={() => skipMut.mutate(i.id)}
              />
            ))
        )}
      </Section>
    </div>
  );
}

function InstanceRow({
  instance,
  onComplete,
  onSkip,
}: {
  instance: RoutineInstance;
  onComplete: () => void;
  onSkip: () => void;
}) {
  const status = instance.doneAt
    ? "done"
    : instance.skipped
      ? "skipped"
      : instance.failingPoints > 0
        ? "missed"
        : instance.scheduledFor === todayIso()
          ? "today"
          : instance.scheduledFor < todayIso()
            ? "overdue"
            : "upcoming";

  const color = {
    done: "var(--accent)",
    skipped: "var(--text-dim)",
    missed: "var(--err, #ef4444)",
    today: "var(--warn, #f59e0b)",
    overdue: "var(--err, #ef4444)",
    upcoming: "var(--text-dim)",
  }[status];

  return (
    <div className="flex items-center gap-2 py-[5px] border-b border-line-soft">
      <span
        className="w-[8px] h-[8px] rounded-full"
        style={{ background: color }}
      />
      <span className="font-mono text-[11px] text-text-dim w-[90px]">
        {instance.scheduledFor}
      </span>
      <span
        className="text-[11px] flex-1 font-mono uppercase tracking-[0.5px]"
        style={{ color }}
      >
        {status}
      </span>
      {!instance.doneAt && !instance.skipped && (
        <>
          <button
            type="button"
            onClick={onComplete}
            title="Mark done"
            className="text-[10px] font-mono uppercase px-[6px] py-[2px] border border-line rounded-[3px] text-accent hover:bg-surface-2"
          >
            done
          </button>
          <button
            type="button"
            onClick={onSkip}
            title="Skip"
            className="text-[10px] font-mono uppercase px-[6px] py-[2px] border border-line rounded-[3px] text-text-dim hover:bg-surface-2"
          >
            skip
          </button>
        </>
      )}
    </div>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-[10px] uppercase font-mono text-text-dim tracking-[0.5px]">
        {label}
      </span>
      {children}
    </label>
  );
}

function ChipBtn({
  children,
  active,
  onClick,
}: {
  children: React.ReactNode;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="px-[8px] py-[3px] text-[10px] font-mono uppercase tracking-[0.5px] border rounded-[4px]"
      style={{
        background: active ? "var(--accent)" : "transparent",
        color: active ? "var(--accent-fg)" : "var(--text-dim)",
        borderColor: active ? "var(--accent)" : "var(--line)",
      }}
    >
      {children}
    </button>
  );
}

function Stat({
  label,
  value,
  color,
}: {
  label: string;
  value: string;
  color?: string;
}) {
  return (
    <div className="flex flex-col">
      <span className="text-[9px] uppercase font-mono text-text-dim tracking-[0.5px]">
        {label}
      </span>
      <span
        className="text-[14px] font-semibold font-mono"
        style={{ color: color ?? "var(--text)" }}
      >
        {value}
      </span>
    </div>
  );
}

function Section({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1">
      <div className="text-[10px] uppercase font-mono text-text-dim tracking-[0.5px]">
        {title}
      </div>
      <div className="flex flex-col">{children}</div>
    </div>
  );
}

// ----- helpers -----

function todayIso(): string {
  return new Date().toISOString().slice(0, 10);
}

function describeCadence(rrule: string): string {
  const parts: Record<string, string> = {};
  rrule
    .replace(/^RRULE:/i, "")
    .split(";")
    .forEach((p) => {
      const [k, v] = p.split("=");
      if (k && v) parts[k.toUpperCase()] = v;
    });
  const freq = parts["FREQ"];
  const interval = parseInt(parts["INTERVAL"] ?? "1");
  if (freq === "DAILY") {
    if (interval <= 1) return "every day";
    return `every ${interval} days`;
  }
  if (freq === "WEEKLY" && parts["BYDAY"]) {
    const days = parts["BYDAY"]
      .split(",")
      .map((d) => {
        const map: Record<string, string> = {
          MO: "Mon",
          TU: "Tue",
          WE: "Wed",
          TH: "Thu",
          FR: "Fri",
          SA: "Sat",
          SU: "Sun",
        };
        return map[d.trim()] ?? d;
      })
      .join("/");
    return `weekly · ${days}`;
  }
  return rrule;
}
