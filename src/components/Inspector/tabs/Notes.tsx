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
  deleteNote as ipcDeleteNote,
  listNotes,
  pinNote as ipcPinNote,
  searchNotes,
  upsertNote,
} from "../../../ipc";
import { newId } from "../../../features/inspector/ids";
import type { Note, Project } from "../../../types";

// Atlas - Inspector / Notes tab.

interface NotesProps {
  project: Project;
}

export function Notes({ project }: NotesProps) {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);
  const setOpenNote = useUiStore((s) => s.setOpenNote);

  const [query, setQuery] = useState("");

  // Clear the search box when switching projects so filters don't leak.
  useEffect(() => {
    setQuery("");
  }, [project.id]);

  const trimmedQuery = query.trim();
  const listKey = useMemo(
    () =>
      trimmedQuery
        ? (["notes", project.id, trimmedQuery] as const)
        : (["notes", project.id] as const),
    [project.id, trimmedQuery],
  );

  const { data, isLoading, error, refetch } = useQuery<Note[]>({
    queryKey: [...listKey],
    queryFn: () =>
      trimmedQuery
        ? searchNotes(project.id, trimmedQuery)
        : listNotes(project.id),
    staleTime: 5_000,
    retry: false,
  });

  const notes = data ?? [];

  const sorted = useMemo(() => {
    return [...notes].sort((a, b) => {
      if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
      return (
        new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime()
      );
    });
  }, [notes]);

  // ---- mutations --------------------------------------------------------

  const invalidateLists = () => {
    // Any `['notes', projectId, ...]` cache may need a refresh - invalidate
    queryClient.invalidateQueries({ queryKey: ["notes", project.id] });
  };

  const createMutation = useMutation({
    mutationFn: async () => {
      const now = new Date().toISOString();
      const note: Note = {
        id: newId(),
        title: "Untitled note",
        body: "<p></p>",
        pinned: false,
        createdAt: now,
        updatedAt: now,
      };
      await upsertNote(project.id, note);
      return note;
    },
    onSuccess: (note) => {
      // Seed the single-note cache so the editor opens instantly without
      queryClient.setQueryData(["note", project.id, note.id], note);
      queryClient.setQueryData<Note[]>(["notes", project.id], (old) =>
        old ? [note, ...old] : [note],
      );
      setOpenNote({ projectId: project.id, noteId: note.id });
    },
    onError: (err) => {
      pushToast(
        "error",
        `Couldn't create note: ${err instanceof Error ? err.message : String(err)}`,
      );
    },
  });

  const pinMutation = useMutation({
    mutationFn: (note: Note) =>
      ipcPinNote(project.id, note.id, !note.pinned),
    onMutate: async (note) => {
      await queryClient.cancelQueries({ queryKey: ["notes", project.id] });
      const prev = queryClient.getQueryData<Note[]>(["notes", project.id]);
      queryClient.setQueryData<Note[]>(["notes", project.id], (old) =>
        old
          ? old.map((n) =>
              n.id === note.id ? { ...n, pinned: !n.pinned } : n,
            )
          : old,
      );
      return { prev };
    },
    onError: (err, _note, ctx) => {
      if (ctx?.prev)
        queryClient.setQueryData(["notes", project.id], ctx.prev);
      pushToast(
        "error",
        `Couldn't toggle pin: ${err instanceof Error ? err.message : String(err)}`,
      );
    },
    onSuccess: () => invalidateLists(),
  });

  const deleteMutation = useMutation({
    mutationFn: (noteId: string) => ipcDeleteNote(project.id, noteId),
    onMutate: async (noteId) => {
      await queryClient.cancelQueries({ queryKey: ["notes", project.id] });
      const prev = queryClient.getQueryData<Note[]>(["notes", project.id]);
      queryClient.setQueryData<Note[]>(["notes", project.id], (old) =>
        old ? old.filter((n) => n.id !== noteId) : old,
      );
      return { prev };
    },
    onError: (err, _id, ctx) => {
      if (ctx?.prev)
        queryClient.setQueryData(["notes", project.id], ctx.prev);
      pushToast(
        "error",
        `Couldn't delete: ${err instanceof Error ? err.message : String(err)}`,
      );
    },
    onSuccess: () => invalidateLists(),
  });

  // ---- render -----------------------------------------------------------

  return (
    <div className="flex flex-col h-full">
      <div className="px-[14px] pt-[14px] pb-[8px] flex items-center gap-2 shrink-0">
        <span className="font-mono text-[10px] text-text-dim uppercase tracking-[0.6px]">
          {notes.length} {notes.length === 1 ? "NOTE" : "NOTES"}
        </span>
        <div className="flex-1" />
        <button
          type="button"
          onClick={() => createMutation.mutate()}
          disabled={createMutation.isPending}
          className="inline-flex items-center gap-[5px] px-[8px] py-[3px] rounded-[3px] font-mono text-[10px] font-semibold"
          style={{
            background: "var(--accent)",
            color: "var(--accent-fg)",
            border: "none",
            cursor: createMutation.isPending ? "wait" : "pointer",
            opacity: createMutation.isPending ? 0.6 : 1,
          }}
        >
          <Icon name="plus" size={10} stroke="var(--accent-fg)" />
          new
        </button>
      </div>

      <div className="px-[14px] pb-[8px] shrink-0">
        <div
          className="flex items-center gap-[6px] h-[26px] px-[8px] rounded-[4px] transition-colors focus-within:border-accent"
          style={{
            background: "var(--surface-2)",
            border: "1px solid var(--line)",
          }}
        >
          <Icon name="search" size={11} stroke="var(--text-dim)" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search notes"
            className="flex-1 bg-transparent text-[12px] text-text"
            style={{
              fontFamily: "var(--sans)",
              outline: "none",
              border: "none",
              boxShadow: "none",
            }}
          />
          {query && (
            <button
              type="button"
              onClick={() => setQuery("")}
              title="Clear"
              aria-label="Clear search"
              className="text-text-dimmer hover:text-text-dim"
            >
              ×
            </button>
          )}
        </div>
      </div>

      <div className="flex-1 min-h-0 px-[14px] pb-[14px] overflow-y-auto">
        {isLoading && !data && <TabSkeleton rows={3} />}
        {error && (
          <TabError
            message={error instanceof Error ? error.message : String(error)}
            onRetry={() => void refetch()}
          />
        )}
        {!isLoading && !error && sorted.length === 0 && (
          <TabEmpty
            icon="note"
            title={
              trimmedQuery
                ? `No notes matching "${trimmedQuery}"`
                : "No notes yet"
            }
            hint={
              trimmedQuery
                ? "Try a different query"
                : "Press N or click + to start one"
            }
          />
        )}
        {sorted.map((note) => (
          <NoteCard
            key={note.id}
            note={note}
            onOpen={() =>
              setOpenNote({ projectId: project.id, noteId: note.id })
            }
            onTogglePin={() => pinMutation.mutate(note)}
            onDelete={() => {
              if (window.confirm("Delete this note?"))
                deleteMutation.mutate(note.id);
            }}
          />
        ))}
      </div>
    </div>
  );
}

// ---- card ----------------------------------------------------------------

interface NoteCardProps {
  note: Note;
  onOpen: () => void;
  onTogglePin: () => void;
  onDelete: () => void;
}

function NoteCard({ note, onOpen, onTogglePin, onDelete }: NoteCardProps) {
  const preview = useMemo(() => stripHtml(note.body), [note.body]);

  return (
    <div
      onClick={onOpen}
      className="group relative px-[12px] py-[10px] mb-[6px] rounded-[5px] cursor-pointer"
      style={{
        background: "var(--surface-2)",
        border: `1px solid ${note.pinned ? "oklch(0.78 0.17 145 / 0.25)" : "transparent"}`,
      }}
    >
      <div className="flex items-center gap-[6px] mb-[4px]">
        {note.pinned && (
          <Icon name="pin-fill" size={10} stroke="var(--accent)" />
        )}
        <span className="text-[12px] font-semibold text-text flex-1 truncate">
          {note.title || "Untitled note"}
        </span>
        <div className="opacity-0 group-hover:opacity-100 transition-opacity flex gap-[2px]">
          <CardIconBtn
            title={note.pinned ? "Unpin" : "Pin"}
            onClick={(e) => {
              e.stopPropagation();
              onTogglePin();
            }}
            icon={note.pinned ? "pin-fill" : "pin"}
            stroke={note.pinned ? "var(--accent)" : "var(--text-dim)"}
          />
          <CardIconBtn
            title="Delete"
            onClick={(e) => {
              e.stopPropagation();
              onDelete();
            }}
            icon="trash"
            stroke="var(--text-dim)"
          />
        </div>
      </div>
      <div
        className="text-[11px] leading-snug text-text-dim mb-[6px]"
        style={{
          display: "-webkit-box",
          WebkitLineClamp: 2,
          WebkitBoxOrient: "vertical",
          overflow: "hidden",
        }}
      >
        {preview || (
          <span className="text-text-dimmer italic">Empty note</span>
        )}
      </div>
      <div className="font-mono text-[10px] text-text-dimmer flex items-center gap-[6px]">
        <Icon name="clock" size={10} stroke="var(--text-dimmer)" />
        <span>{formatRelative(note.updatedAt)}</span>
      </div>
    </div>
  );
}

function CardIconBtn({
  icon,
  stroke,
  onClick,
  title,
}: {
  icon: import("../../Icon").IconName;
  stroke: string;
  onClick: (e: React.MouseEvent) => void;
  title: string;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      onClick={onClick}
      className="w-[20px] h-[20px] inline-flex items-center justify-center rounded-[3px] hover:bg-surface"
      style={{ background: "transparent", border: "none" }}
    >
      <Icon name={icon} size={11} stroke={stroke} />
    </button>
  );
}

// ---- helpers -------------------------------------------------------------

// Strips HTML tags and collapses whitespace so the preview is a clean 2-line
function stripHtml(html: string): string {
  if (!html) return "";
  // Using a throwaway DOM parser here keeps us honest with entities
  const doc = new DOMParser().parseFromString(html, "text/html");
  const text = doc.body.textContent ?? "";
  return text.replace(/\s+/g, " ").trim();
}

function formatRelative(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const diffMs = Date.now() - d.getTime();
  const mins = Math.floor(diffMs / 60_000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d ago`;
  return d.toISOString().slice(0, 10);
}
