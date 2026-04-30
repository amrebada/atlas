import { useEffect, useMemo, useState } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { Icon } from "../../Icon";
import { TabEmpty, TabError, TabSkeleton } from "../TabStates";
import { useUiStore } from "../../../state/store";
import {
  createMilestone,
  deleteMilestone,
  extendMilestone,
  listMilestones,
  listTodos,
  setMilestoneStatus,
  updateMilestone,
  upsertTodo,
} from "../../../ipc";
import { newId } from "../../../features/inspector/ids";
import type {
  ExtensionReason,
  Milestone,
  MilestoneStatus,
  Priority,
  Project,
  Todo,
} from "../../../types";

interface MilestonesProps {
  project: Project;
}

type Mode = { kind: "list" } | { kind: "detail"; id: string } | { kind: "create" };

const PRIORITY_OPTIONS: Array<{ value: Priority; label: string }> = [
  { value: "p0", label: "P0 — must" },
  { value: "p1", label: "P1 — should" },
  { value: "p2", label: "P2 — nice" },
  { value: "p3", label: "P3 — eventually" },
];

const PRIORITY_COLOR: Record<Priority, string> = {
  p0: "var(--err, #ef4444)",
  p1: "var(--warn, #f59e0b)",
  p2: "var(--accent, #3b82f6)",
  p3: "var(--text-dim, #6b7280)",
};

export function Milestones({ project }: MilestonesProps) {
  const [mode, setMode] = useState<Mode>({ kind: "list" });
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);

  // Reset to list view when the user switches projects.
  useEffect(() => {
    setMode({ kind: "list" });
  }, [project.id]);

  const milestonesKey = useMemo(
    () => ["milestones", project.id] as const,
    [project.id],
  );
  const todosKey = useMemo(() => ["todos", project.id] as const, [project.id]);

  const milestonesQ = useQuery<Milestone[]>({
    queryKey: [...milestonesKey],
    queryFn: () => listMilestones(project.id),
    staleTime: 5_000,
    retry: false,
  });

  const todosQ = useQuery<Todo[]>({
    queryKey: [...todosKey],
    queryFn: () => listTodos(project.id),
    staleTime: 5_000,
    retry: false,
  });

  const milestones = milestonesQ.data ?? [];
  const todos = todosQ.data ?? [];

  const createMut = useMutation({
    mutationFn: (m: Milestone) => createMilestone(project.id, m),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: [...milestonesKey] });
      setMode({ kind: "list" });
    },
    onError: (e) => pushToast("error", `Couldn't create milestone: ${String(e)}`),
  });

  const updateMut = useMutation({
    mutationFn: ({
      id,
      patch,
    }: {
      id: string;
      patch: Parameters<typeof updateMilestone>[2];
    }) => updateMilestone(project.id, id, patch),
    onSuccess: () =>
      void queryClient.invalidateQueries({ queryKey: [...milestonesKey] }),
    onError: (e) => pushToast("error", `Couldn't update milestone: ${String(e)}`),
  });

  const extendMut = useMutation({
    mutationFn: (vars: {
      id: string;
      deadline: string;
      reason: ExtensionReason;
      note?: string;
    }) =>
      extendMilestone(project.id, vars.id, vars.deadline, vars.reason, vars.note),
    onSuccess: () =>
      void queryClient.invalidateQueries({ queryKey: [...milestonesKey] }),
    onError: (e) => pushToast("error", `Couldn't extend deadline: ${String(e)}`),
  });

  const statusMut = useMutation({
    mutationFn: (vars: { id: string; status: MilestoneStatus }) =>
      setMilestoneStatus(project.id, vars.id, vars.status),
    onSuccess: () =>
      void queryClient.invalidateQueries({ queryKey: [...milestonesKey] }),
    onError: (e) => pushToast("error", `Couldn't change status: ${String(e)}`),
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => deleteMilestone(project.id, id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: [...milestonesKey] });
      void queryClient.invalidateQueries({ queryKey: [...todosKey] });
      setMode({ kind: "list" });
    },
    onError: (e) => pushToast("error", `Couldn't delete milestone: ${String(e)}`),
  });

  const attachTodoMut = useMutation({
    mutationFn: (vars: { todo: Todo; milestoneId: string | null }) =>
      upsertTodo(project.id, {
        ...vars.todo,
        milestoneId: vars.milestoneId ?? undefined,
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: [...todosKey] });
      void queryClient.invalidateQueries({ queryKey: [...milestonesKey] });
    },
    onError: (e) => pushToast("error", `Couldn't move todo: ${String(e)}`),
  });

  if (mode.kind === "create") {
    return (
      <CreateForm
        defaultDeadline={defaultDeadlineISO()}
        onCancel={() => setMode({ kind: "list" })}
        onSubmit={(payload) => {
          const id = newId();
          const now = new Date().toISOString();
          createMut.mutate({
            id,
            projectId: project.id,
            title: payload.title,
            description: undefined,
            deadline: payload.deadline,
            originalDeadline: payload.deadline,
            status: "active",
            priority: payload.priority,
            order: milestones.length,
            todoIds: [],
            extensions: [],
            successPoints: 0,
            failingPoints: 0,
            createdAt: now,
          });
        }}
        pending={createMut.isPending}
      />
    );
  }

  if (mode.kind === "detail") {
    const m = milestones.find((mm) => mm.id === mode.id);
    if (!m) {
      // Was probably just deleted — fall back to list.
      return (
        <Detail
          milestone={null}
          allTodos={[]}
          onBack={() => setMode({ kind: "list" })}
          onAttach={() => {}}
          onExtend={() => {}}
          onSetStatus={() => {}}
          onUpdate={() => {}}
          onDelete={() => {}}
          extendPending={false}
        />
      );
    }
    return (
      <Detail
        milestone={m}
        allTodos={todos}
        extendPending={extendMut.isPending}
        onBack={() => setMode({ kind: "list" })}
        onAttach={(todo, attach) =>
          attachTodoMut.mutate({
            todo,
            milestoneId: attach ? m.id : null,
          })
        }
        onExtend={(deadline, reason, note) =>
          extendMut.mutate({ id: m.id, deadline, reason, note })
        }
        onSetStatus={(status) => statusMut.mutate({ id: m.id, status })}
        onUpdate={(patch) => updateMut.mutate({ id: m.id, patch })}
        onDelete={() => deleteMut.mutate(m.id)}
      />
    );
  }

  return (
    <div className="p-[14px] overflow-y-auto h-full">
      <Header
        count={milestones.length}
        onCreate={() => setMode({ kind: "create" })}
      />

      {milestonesQ.isLoading && !milestonesQ.data && <TabSkeleton rows={3} />}
      {milestonesQ.error && (
        <TabError
          message={String(milestonesQ.error)}
          onRetry={() => void milestonesQ.refetch()}
        />
      )}
      {!milestonesQ.isLoading && !milestonesQ.error && milestones.length === 0 && (
        <TabEmpty
          icon="clock"
          title="No milestones yet"
          hint="Press + to plan a deadline-driven goal"
        />
      )}

      <div className="flex flex-col gap-2 mt-3">
        {milestones.map((m) => (
          <MilestoneCard
            key={m.id}
            milestone={m}
            todos={todos.filter((t) => t.milestoneId === m.id)}
            onClick={() => setMode({ kind: "detail", id: m.id })}
          />
        ))}
      </div>
    </div>
  );
}

// ----- subcomponents -----

function Header({ count, onCreate }: { count: number; onCreate: () => void }) {
  return (
    <div className="flex items-center justify-between mb-2">
      <div className="text-[12px] text-text-dim">
        {count} milestone{count === 1 ? "" : "s"}
      </div>
      <button
        type="button"
        onClick={onCreate}
        title="New milestone"
        aria-label="New milestone"
        className="inline-flex items-center gap-1 px-[8px] py-[3px] bg-accent text-accent-fg rounded-[4px] text-[10px] font-mono uppercase tracking-[0.5px]"
      >
        <Icon name="plus" size={11} stroke="var(--accent-fg)" />
        new
      </button>
    </div>
  );
}

function MilestoneCard({
  milestone,
  todos,
  onClick,
}: {
  milestone: Milestone;
  todos: Todo[];
  onClick: () => void;
}) {
  const total = todos.length;
  const done = todos.filter((t) => t.done).length;
  const progress = total === 0 ? 0 : Math.round((done / total) * 100);
  const health = healthFromDeadline(milestone);
  const rate = rollingRate(milestone);

  return (
    <button
      type="button"
      onClick={onClick}
      className="text-left p-[10px] bg-surface-2 border border-line rounded-[6px] hover:border-accent transition-colors"
    >
      <div className="flex items-center gap-2 mb-1">
        <span
          className="w-[8px] h-[8px] rounded-full shrink-0"
          style={{ background: health.color }}
          title={health.label}
        />
        <span
          className="font-mono text-[9px] uppercase tracking-[0.5px]"
          style={{ color: PRIORITY_COLOR[milestone.priority] }}
        >
          {milestone.priority}
        </span>
        <span className="font-semibold text-[12px] flex-1 truncate">
          {milestone.title}
        </span>
        <span className="font-mono text-[10px] text-text-dim shrink-0">
          {milestone.deadline}
        </span>
      </div>
      <div className="flex items-center gap-2">
        <div className="flex-1 h-[4px] bg-line rounded-full overflow-hidden">
          <div
            className="h-full rounded-full"
            style={{ width: `${progress}%`, background: "var(--accent)" }}
          />
        </div>
        <span className="font-mono text-[10px] text-text-dim shrink-0">
          {done}/{total}
        </span>
        <span
          className="font-mono text-[10px] shrink-0"
          style={{ color: rate >= 0.85 ? "var(--accent)" : rate >= 0.5 ? "var(--warn, #f59e0b)" : "var(--err, #ef4444)" }}
          title="Rolling success rate"
        >
          {Math.round(rate * 100)}%
        </span>
      </div>
      {milestone.status !== "active" && (
        <div className="mt-1 font-mono text-[9px] uppercase text-text-dim">
          status: {milestone.status}
          {milestone.extensions.length > 0 &&
            ` · extended ${milestone.extensions.length}×`}
        </div>
      )}
    </button>
  );
}

function CreateForm({
  defaultDeadline,
  onCancel,
  onSubmit,
  pending,
}: {
  defaultDeadline: string;
  onCancel: () => void;
  onSubmit: (p: { title: string; deadline: string; priority: Priority }) => void;
  pending: boolean;
}) {
  const [title, setTitle] = useState("");
  const [deadline, setDeadline] = useState(defaultDeadline);
  const [priority, setPriority] = useState<Priority>("p2");

  return (
    <div className="p-[14px] flex flex-col gap-3 h-full overflow-y-auto">
      <div className="flex items-center justify-between">
        <span className="font-semibold text-[13px]">New milestone</span>
        <button
          type="button"
          onClick={onCancel}
          className="text-text-dim hover:text-text text-[11px]"
        >
          Cancel
        </button>
      </div>
      <label className="flex flex-col gap-1">
        <span className="text-[10px] uppercase font-mono text-text-dim tracking-[0.5px]">
          Title
        </span>
        <input
          autoFocus
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="e.g. v0.4 alpha"
          className="bg-surface-2 border border-line rounded-[5px] px-[8px] py-[5px] text-[12px] outline-none focus:border-accent"
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-[10px] uppercase font-mono text-text-dim tracking-[0.5px]">
          Deadline
        </span>
        <input
          type="date"
          value={deadline}
          onChange={(e) => setDeadline(e.target.value)}
          className="bg-surface-2 border border-line rounded-[5px] px-[8px] py-[5px] text-[12px] outline-none focus:border-accent font-mono"
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-[10px] uppercase font-mono text-text-dim tracking-[0.5px]">
          Priority
        </span>
        <select
          value={priority}
          onChange={(e) => setPriority(e.target.value as Priority)}
          className="bg-surface-2 border border-line rounded-[5px] px-[8px] py-[5px] text-[12px] outline-none focus:border-accent"
        >
          {PRIORITY_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </label>
      <button
        type="button"
        disabled={!title.trim() || pending}
        onClick={() =>
          onSubmit({ title: title.trim(), deadline, priority })
        }
        className="self-start mt-1 px-[12px] py-[5px] bg-accent text-accent-fg rounded-[5px] text-[11px] font-semibold disabled:opacity-50"
      >
        {pending ? "Creating…" : "Create milestone"}
      </button>
    </div>
  );
}

function Detail({
  milestone,
  allTodos,
  extendPending,
  onBack,
  onAttach,
  onExtend,
  onSetStatus,
  onUpdate,
  onDelete,
}: {
  milestone: Milestone | null;
  allTodos: Todo[];
  extendPending: boolean;
  onBack: () => void;
  onAttach: (todo: Todo, attach: boolean) => void;
  onExtend: (deadline: string, reason: ExtensionReason, note?: string) => void;
  onSetStatus: (status: MilestoneStatus) => void;
  onUpdate: (patch: { title?: string; description?: string; priority?: Priority }) => void;
  onDelete: () => void;
}) {
  const [extending, setExtending] = useState(false);
  const [newDeadline, setNewDeadline] = useState("");
  const [showLog, setShowLog] = useState(false);

  if (!milestone) {
    return (
      <div className="p-[14px] text-text-dim text-[12px]">
        Milestone gone.{" "}
        <button onClick={onBack} className="underline">
          Back to list
        </button>
      </div>
    );
  }

  const memberIds = new Set(
    allTodos.filter((t) => t.milestoneId === milestone.id).map((t) => t.id),
  );
  const memberTodos = allTodos.filter((t) => memberIds.has(t.id));
  const candidateTodos = allTodos.filter((t) => !memberIds.has(t.id));
  const total = memberTodos.length;
  const done = memberTodos.filter((t) => t.done).length;
  const rate = rollingRate(milestone);
  const health = healthFromDeadline(milestone);

  const previewCost = useMemo(() => {
    if (!extending || !newDeadline) return 0;
    return previewSoftenCost(milestone, newDeadline);
  }, [extending, newDeadline, milestone]);

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
        <span className="flex-1" />
        <span
          className="w-[8px] h-[8px] rounded-full"
          style={{ background: health.color }}
          title={health.label}
        />
        <span className="font-mono text-[10px] text-text-dim">
          {health.label}
        </span>
      </div>

      <div>
        <input
          value={milestone.title}
          onChange={(e) => onUpdate({ title: e.target.value })}
          className="w-full bg-transparent border-none outline-none text-[15px] font-semibold focus:border-b focus:border-accent"
        />
        <div className="flex items-center gap-2 mt-1">
          <select
            value={milestone.priority}
            onChange={(e) => onUpdate({ priority: e.target.value as Priority })}
            className="bg-transparent border-none text-[10px] font-mono uppercase tracking-[0.5px] outline-none"
            style={{ color: PRIORITY_COLOR[milestone.priority] }}
          >
            {PRIORITY_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.value.toUpperCase()}
              </option>
            ))}
          </select>
          <span className="text-text-dimmer text-[10px]">·</span>
          <span className="font-mono text-[10px] text-text-dim">
            deadline: {milestone.deadline}
          </span>
          {milestone.deadline !== milestone.originalDeadline && (
            <span className="font-mono text-[10px] text-warn">
              (was {milestone.originalDeadline})
            </span>
          )}
        </div>
      </div>

      {/* Stats row */}
      <div className="flex items-center gap-3 p-[10px] bg-surface-2 border border-line rounded-[6px]">
        <Stat label="todos" value={`${done}/${total}`} />
        <Stat
          label="success"
          value={`${Math.round(rate * 100)}%`}
          color={rate >= 0.85 ? "var(--accent)" : rate >= 0.5 ? "var(--warn, #f59e0b)" : "var(--err, #ef4444)"}
        />
        <Stat label="success pts" value={Math.round(milestone.successPoints).toString()} />
        <Stat label="fail pts" value={Math.round(milestone.failingPoints).toString()} />
      </div>

      {/* Status + Extend + Delete actions */}
      <div className="flex items-center gap-2 flex-wrap">
        {(["active", "done", "missed", "cancelled"] as MilestoneStatus[]).map((s) => (
          <button
            key={s}
            type="button"
            disabled={milestone.status === s}
            onClick={() => onSetStatus(s)}
            className="px-[8px] py-[3px] text-[10px] font-mono uppercase tracking-[0.5px] border rounded-[4px]"
            style={{
              background: milestone.status === s ? "var(--accent)" : "transparent",
              color: milestone.status === s ? "var(--accent-fg)" : "var(--text-dim)",
              borderColor: milestone.status === s ? "var(--accent)" : "var(--line)",
              opacity: milestone.status === s ? 1 : 0.85,
            }}
          >
            {s}
          </button>
        ))}
        <span className="flex-1" />
        <button
          type="button"
          onClick={() => {
            setExtending((v) => !v);
            setNewDeadline(milestone.deadline);
          }}
          className="px-[8px] py-[3px] text-[10px] font-mono uppercase tracking-[0.5px] border border-line rounded-[4px] text-text-dim hover:text-text"
        >
          extend
        </button>
        <button
          type="button"
          onClick={() => {
            if (
              confirm(
                `Delete milestone "${milestone.title}"? Member todos will move back to project root.`,
              )
            ) {
              onDelete();
            }
          }}
          className="px-[8px] py-[3px] text-[10px] font-mono uppercase tracking-[0.5px] border border-line rounded-[4px] text-text-dim hover:text-err"
          style={{ borderColor: "var(--line)" }}
        >
          delete
        </button>
      </div>

      {extending && (
        <div className="p-[10px] border border-line rounded-[6px] bg-surface-2 flex flex-col gap-2">
          <div className="text-[10px] uppercase font-mono text-text-dim tracking-[0.5px]">
            Move deadline to
          </div>
          <input
            type="date"
            value={newDeadline}
            onChange={(e) => setNewDeadline(e.target.value)}
            className="bg-bg border border-line rounded-[5px] px-[8px] py-[4px] text-[12px] outline-none focus:border-accent font-mono"
          />
          {previewCost > 0 ? (
            <div className="text-[11px] text-warn">
              Softening cost: ~{Math.round(previewCost)} failing points
              <span className="text-text-dimmer ml-1">
                (akrasia horizon)
              </span>
            </div>
          ) : (
            <div className="text-[11px] text-text-dim">
              Outside the akrasia horizon — free move.
            </div>
          )}
          <div className="flex gap-2">
            <button
              type="button"
              disabled={extendPending || !newDeadline || newDeadline === milestone.deadline}
              onClick={() => {
                onExtend(
                  newDeadline,
                  previewCost > 0 ? "user-soften" : "user-soften",
                );
                setExtending(false);
              }}
              className="px-[10px] py-[4px] bg-accent text-accent-fg rounded-[5px] text-[11px] font-semibold disabled:opacity-50"
            >
              {extendPending ? "Saving…" : "Confirm"}
            </button>
            <button
              type="button"
              onClick={() => setExtending(false)}
              className="px-[10px] py-[4px] text-text-dim hover:text-text text-[11px]"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* Member todos */}
      <Section title="Member todos">
        {memberTodos.length === 0 ? (
          <div className="text-[11px] text-text-dim italic">
            No todos yet — attach some from the project below.
          </div>
        ) : (
          memberTodos.map((t) => (
            <TodoRowMini
              key={t.id}
              todo={t}
              checked
              onToggleAttach={() => onAttach(t, false)}
            />
          ))
        )}
      </Section>

      {/* Project todos that aren't yet members */}
      {candidateTodos.length > 0 && (
        <Section title="Attach from project">
          {candidateTodos.map((t) => (
            <TodoRowMini
              key={t.id}
              todo={t}
              checked={false}
              onToggleAttach={() => onAttach(t, true)}
            />
          ))}
        </Section>
      )}

      {/* Extension log */}
      {milestone.extensions.length > 0 && (
        <Section
          title={`Extension log (${milestone.extensions.length})`}
          collapsed={!showLog}
          onToggle={() => setShowLog((v) => !v)}
        >
          {showLog &&
            milestone.extensions
              .slice()
              .reverse()
              .map((e, i) => (
                <div
                  key={i}
                  className="text-[11px] font-mono text-text-dim border-b border-line py-1"
                >
                  <div>
                    <span className="text-text">{e.from}</span>
                    {" → "}
                    <span className="text-text">{e.to}</span>{" "}
                    <span className="text-text-dimmer">({e.reason})</span>
                  </div>
                  {e.failingPointsApplied > 0 && (
                    <div className="text-warn">
                      cost: {Math.round(e.failingPointsApplied)} pts
                    </div>
                  )}
                  {e.note && <div className="italic">{e.note}</div>}
                </div>
              ))}
        </Section>
      )}
    </div>
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
  collapsed,
  onToggle,
  children,
}: {
  title: string;
  collapsed?: boolean;
  onToggle?: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1">
      <div
        className="text-[10px] uppercase font-mono text-text-dim tracking-[0.5px] flex items-center gap-1 cursor-pointer select-none"
        onClick={onToggle}
      >
        {onToggle && (
          <Icon
            name={collapsed ? "chevron" : "chevron-d"}
            size={10}
            stroke="currentColor"
          />
        )}
        {title}
      </div>
      <div className="flex flex-col">{children}</div>
    </div>
  );
}

function TodoRowMini({
  todo,
  checked,
  onToggleAttach,
}: {
  todo: Todo;
  checked: boolean;
  onToggleAttach: () => void;
}) {
  return (
    <div className="flex items-center gap-2 py-[5px] border-b border-line-soft">
      <button
        type="button"
        onClick={onToggleAttach}
        title={checked ? "Detach from milestone" : "Attach to milestone"}
        aria-label={checked ? "Detach todo" : "Attach todo"}
        aria-pressed={checked}
      >
        <Icon
          name={checked ? "square-check" : "square"}
          size={13}
          stroke={checked ? "var(--accent)" : "var(--text-dim)"}
        />
      </button>
      <span
        className="text-[12px] flex-1 truncate"
        style={{
          color: todo.done ? "var(--text-dimmer)" : "var(--text)",
          textDecoration: todo.done ? "line-through" : "none",
        }}
      >
        {todo.text}
      </span>
      {todo.priority && (
        <span
          className="font-mono text-[9px] uppercase tracking-[0.5px]"
          style={{ color: PRIORITY_COLOR[todo.priority] }}
        >
          {todo.priority}
        </span>
      )}
      {todo.deadline && (
        <span className="font-mono text-[10px] text-text-dim">
          {todo.deadline}
        </span>
      )}
    </div>
  );
}

// ----- helpers -----

function defaultDeadlineISO(): string {
  // Default: 14 days out — enough buffer to dodge the akrasia horizon by default.
  const d = new Date();
  d.setDate(d.getDate() + 14);
  return d.toISOString().slice(0, 10);
}

function rollingRate(m: Milestone): number {
  const total = m.successPoints + m.failingPoints;
  return total <= 0 ? 1 : m.successPoints / total;
}

function healthFromDeadline(m: Milestone): { color: string; label: string } {
  if (m.status === "done") return { color: "var(--accent)", label: "done" };
  if (m.status === "missed") return { color: "var(--err, #ef4444)", label: "missed" };
  if (m.status === "cancelled")
    return { color: "var(--text-dim)", label: "cancelled" };

  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const dl = new Date(m.deadline);
  dl.setHours(0, 0, 0, 0);
  const days = Math.round((dl.getTime() - today.getTime()) / 86_400_000);
  if (days < 0) return { color: "var(--err, #ef4444)", label: `${-days}d overdue` };
  if (days === 0) return { color: "#f97316", label: "today" };
  if (days <= 2) return { color: "#3b82f6", label: `${days}d left` };
  return { color: "var(--accent)", label: `${days}d left` };
}

const AKRASIA_DAYS = 7;
const SOFTEN_COST_PER_DAY = 50;
const PRIORITY_WEIGHT: Record<Priority, number> = {
  p0: 2.0,
  p1: 1.5,
  p2: 1.0,
  p3: 0.5,
};

/** Mirror of `score_engine::cost_of_extension` for a UI preview only —
 *  the server is the source of truth on save. Returns the failing-point
 *  cost the user would incur by softening to `newDeadline` right now. */
function previewSoftenCost(m: Milestone, newDeadlineISO: string): number {
  const now = new Date();
  const horizon = new Date(now.getTime() + AKRASIA_DAYS * 86_400_000);
  const from = new Date(m.deadline);
  const to = new Date(newDeadlineISO);
  if (Number.isNaN(to.getTime())) return 0;

  const lo = from > now ? from : now;
  const hi = to < horizon ? to : horizon;
  if (hi <= lo) return 0;
  const days = (hi.getTime() - lo.getTime()) / 86_400_000;
  return days * SOFTEN_COST_PER_DAY * PRIORITY_WEIGHT[m.priority];
}
