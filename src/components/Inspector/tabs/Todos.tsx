import { useEffect, useMemo, useState, type KeyboardEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Icon } from "../../Icon";
import { TabEmpty, TabError, TabSkeleton } from "../TabStates";
import { useUiStore } from "../../../state/store";
import {
  deleteTodo as ipcDeleteTodo,
  listTodos,
  toggleTodo as ipcToggleTodo,
  upsertTodo,
} from "../../../ipc";
import { newId } from "../../../features/inspector/ids";
import type { Project, Todo } from "../../../types";

// Atlas - Inspector / Todos tab.

interface TodosProps {
  project: Project;
}

type FilterMode = "open" | "all" | "done";

export function Todos({ project }: TodosProps) {
  const [filter, setFilter] = useState<FilterMode>("open");
  const [draft, setDraft] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editText, setEditText] = useState("");

  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);
  const queryKey = useMemo(() => ["todos", project.id] as const, [project.id]);

  // Reset editor + draft when project changes - sticky drafts across projects
  useEffect(() => {
    setDraft("");
    setEditingId(null);
    setEditText("");
    setFilter("open");
  }, [project.id]);

  const { data, isLoading, error, refetch } = useQuery<Todo[]>({
    queryKey: [...queryKey],
    queryFn: () => listTodos(project.id),
    staleTime: 5_000,
    retry: false,
  });

  const todos = data ?? [];

  const upsert = useMutation({
    mutationFn: (todo: Todo) => upsertTodo(project.id, todo),
    onMutate: async (todo) => {
      await queryClient.cancelQueries({ queryKey: [...queryKey] });
      const previous = queryClient.getQueryData<Todo[]>([...queryKey]) ?? [];
      const next = previous.some((t) => t.id === todo.id)
        ? previous.map((t) => (t.id === todo.id ? todo : t))
        : [...previous, todo];
      queryClient.setQueryData<Todo[]>([...queryKey], next);
      return { previous };
    },
    onError: (err, _vars, ctx) => {
      if (ctx?.previous)
        queryClient.setQueryData<Todo[]>([...queryKey], ctx.previous);
      pushToast(
        "error",
        `Couldn't save todo: ${err instanceof Error ? err.message : String(err)}`,
      );
    },
  });

  const toggle = useMutation({
    mutationFn: (id: string) => ipcToggleTodo(project.id, id),
    onMutate: async (id) => {
      await queryClient.cancelQueries({ queryKey: [...queryKey] });
      const previous = queryClient.getQueryData<Todo[]>([...queryKey]) ?? [];
      queryClient.setQueryData<Todo[]>(
        [...queryKey],
        previous.map((t) => (t.id === id ? { ...t, done: !t.done } : t)),
      );
      return { previous };
    },
    onError: (err, _id, ctx) => {
      if (ctx?.previous)
        queryClient.setQueryData<Todo[]>([...queryKey], ctx.previous);
      pushToast(
        "error",
        `Couldn't toggle: ${err instanceof Error ? err.message : String(err)}`,
      );
    },
  });

  const remove = useMutation({
    mutationFn: (id: string) => ipcDeleteTodo(project.id, id),
    onMutate: async (id) => {
      await queryClient.cancelQueries({ queryKey: [...queryKey] });
      const previous = queryClient.getQueryData<Todo[]>([...queryKey]) ?? [];
      queryClient.setQueryData<Todo[]>(
        [...queryKey],
        previous.filter((t) => t.id !== id),
      );
      return { previous };
    },
    onError: (err, _id, ctx) => {
      if (ctx?.previous)
        queryClient.setQueryData<Todo[]>([...queryKey], ctx.previous);
      pushToast(
        "error",
        `Couldn't delete: ${err instanceof Error ? err.message : String(err)}`,
      );
    },
  });

  const visible = todos.filter((t) =>
    filter === "all" ? true : filter === "open" ? !t.done : t.done,
  );
  const openCount = todos.filter((t) => !t.done).length;
  const doneCount = todos.length - openCount;

  const addFromDraft = () => {
    const text = draft.trim();
    if (!text) return;
    upsert.mutate({
      id: newId(),
      done: false,
      text,
      createdAt: new Date().toISOString(),
    });
    setDraft("");
  };

  const beginEdit = (t: Todo) => {
    setEditingId(t.id);
    setEditText(t.text);
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditText("");
  };

  const commitEdit = () => {
    if (!editingId) return;
    const next = editText.trim();
    const target = todos.find((t) => t.id === editingId);
    if (!target) return cancelEdit();
    if (!next) {
      remove.mutate(editingId);
    } else if (next !== target.text) {
      upsert.mutate({ ...target, text: next });
    }
    cancelEdit();
  };

  return (
    <div className="p-[14px] overflow-y-auto h-full">
      <FilterBar
        filter={filter}
        onChange={setFilter}
        counts={{ open: openCount, all: todos.length, done: doneCount }}
      />

      <div className="flex items-center gap-2 px-[8px] py-[6px] mt-[10px] mb-[8px] bg-surface-2 rounded-[5px] border border-line">
        <Icon name="plus" size={12} stroke="var(--text-dim)" />
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addFromDraft();
            }
          }}
          placeholder="add a todo — press enter"
          className="flex-1 bg-transparent border-none outline-none text-text text-[12px] font-sans"
        />
        {draft && (
          <button
            type="button"
            onClick={addFromDraft}
            className="px-[8px] py-[2px] font-mono text-[10px] bg-accent text-accent-fg rounded-[3px] font-semibold"
          >
            add
          </button>
        )}
      </div>

      {isLoading && !data && <TabSkeleton rows={4} />}
      {error && (
        <TabError
          message={error instanceof Error ? error.message : String(error)}
          onRetry={() => void refetch()}
        />
      )}
      {!isLoading && !error && visible.length === 0 && (
        <TabEmpty
          icon="check"
          title={
            filter === "done"
              ? "Nothing completed yet"
              : filter === "open"
                ? "All clear"
                : "No todos yet"
          }
          hint={
            filter === "open" && doneCount > 0
              ? `${doneCount} completed — see 'done' tab`
              : filter === "all"
                ? "Type above and press Enter to add one"
                : null
          }
        />
      )}

      {visible.map((t) => {
        const isEditing = editingId === t.id;
        return (
          <TodoRow
            key={t.id}
            todo={t}
            isEditing={isEditing}
            editText={editText}
            onEditTextChange={setEditText}
            onToggle={() => toggle.mutate(t.id)}
            onBeginEdit={() => beginEdit(t)}
            onCommitEdit={commitEdit}
            onCancelEdit={cancelEdit}
            onDelete={() => remove.mutate(t.id)}
          />
        );
      })}
    </div>
  );
}

function FilterBar({
  filter,
  onChange,
  counts,
}: {
  filter: FilterMode;
  onChange: (m: FilterMode) => void;
  counts: { open: number; all: number; done: number };
}) {
  const opts: FilterMode[] = ["open", "all", "done"];
  return (
    <div className="flex border border-line rounded-[4px] overflow-hidden w-fit">
      {opts.map((m, i) => {
        const active = filter === m;
        return (
          <button
            key={m}
            type="button"
            onClick={() => onChange(m)}
            className="px-[9px] py-[3px] font-mono text-[10px] uppercase tracking-[0.5px]"
            style={{
              background: active ? "var(--surface-2)" : "transparent",
              color: active ? "var(--text)" : "var(--text-dim)",
              borderRight: i < opts.length - 1 ? "1px solid var(--line)" : undefined,
            }}
          >
            {m} <span className="text-text-dimmer">{counts[m]}</span>
          </button>
        );
      })}
    </div>
  );
}

interface TodoRowProps {
  todo: Todo;
  isEditing: boolean;
  editText: string;
  onEditTextChange: (s: string) => void;
  onToggle: () => void;
  onBeginEdit: () => void;
  onCommitEdit: () => void;
  onCancelEdit: () => void;
  onDelete: () => void;
}

function TodoRow({
  todo: t,
  isEditing,
  editText,
  onEditTextChange,
  onToggle,
  onBeginEdit,
  onCommitEdit,
  onCancelEdit,
  onDelete,
}: TodoRowProps) {
  const isDueToday = t.due === "today";
  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      onCommitEdit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onCancelEdit();
    }
  };
  return (
    <div
      className="todo-row group flex items-start gap-2 py-[8px] px-[6px] border-b border-line-soft rounded-[3px]"
    >
      <button
        type="button"
        onClick={onToggle}
        className="cursor-pointer pt-[1px]"
        title={t.done ? "Mark open" : "Mark done"}
        aria-label={t.done ? "Mark todo as open" : "Mark todo as done"}
        aria-pressed={t.done}
      >
        <Icon
          name={t.done ? "square-check" : "square"}
          size={13}
          stroke={t.done ? "var(--accent)" : "var(--text-dim)"}
        />
      </button>
      {isEditing ? (
        <input
          autoFocus
          value={editText}
          onChange={(e) => onEditTextChange(e.target.value)}
          onBlur={onCommitEdit}
          onKeyDown={onKey}
          className="flex-1 bg-bg border border-accent rounded-[3px] px-[6px] py-[2px] outline-none text-text text-[12px] font-sans"
        />
      ) : (
        <span
          onDoubleClick={onBeginEdit}
          className="flex-1 text-[12px] leading-[1.4] cursor-text"
          style={{
            color: t.done ? "var(--text-dimmer)" : "var(--text)",
            textDecoration: t.done ? "line-through" : "none",
          }}
        >
          {t.text}
        </span>
      )}
      {t.due && !isEditing && (
        <span
          className="font-mono text-[10px] px-[5px] py-[1px] rounded-[3px]"
          style={{
            background: isDueToday
              ? "oklch(0.78 0.15 80 / 0.18)"
              : "var(--surface-2)",
            color: isDueToday ? "var(--warn)" : "var(--text-dim)",
          }}
        >
          {t.due}
        </span>
      )}
      {!isEditing && (
        <div className="flex gap-[2px] opacity-0 group-hover:opacity-100 transition-opacity">
          <button
            type="button"
            onClick={onBeginEdit}
            title="Edit"
            aria-label="Edit todo"
            className="w-[22px] h-[22px] inline-flex items-center justify-center bg-transparent border-none rounded-[3px]"
          >
            <Icon name="note" size={11} stroke="var(--text-dim)" />
          </button>
          <button
            type="button"
            onClick={onDelete}
            title="Delete"
            aria-label="Delete todo"
            className="w-[22px] h-[22px] inline-flex items-center justify-center bg-transparent border-none rounded-[3px]"
          >
            <Icon name="trash" size={11} stroke="var(--text-dim)" />
          </button>
        </div>
      )}
    </div>
  );
}
