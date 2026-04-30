import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { Icon } from "../../components/Icon";
import {
  completeRoutineInstance,
  deleteTodo,
  skipRoutineInstance,
  toggleTodo,
  upsertTodo,
} from "../../ipc";
import { useUiStore } from "../../state/store";
import type { PlannerToday, Todo, TodayItem } from "../../types";
import { itemTitle } from "./TodayShared";

interface Props {
  open: boolean;
  today: PlannerToday | null;
  onClose: () => void;
}

/** Sunsama-style end-of-day walkthrough. Steps through every unfinished
 *  must-do item one-by-one. The user picks done / drop / defer; the
 *  modal closes when the queue is exhausted or the user dismisses. */
export function ShutdownFlow({ open, today, onClose }: Props) {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);
  const [idx, setIdx] = useState(0);

  // Restart from the top whenever the modal opens.
  const items = today?.mustDo ?? [];
  const current = items[idx];

  const completeTodoMut = useMutation({
    mutationFn: (vars: { projectId: string; todoId: string }) =>
      toggleTodo(vars.projectId, vars.todoId),
    onSuccess: () =>
      void queryClient.invalidateQueries({ queryKey: ["planner-today"] }),
    onError: (e) => pushToast("error", `Couldn't mark done: ${String(e)}`),
  });

  const dropTodoMut = useMutation({
    mutationFn: (vars: { projectId: string; todoId: string }) =>
      deleteTodo(vars.projectId, vars.todoId),
    onSuccess: () =>
      void queryClient.invalidateQueries({ queryKey: ["planner-today"] }),
    onError: (e) => pushToast("error", `Couldn't drop: ${String(e)}`),
  });

  const deferTodoMut = useMutation({
    mutationFn: (vars: { projectId: string; todo: Todo }) =>
      upsertTodo(vars.projectId, {
        ...vars.todo,
        deadline: tomorrowIso(),
      }),
    onSuccess: () =>
      void queryClient.invalidateQueries({ queryKey: ["planner-today"] }),
    onError: (e) => pushToast("error", `Couldn't defer: ${String(e)}`),
  });

  const completeRoutineMut = useMutation({
    mutationFn: (id: string) => completeRoutineInstance(id),
    onSuccess: () =>
      void queryClient.invalidateQueries({ queryKey: ["planner-today"] }),
    onError: (e) => pushToast("error", `Couldn't mark done: ${String(e)}`),
  });

  const skipRoutineMut = useMutation({
    mutationFn: (id: string) => skipRoutineInstance(id),
    onSuccess: () =>
      void queryClient.invalidateQueries({ queryKey: ["planner-today"] }),
    onError: (e) => pushToast("error", `Couldn't skip: ${String(e)}`),
  });

  const next = () => setIdx((i) => i + 1);

  const onDone = (item: TodayItem) => {
    if (item.kind === "todo") {
      completeTodoMut.mutate({ projectId: item.projectId, todoId: item.id });
    } else if (item.kind === "routine-instance") {
      completeRoutineMut.mutate(item.id);
    }
    next();
  };
  const onDrop = (item: TodayItem) => {
    if (item.kind === "todo") {
      dropTodoMut.mutate({ projectId: item.projectId, todoId: item.id });
    } else if (item.kind === "routine-instance") {
      skipRoutineMut.mutate(item.id);
    }
    // Milestones can't be "dropped" from the shutdown flow.
    next();
  };
  const onDefer = (item: TodayItem) => {
    if (item.kind === "todo") {
      // Naive defer: clone the row with deadline=tomorrow. The backend
      // upsert will stamp updates and recompute milestone scores.
      deferTodoMut.mutate({
        projectId: item.projectId,
        todo: rebuildTodoFromItem(item),
      });
    }
    // Routine instances can't be "deferred" — their cadence already
    // governs the next occurrence. Skipping is the right escape.
    next();
  };

  if (!open) return null;
  if (!today) return null;

  // Done queue → confirmation.
  if (!current) {
    return (
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Shutdown complete"
        className="fixed inset-0 z-50 bg-bg/85 backdrop-blur-sm flex items-center justify-center"
        onClick={(e) => {
          if (e.target === e.currentTarget) onClose();
        }}
      >
        <div className="w-[420px] bg-bg border border-line rounded-[10px] shadow-2xl flex flex-col items-center gap-3 p-[24px]">
          <Icon name="check" size={20} stroke="var(--accent)" />
          <h2 className="text-[14px] font-semibold">Day shut down</h2>
          <p className="text-[12px] text-text-dim text-center">
            Everything in the must-do queue has been triaged. Tomorrow opens
            fresh.
          </p>
          <button
            type="button"
            onClick={onClose}
            className="mt-2 px-[12px] py-[5px] bg-accent text-accent-fg rounded-[5px] text-[11px] font-semibold"
          >
            Close
          </button>
        </div>
      </div>
    );
  }

  const total = items.length;
  const cur = idx + 1;
  const milestone = current.kind === "milestone-deadline";

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Evening shutdown"
      className="fixed inset-0 z-50 bg-bg/85 backdrop-blur-sm flex items-center justify-center"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-[480px] max-w-[92vw] bg-bg border border-line rounded-[10px] shadow-2xl flex flex-col">
        <header className="flex items-center gap-2 p-[12px] border-b border-line">
          <Icon name="sparkle" size={14} stroke="var(--accent)" />
          <span className="text-[14px] font-semibold flex-1">Shutdown</span>
          <span className="font-mono text-[10px] text-text-dim">
            {cur} / {total}
          </span>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="w-[24px] h-[24px] inline-flex items-center justify-center text-text-dim hover:text-text"
          >
            ✕
          </button>
        </header>

        <div className="px-[18px] py-[16px] flex flex-col gap-3">
          <span
            className="font-mono text-[9px] uppercase tracking-[0.5px]"
            style={{ color: "var(--text-dim)" }}
          >
            {current.kind === "todo"
              ? "todo"
              : current.kind === "routine-instance"
                ? "routine"
                : "milestone"}
          </span>
          <span className="text-[15px] font-semibold">{itemTitle(current)}</span>
          <span className="text-[11px] text-text-dim">
            {(current.kind === "todo"
              ? current.projectName
              : current.kind === "routine-instance"
                ? current.projectName ?? "global"
                : current.projectName) || ""}
          </span>
        </div>

        <footer className="flex gap-2 p-[12px] border-t border-line">
          {milestone ? (
            <span className="text-[11px] text-text-dim italic flex-1">
              Milestones aren't completed from the shutdown flow — open the
              project to mark this one.
            </span>
          ) : (
            <>
              <button
                type="button"
                onClick={() => onDone(current)}
                className="flex-1 py-[6px] bg-accent text-accent-fg rounded-[5px] text-[11px] font-semibold"
              >
                Done
              </button>
              <button
                type="button"
                onClick={() => onDefer(current)}
                disabled={current.kind !== "todo"}
                title={
                  current.kind === "todo"
                    ? "Move deadline to tomorrow"
                    : "Routines reschedule on their own cadence"
                }
                className="flex-1 py-[6px] border border-line rounded-[5px] text-[11px] text-text-dim hover:text-text disabled:opacity-40"
              >
                Defer
              </button>
              <button
                type="button"
                onClick={() => onDrop(current)}
                className="flex-1 py-[6px] border border-line rounded-[5px] text-[11px] text-text-dim hover:text-err"
              >
                Drop
              </button>
            </>
          )}
          {milestone && (
            <button
              type="button"
              onClick={next}
              className="flex-1 py-[6px] border border-line rounded-[5px] text-[11px] text-text-dim hover:text-text"
            >
              Skip
            </button>
          )}
        </footer>
      </div>
    </div>
  );
}

function tomorrowIso(): string {
  const d = new Date();
  d.setDate(d.getDate() + 1);
  return d.toISOString().slice(0, 10);
}

/** Reconstruct a `Todo` shape from a `TodayItem.kind === "todo"` so the
 *  defer mutation can upsert it. The Today payload doesn't carry every
 *  Todo field; missing fields default to undefined and the backend
 *  upsert path treats them additively. */
function rebuildTodoFromItem(item: Extract<TodayItem, { kind: "todo" }>): Todo {
  return {
    id: item.id,
    text: item.text,
    done: false,
    createdAt: new Date().toISOString(),
    projectId: item.projectId,
    priority: item.priority,
    deadline: item.deadline ?? undefined,
    pinnedToday: item.pinnedToday,
  };
}
