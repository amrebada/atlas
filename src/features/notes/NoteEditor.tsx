import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { Icon, type IconName } from "../../components/Icon";
import {
  deleteNote as ipcDeleteNote,
  getNote,
  pinNote as ipcPinNote,
  upsertNote,
} from "../../ipc";
import { useUiStore } from "../../state/store";
import type { Note, Project } from "../../types";
import { RichTextEditor } from "./RichTextEditor";
import { copyNoteBody, type NoteCopyFormat } from "./note-clipboard";

// Atlas - full-page Tiptap note editor overlay.

interface NoteEditorOverlayProps {
  project: Project;
  noteId: string;
  onClose: () => void;
}

export function NoteEditorOverlay({
  project,
  noteId,
  onClose,
}: NoteEditorOverlayProps) {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);

  const { data: note, isLoading } = useQuery<Note | null>({
    queryKey: ["note", project.id, noteId],
    queryFn: () => getNote(project.id, noteId),
    staleTime: Infinity,
    retry: false,
  });

  if (isLoading || !note) {
    return (
      <OverlayShell>
        <div className="flex-1 flex items-center justify-center text-text-dimmer font-mono text-[12px]">
          loading note…
        </div>
      </OverlayShell>
    );
  }

  return (
    <NoteEditorInner
      project={project}
      initialNote={note}
      onClose={onClose}
      onSaved={(updated) => {
        // Keep both the single-note and the list caches consistent.
        queryClient.setQueryData<Note>(["note", project.id, updated.id], updated);
        queryClient.setQueryData<Note[]>(["notes", project.id], (old) => {
          if (!old) return old;
          const i = old.findIndex((n) => n.id === updated.id);
          if (i === -1) return [updated, ...old];
          const next = old.slice();
          next[i] = updated;
          return next;
        });
      }}
      onDeleted={(id) => {
        queryClient.setQueryData<Note[]>(["notes", project.id], (old) =>
          old ? old.filter((n) => n.id !== id) : old,
        );
      }}
      pushToast={pushToast}
    />
  );
}

// ---- wrapper -------------------------------------------------------------

function OverlayShell({ children }: { children: ReactNode }) {
  return createPortal(
    <div
      className="fixed inset-0 z-[150] flex flex-col"
      style={{
        background: "var(--bg)",
        color: "var(--text)",
        fontFamily: "var(--sans)",
      }}
      role="dialog"
      aria-modal="true"
    >
      {children}
    </div>,
    document.body,
  );
}

// ---- main editor ---------------------------------------------------------

interface InnerProps {
  project: Project;
  initialNote: Note;
  onClose: () => void;
  onSaved: (note: Note) => void;
  onDeleted: (id: string) => void;
  pushToast: (
    kind: "info" | "success" | "warn" | "error",
    msg: string,
  ) => void;
}

function NoteEditorInner({
  project,
  initialNote,
  onClose,
  onSaved,
  onDeleted,
  pushToast,
}: InnerProps) {
  const [title, setTitle] = useState(initialNote.title);
  const [pinned, setPinned] = useState(initialNote.pinned);
  const [savedAt, setSavedAt] = useState<Date | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [isDirty, setIsDirty] = useState(false);

  // Stash the initial body + id - keeps `doSave` pure without rebinding on
  const noteRef = useRef<Note>(initialNote);
  // Latest editor HTML. RichTextEditor is uncontrolled; its onChange keeps
  // this ref current so the debounced auto-save and the unmount flush always
  // persist exactly what is on screen.
  const bodyHtmlRef = useRef<string>(initialNote.body);
  // Debounce handle for auto-save. Each edit resets the timer; a save fires
  // 800ms after the user stops typing.
  const autoSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // ---- save / delete / pin ----------------------------------------------

  const doSave = useCallback(async () => {
    const next: Note = {
      ...noteRef.current,
      title: title.trim() || "Untitled note",
      body: bodyHtmlRef.current,
      pinned,
      updatedAt: new Date().toISOString(),
    };
    setIsSaving(true);
    try {
      await upsertNote(project.id, next);
      noteRef.current = next;
      setSavedAt(new Date());
      setIsDirty(false);
      onSaved(next);
    } catch (err) {
      pushToast(
        "error",
        `Couldn't save: ${err instanceof Error ? err.message : String(err)}`,
      );
    } finally {
      setIsSaving(false);
    }
  }, [title, pinned, project.id, onSaved, pushToast]);

  // Stable wrapper around the latest doSave so the debounced timer always
  // fires the most recent closure (title/body/pinned change between edits).
  const doSaveRef = useRef(doSave);
  useEffect(() => {
    doSaveRef.current = doSave;
  }, [doSave]);

  const scheduleAutoSave = useCallback(() => {
    if (autoSaveTimerRef.current) clearTimeout(autoSaveTimerRef.current);
    autoSaveTimerRef.current = setTimeout(() => {
      autoSaveTimerRef.current = null;
      void doSaveRef.current();
    }, 800);
  }, []);

  const onBodyChange = useCallback(
    (html: string) => {
      bodyHtmlRef.current = html;
      setIsDirty(true);
      scheduleAutoSave();
    },
    [scheduleAutoSave],
  );

  // Flush pending auto-save before unmount (e.g. closing the overlay) so
  // the user never loses in-flight edits.
  useEffect(() => {
    return () => {
      if (autoSaveTimerRef.current) {
        clearTimeout(autoSaveTimerRef.current);
        autoSaveTimerRef.current = null;
        void doSaveRef.current();
      }
    };
  }, []);

  const doDelete = useCallback(async () => {
    if (!window.confirm("Delete this note?")) return;
    try {
      await ipcDeleteNote(project.id, noteRef.current.id);
      onDeleted(noteRef.current.id);
      pushToast("info", "Note deleted");
      onClose();
    } catch (err) {
      pushToast(
        "error",
        `Couldn't delete: ${err instanceof Error ? err.message : String(err)}`,
      );
    }
  }, [project.id, onDeleted, onClose, pushToast]);

  const togglePin = useCallback(async () => {
    const next = !pinned;
    setPinned(next);
    try {
      await ipcPinNote(project.id, noteRef.current.id, next);
      noteRef.current = { ...noteRef.current, pinned: next };
    } catch (err) {
      // Roll back local state so the UI reflects reality.
      setPinned(!next);
      pushToast(
        "error",
        `Couldn't ${next ? "pin" : "unpin"}: ${err instanceof Error ? err.message : String(err)}`,
      );
    }
  }, [pinned, project.id, pushToast]);

  const doCopy = useCallback(
    async (format: NoteCopyFormat) => {
      try {
        await copyNoteBody(bodyHtmlRef.current, format);
        pushToast(
          "success",
          format === "markdown" ? "Copied as Markdown" : "Copied formatted",
        );
      } catch (err) {
        pushToast(
          "error",
          `Copy failed: ${err instanceof Error ? err.message : String(err)}`,
        );
      }
    },
    [pushToast],
  );

  // ⌘S save, Esc close (skip close when focus is inside the prose to let
  // the editor consume it; when the slash menu is open, RichTextEditor's
  // capture-phase handler stops Escape before it reaches this listener).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void doSave();
        return;
      }
      if (e.key === "Escape") {
        const active = document.activeElement as HTMLElement | null;
        if (active && active.closest(".tt-prose")) return;
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [doSave, onClose]);

  // ---- render ------------------------------------------------------------

  const saveAndClose = async () => {
    await doSave();
    setTimeout(onClose, 80);
  };

  return (
    <OverlayShell>
      {/* header — 78px left inset to clear the native macOS traffic lights
          (same convention as the main TitleBar). `data-tauri-drag-region`
          lets the user drag the window from any empty area of the bar. */}
      <div
        data-tauri-drag-region
        className="flex items-center gap-[10px] pr-[18px] h-[50px] border-b border-line shrink-0"
        style={{ background: "var(--chrome)", paddingLeft: 78 }}
      >
        <button
          type="button"
          data-tauri-drag-region="false"
          onClick={saveAndClose}
          className="inline-flex items-center gap-[5px] h-[26px] px-[10px] rounded-[5px] border border-line text-text-dim text-[12px] hover:text-text"
          style={{ background: "transparent" }}
        >
          <Icon
            name="chevron"
            size={11}
            style={{ transform: "rotate(180deg)" }}
          />
          Back
        </button>
        <div
          data-tauri-drag-region
          className="flex items-center gap-[6px] text-text-dim text-[12px]"
        >
          <Icon name="folder" size={12} />
          <span>{project.name}</span>
          <Icon name="chevron" size={10} />
          <Icon name="note" size={12} />
          <span className="text-text truncate max-w-[300px]">
            {title || "Untitled note"}
          </span>
        </div>
        <div data-tauri-drag-region className="flex-1" />
        <span
          data-tauri-drag-region
          className="font-mono text-[10px] text-text-dim"
        >
          {isSaving
            ? "saving…"
            : isDirty
              ? "unsaved changes"
              : savedAt
                ? `saved ${fmtTime(savedAt)}`
                : noteRef.current.body
                  ? "editing"
                  : "new note"}
        </span>
        <button
          type="button"
          title="Copy as Markdown"
          onClick={() => void doCopy("markdown")}
          className="h-[26px] px-[7px] inline-flex items-center justify-center rounded-[5px] font-mono text-[10px] font-semibold"
          style={{
            background: "transparent",
            border: "1px solid var(--line)",
            color: "var(--text-dim)",
          }}
        >
          MD
        </button>
        <IconButton
          title="Copy formatted"
          onClick={() => void doCopy("formatted")}
          icon="copy"
          stroke="var(--text-dim)"
        />
        <IconButton
          title={pinned ? "Unpin" : "Pin"}
          onClick={togglePin}
          icon={pinned ? "pin-fill" : "pin"}
          stroke={pinned ? "var(--accent)" : "var(--text-dim)"}
        />
        <IconButton
          title="Delete"
          onClick={doDelete}
          icon="trash"
          stroke="var(--text-dim)"
        />
        <div
          className="w-px h-[18px] mx-[4px]"
          style={{ background: "var(--line)" }}
        />
        <button
          type="button"
          onClick={doSave}
          className="inline-flex items-center gap-[6px] h-[26px] px-[12px] rounded-[5px] font-semibold text-[12px]"
          style={{
            background: "var(--accent)",
            color: "var(--accent-fg)",
            border: "none",
          }}
        >
          Save <Kbd>⌘</Kbd>
          <Kbd>S</Kbd>
        </button>
      </div>

      {/* toolbar + body (shared Tiptap core). The title input + meta row
          render through `docHeader` so they keep scrolling with the doc. */}
      <RichTextEditor
        initialHTML={initialNote.body}
        onChange={onBodyChange}
        autoFocus
        docHeader={(focusBody) => (
          <>
            <input
              value={title}
              onChange={(e) => {
                setTitle(e.target.value);
                setIsDirty(true);
                scheduleAutoSave();
              }}
              onKeyDown={(e) => onTitleKeyDown(e, focusBody)}
              placeholder="Untitled note"
              className="w-full py-[4px] mb-[12px] bg-transparent border-0 outline-none text-text"
              style={{
                fontSize: 30,
                fontWeight: 700,
                letterSpacing: "-0.4px",
                fontFamily: "var(--sans)",
              }}
            />
            <div
              className="flex items-center gap-[8px] font-mono text-[11px] text-text-dim mb-[28px] pb-[14px] border-b border-line-soft"
            >
              <Icon name="clock" size={11} />
              <span>{formatRelative(noteRef.current.updatedAt)}</span>
              {pinned && (
                <>
                  <span>·</span>
                  <Icon name="pin-fill" size={10} stroke="var(--accent)" />
                  <span className="text-accent">pinned</span>
                </>
              )}
              <span>·</span>
              <span>{project.name}</span>
            </div>
          </>
        )}
      />
    </OverlayShell>
  );
}

// ---- helpers -------------------------------------------------------------

function onTitleKeyDown(
  e: ReactKeyboardEvent<HTMLInputElement>,
  focusBody: (pos?: "start" | "end") => void,
) {
  // Enter or ↓ jumps focus into the editor body - matches the prototype's
  if (e.key === "Enter" || e.key === "ArrowDown") {
    e.preventDefault();
    focusBody("start");
  }
}

function fmtTime(d: Date): string {
  return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
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

function IconButton({
  icon,
  stroke,
  title,
  onClick,
}: {
  icon: IconName;
  stroke: string;
  title: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      className="w-[26px] h-[26px] inline-flex items-center justify-center rounded-[5px]"
      style={{
        background: "transparent",
        border: "1px solid var(--line)",
        color: "var(--text-dim)",
      }}
    >
      <Icon name={icon} size={13} stroke={stroke} />
    </button>
  );
}

function Kbd({ children }: { children: ReactNode }) {
  return (
    <span
      className="inline-flex items-center justify-center rounded-[3px] font-mono"
      style={{
        padding: "0 3px",
        minWidth: 14,
        height: 14,
        fontSize: 10,
        color: "var(--accent-fg)",
        background: "rgba(0,0,0,0.15)",
      }}
    >
      {children}
    </span>
  );
}
