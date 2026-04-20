import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useEditor, EditorContent, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import Link from "@tiptap/extension-link";
import Image from "@tiptap/extension-image";
import Table from "@tiptap/extension-table";
import TableRow from "@tiptap/extension-table-row";
import TableCell from "@tiptap/extension-table-cell";
import TableHeader from "@tiptap/extension-table-header";
import TaskList from "@tiptap/extension-task-list";
import TaskItem from "@tiptap/extension-task-item";
import Highlight from "@tiptap/extension-highlight";
import Underline from "@tiptap/extension-underline";
import TextAlign from "@tiptap/extension-text-align";
import Typography from "@tiptap/extension-typography";
import Placeholder from "@tiptap/extension-placeholder";

import { Icon, type IconName } from "../../components/Icon";
import {
  deleteNote as ipcDeleteNote,
  getNote,
  pinNote as ipcPinNote,
  upsertNote,
} from "../../ipc";
import { useUiStore } from "../../state/store";
import type { Note, Project } from "../../types";
import {
  SLASH_COMMANDS,
  filterSlashCommands,
  type SlashCommand,
} from "./slash-commands";
import { SlashMenu } from "./SlashMenu";

// Atlas - full-page Tiptap note editor overlay.

interface NoteEditorOverlayProps {
  project: Project;
  noteId: string;
  onClose: () => void;
}

interface SlashState {
  /** Viewport-space coords for the popup. */
  x: number;
  y: number;
  query: string;
  activeIdx: number;
  /** Doc positions for `deleteRange({from, to})` when a command fires. */
  from: number;
  to: number;
}

// In-app replacement for `window.prompt()`. WKWebView (the macOS webview
interface UrlPromptState {
  kind: "link" | "image";
  label: string;
  initial: string;
  placeholder: string;
  onSubmit: (url: string) => void;
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
  const [wordCount, setWordCount] = useState(0);
  const [charCount, setCharCount] = useState(0);
  const [slash, setSlash] = useState<SlashState | null>(null);
  // In-app URL prompt - replaces `window.prompt()` which WKWebView
  const [urlPrompt, setUrlPrompt] = useState<UrlPromptState | null>(null);

  // Stash the initial body + id - keeps `doSave` pure without rebinding on
  const noteRef = useRef<Note>(initialNote);
  const slashRef = useRef<SlashState | null>(null);
  slashRef.current = slash;

  // Tiptap editor setup. `StarterKit` includes paragraph, headings, bold,
  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: { levels: [1, 2, 3] },
        codeBlock: { HTMLAttributes: { class: "tt-codeblock" } },
      }),
      Underline,
      Highlight.configure({ multicolor: false }),
      Link.configure({
        openOnClick: false,
        autolink: true,
        HTMLAttributes: { class: "tt-link" },
      }),
      Image.configure({
        inline: false,
        HTMLAttributes: { class: "tt-image" },
      }),
      Table.configure({
        resizable: true,
        HTMLAttributes: { class: "tt-table" },
      }),
      TableRow,
      TableHeader,
      TableCell,
      TaskList.configure({ HTMLAttributes: { class: "tt-tasklist" } }),
      TaskItem.configure({
        nested: true,
        HTMLAttributes: { class: "tt-taskitem" },
      }),
      TextAlign.configure({ types: ["heading", "paragraph"] }),
      Typography,
      Placeholder.configure({ placeholder: "Start writing…" }),
    ],
    content: initialNote.body || "<p></p>",
    autofocus: "end",
    onUpdate: ({ editor }) => {
      const text = editor.getText();
      setCharCount(text.length);
      setWordCount(text.trim() ? text.trim().split(/\s+/).length : 0);
      detectSlash(editor);
    },
    onSelectionUpdate: ({ editor }) => {
      detectSlash(editor);
    },
  });

  // Keep word/char counts in sync on first mount.
  useEffect(() => {
    if (!editor) return;
    const text = editor.getText();
    setCharCount(text.length);
    setWordCount(text.trim() ? text.trim().split(/\s+/).length : 0);
  }, [editor]);

  // ---- slash detection ---------------------------------------------------

  const detectSlash = useCallback((ed: Editor) => {
    const { from, empty } = ed.state.selection;
    if (!empty) {
      setSlash(null);
      return;
    }
    const $from = ed.state.selection.$from;
    const lineStart = $from.start();
    const textBefore = ed.state.doc.textBetween(
      lineStart,
      from,
      "\n",
      "\0",
    );
    // `/query` preceded by start-of-line or a single whitespace character.
    const m = textBefore.match(/(^|\s)(\/[^\s/]*)$/);
    if (!m) {
      setSlash(null);
      return;
    }
    const slashText = m[2]; // e.g. "/hea"
    const query = slashText.slice(1);
    const slashFrom = from - slashText.length;
    const coords = ed.view.coordsAtPos(slashFrom);
    setSlash((prev) => ({
      query,
      activeIdx: prev && prev.query === query ? prev.activeIdx : 0,
      from: slashFrom,
      to: from,
      x: coords.left,
      y: coords.bottom + 6,
    }));
  }, []);

  const filteredSlash = useMemo(() => {
    if (!slash) return [];
    return filterSlashCommands(SLASH_COMMANDS, slash.query);
  }, [slash]);

  const runSlashCommand = useCallback(
    (cmd: SlashCommand) => {
      if (!editor) return;
      const s = slashRef.current;
      if (!s) return;
      // Delete the `/query` characters, then run the command on the clean
      const chain = editor.chain().focus().deleteRange({ from: s.from, to: s.to });
      cmd.run(chain, editor);
      setSlash(null);
    },
    [editor],
  );

  // Capture-phase key handler for the slash menu. Must fire BEFORE Tiptap's
  useEffect(() => {
    if (!slash) return;
    const onKey = (e: KeyboardEvent) => {
      const s = slashRef.current;
      if (!s) return;
      const items = filterSlashCommands(SLASH_COMMANDS, s.query);
      if (e.key === "ArrowDown") {
        e.preventDefault();
        e.stopPropagation();
        setSlash((prev) =>
          prev
            ? {
                ...prev,
                activeIdx:
                  (prev.activeIdx + 1) % Math.max(items.length, 1),
              }
            : prev,
        );
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        e.stopPropagation();
        setSlash((prev) =>
          prev
            ? {
                ...prev,
                activeIdx:
                  (prev.activeIdx - 1 + items.length) %
                  Math.max(items.length, 1),
              }
            : prev,
        );
      } else if (e.key === "Enter" || e.key === "Tab") {
        if (items.length === 0) return;
        e.preventDefault();
        e.stopPropagation();
        const picked = items[s.activeIdx] ?? items[0];
        runSlashCommand(picked);
      } else if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setSlash(null);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [slash, runSlashCommand]);

  // ---- save / delete / pin ----------------------------------------------

  const doSave = useCallback(async () => {
    if (!editor) return;
    const body = editor.getHTML();
    const now = new Date().toISOString();
    const next: Note = {
      ...noteRef.current,
      title: title.trim() || "Untitled note",
      body,
      pinned,
      updatedAt: now,
    };
    try {
      await upsertNote(project.id, next);
      noteRef.current = next;
      setSavedAt(new Date());
      onSaved(next);
    } catch (err) {
      pushToast(
        "error",
        `Couldn't save: ${err instanceof Error ? err.message : String(err)}`,
      );
    }
  }, [editor, title, pinned, project.id, onSaved, pushToast]);

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

  // ⌘S save, Esc close (skip close when focus is inside the prose to let
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void doSave();
        return;
      }
      if (e.key === "Escape") {
        if (slashRef.current) return; // slash menu handles it
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

  const ed = editor;
  const is = (name: string, attrs?: Record<string, unknown>) =>
    ed?.isActive(name, attrs) ?? false;

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
          {savedAt
            ? `saved ${fmtTime(savedAt)}`
            : noteRef.current.body
              ? "editing"
              : "new note"}
        </span>
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

      {/* toolbar */}
      <div
        className="flex items-center gap-[2px] flex-wrap px-[14px] py-[6px] border-b border-line shrink-0"
        style={{ background: "var(--chrome)" }}
      >
        <TBGroup>
          <TBBtn
            title="Undo"
            onClick={() => ed?.chain().focus().undo().run()}
            disabled={!ed?.can().undo()}
          >
            <Svg>
              <path d="M3 8h8a3 3 0 010 6H9" />
              <path d="M6 4L2 8l4 4" />
            </Svg>
          </TBBtn>
          <TBBtn
            title="Redo"
            onClick={() => ed?.chain().focus().redo().run()}
            disabled={!ed?.can().redo()}
          >
            <Svg>
              <path d="M13 8H5a3 3 0 000 6h2" />
              <path d="M10 4l4 4-4 4" />
            </Svg>
          </TBBtn>
        </TBGroup>

        <TBSep />

        <ParagraphSelect editor={ed} />

        <TBSep />

        <TBGroup>
          <TBTextBtn
            title="Bold (⌘B)"
            active={is("bold")}
            onClick={() => ed?.chain().focus().toggleBold().run()}
          >
            <strong style={{ fontSize: 12 }}>B</strong>
          </TBTextBtn>
          <TBTextBtn
            title="Italic (⌘I)"
            active={is("italic")}
            onClick={() => ed?.chain().focus().toggleItalic().run()}
          >
            <em style={{ fontSize: 12, fontFamily: "serif" }}>I</em>
          </TBTextBtn>
          <TBTextBtn
            title="Underline (⌘U)"
            active={is("underline")}
            onClick={() => ed?.chain().focus().toggleUnderline().run()}
          >
            <span style={{ fontSize: 12, textDecoration: "underline" }}>U</span>
          </TBTextBtn>
          <TBTextBtn
            title="Strike"
            active={is("strike")}
            onClick={() => ed?.chain().focus().toggleStrike().run()}
          >
            <span style={{ fontSize: 12, textDecoration: "line-through" }}>
              S
            </span>
          </TBTextBtn>
          <TBTextBtn
            title="Code"
            active={is("code")}
            onClick={() => ed?.chain().focus().toggleCode().run()}
          >
            <span style={{ fontFamily: "var(--mono)", fontSize: 11 }}>
              {"</>"}
            </span>
          </TBTextBtn>
          <TBTextBtn
            title="Highlight"
            active={is("highlight")}
            onClick={() => ed?.chain().focus().toggleHighlight().run()}
          >
            <span
              style={{
                background: "oklch(0.85 0.16 95 / 0.5)",
                padding: "0 3px",
                fontSize: 11,
                borderRadius: 2,
                color: "var(--text)",
              }}
            >
              H
            </span>
          </TBTextBtn>
        </TBGroup>

        <TBSep />

        <TBGroup>
          <TBBtn
            title="Bullet list"
            active={is("bulletList")}
            onClick={() => ed?.chain().focus().toggleBulletList().run()}
          >
            <Svg>
              <path d="M5 4h9M5 8h9M5 12h9" />
              <path d="M2 4h.5M2 8h.5M2 12h.5" />
            </Svg>
          </TBBtn>
          <TBTextBtn
            title="Numbered list"
            active={is("orderedList")}
            onClick={() => ed?.chain().focus().toggleOrderedList().run()}
          >
            <span
              style={{
                fontFamily: "var(--mono)",
                fontSize: 10,
                letterSpacing: -0.5,
              }}
            >
              1.
            </span>
          </TBTextBtn>
          <TBBtn
            title="Task list"
            active={is("taskList")}
            onClick={() => ed?.chain().focus().toggleTaskList().run()}
          >
            <Svg>
              <path d="M2 4h3v3H2zM2 9h3v3H2z" />
              <path d="M7 5.5h7M7 10.5h7" />
            </Svg>
          </TBBtn>
          <TBBtn
            title="Quote"
            active={is("blockquote")}
            onClick={() => ed?.chain().focus().toggleBlockquote().run()}
          >
            <Svg>
              <path d="M4 4v8M4 4h3M4 12h3" />
              <path d="M10 4v8M10 4h3M10 12h3" />
            </Svg>
          </TBBtn>
          <TBTextBtn
            title="Code block"
            active={is("codeBlock")}
            onClick={() => ed?.chain().focus().toggleCodeBlock().run()}
          >
            <span style={{ fontFamily: "var(--mono)", fontSize: 10 }}>
              {"{ }"}
            </span>
          </TBTextBtn>
        </TBGroup>

        <TBSep />

        <TBGroup>
          {(
            [
              ["left", "M2 4h12M2 8h8M2 12h12"],
              ["center", "M2 4h12M4 8h8M2 12h12"],
              ["right", "M2 4h12M6 8h8M2 12h12"],
              ["justify", "M2 4h12M2 8h12M2 12h12"],
            ] as const
          ).map(([a, d]) => (
            <TBBtn
              key={a}
              title={`Align ${a}`}
              active={ed?.isActive({ textAlign: a }) ?? false}
              onClick={() => ed?.chain().focus().setTextAlign(a).run()}
            >
              <Svg>
                <path d={d} />
              </Svg>
            </TBBtn>
          ))}
        </TBGroup>

        <TBSep />

        <TBGroup>
          <TBBtn
            title="Link"
            active={is("link")}
            onClick={() => {
              if (!ed) return;
              const prev = (ed.getAttributes("link").href as string) || "";
              setUrlPrompt({
                kind: "link",
                label: "Link URL",
                initial: prev,
                placeholder: "https://…",
                onSubmit: (url) => {
                  if (url === "") {
                    ed
                      .chain()
                      .focus()
                      .extendMarkRange("link")
                      .unsetLink()
                      .run();
                  } else {
                    ed
                      .chain()
                      .focus()
                      .extendMarkRange("link")
                      .setLink({ href: url })
                      .run();
                  }
                },
              });
            }}
          >
            <Svg>
              <path d="M6 10l4-4" />
              <path d="M7 4l1-1a3 3 0 014 4l-1 1" />
              <path d="M9 12l-1 1a3 3 0 01-4-4l1-1" />
            </Svg>
          </TBBtn>
          <TBBtn
            title="Image"
            onClick={() => {
              if (!ed) return;
              setUrlPrompt({
                kind: "image",
                label: "Image URL",
                initial: "",
                placeholder: "https://…/pic.png",
                onSubmit: (url) => {
                  if (url) ed.chain().focus().setImage({ src: url }).run();
                },
              });
            }}
          >
            <Svg>
              <path d="M2 3h12v10H2z" />
              <path d="M2 11l3-3 2 2 3-3 4 4" />
              <path d="M5 6a1 1 0 110-2 1 1 0 010 2z" />
            </Svg>
          </TBBtn>
          <TBBtn
            title="Horizontal rule"
            onClick={() => ed?.chain().focus().setHorizontalRule().run()}
          >
            <Svg>
              <path d="M2 8h12" />
            </Svg>
          </TBBtn>
          <TBBtn
            title="Table (3×3)"
            onClick={() =>
              ed
                ?.chain()
                .focus()
                .insertTable({ rows: 3, cols: 3, withHeaderRow: true })
                .run()
            }
          >
            <Svg>
              <path d="M2 3h12v10H2zM2 6.5h12M2 10h12M6 3v10M10 3v10" />
            </Svg>
          </TBBtn>
        </TBGroup>

        {/* Table manipulation — only renders when cursor is inside a
            table. Tiptap's `@tiptap/extension-table` exposes chain commands
            for row / column / table mutations; we surface the common ones. */}
        {ed?.isActive("table") && (
          <>
            <TBSep />
            <TBGroup>
              <TBTextBtn
                title="Insert row above"
                onClick={() => ed.chain().focus().addRowBefore().run()}
              >
                +↑R
              </TBTextBtn>
              <TBTextBtn
                title="Insert row below"
                onClick={() => ed.chain().focus().addRowAfter().run()}
              >
                +↓R
              </TBTextBtn>
              <TBTextBtn
                title="Delete row"
                onClick={() => ed.chain().focus().deleteRow().run()}
              >
                −R
              </TBTextBtn>
              <TBTextBtn
                title="Insert column left"
                onClick={() => ed.chain().focus().addColumnBefore().run()}
              >
                +←C
              </TBTextBtn>
              <TBTextBtn
                title="Insert column right"
                onClick={() => ed.chain().focus().addColumnAfter().run()}
              >
                +→C
              </TBTextBtn>
              <TBTextBtn
                title="Delete column"
                onClick={() => ed.chain().focus().deleteColumn().run()}
              >
                −C
              </TBTextBtn>
              <TBTextBtn
                title="Delete table"
                onClick={() => ed.chain().focus().deleteTable().run()}
              >
                ✕T
              </TBTextBtn>
            </TBGroup>
          </>
        )}

        <div className="flex-1" />

        <span className="font-mono text-[10px] text-text-dim">
          {wordCount} words · {charCount} chars
        </span>
      </div>

      {/* body */}
      <div
        className="flex-1 overflow-y-auto"
        style={{ background: "var(--bg)" }}
      >
        <div
          className="mx-auto"
          style={{ maxWidth: 720, padding: "40px 40px 80px" }}
        >
          <input
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            onKeyDown={(e) => onTitleKeyDown(e, editor)}
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
          <div className="relative">
            <EditorContent editor={editor} className="tt-prose" />
          </div>
        </div>
      </div>

      {slash && filteredSlash.length > 0 && (
        <SlashMenu
          x={slash.x}
          y={slash.y}
          query={slash.query}
          items={filteredSlash}
          activeIdx={slash.activeIdx}
          onPick={runSlashCommand}
          onHover={(i) =>
            setSlash((prev) => (prev ? { ...prev, activeIdx: i } : prev))
          }
        />
      )}

      {urlPrompt && (
        <UrlPromptModal
          state={urlPrompt}
          onClose={() => setUrlPrompt(null)}
        />
      )}
    </OverlayShell>
  );
}

// Centered modal that collects a URL for link/image insertion. Rendered
function UrlPromptModal({
  state,
  onClose,
}: {
  state: UrlPromptState;
  onClose: () => void;
}) {
  const [value, setValue] = useState(state.initial);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const submit = (v: string) => {
    state.onSubmit(v);
    onClose();
  };

  return (
    <div
      className="fixed inset-0 z-[160] flex items-center justify-center"
      style={{ background: "rgba(0,0,0,0.45)" }}
      onClick={onClose}
    >
      <div
        className="w-[420px] rounded-[8px] p-4 flex flex-col gap-3"
        style={{
          background: "var(--surface)",
          border: "1px solid var(--line)",
          boxShadow: "0 20px 60px rgba(0,0,0,0.45)",
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="text-[13px] font-semibold text-text">{state.label}</div>
        <input
          ref={inputRef}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder={state.placeholder}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              submit(value.trim());
            } else if (e.key === "Escape") {
              e.preventDefault();
              onClose();
            }
          }}
          className="h-[30px] px-[10px] rounded-[5px] text-[13px]"
          style={{
            background: "var(--surface-2)",
            border: "1px solid var(--line)",
            color: "var(--text)",
            fontFamily: "var(--mono)",
            outline: "none",
          }}
        />
        <div className="flex justify-end gap-2">
          {state.kind === "image" && (
            <button
              onClick={async () => {
                try {
                  const picked = await openDialog({
                    directory: false,
                    multiple: false,
                    filters: [
                      {
                        name: "Image",
                        extensions: [
                          "png",
                          "jpg",
                          "jpeg",
                          "gif",
                          "webp",
                          "svg",
                          "avif",
                          "bmp",
                          "ico",
                        ],
                      },
                    ],
                  });
                  if (typeof picked === "string") {
                    // `convertFileSrc` turns an absolute path into the
                    setValue(convertFileSrc(picked));
                  }
                } catch {
                  /* user cancelled; ignore */
                }
              }}
              className="h-[26px] px-[10px] rounded-[5px] text-[11px]"
              style={{
                background: "transparent",
                border: "1px solid var(--line)",
                color: "var(--text)",
              }}
            >
              Browse…
            </button>
          )}
          <div className="flex-1" />
          {state.kind === "link" && state.initial && (
            <button
              onClick={() => submit("")}
              className="h-[26px] px-[10px] rounded-[5px] text-[11px]"
              style={{
                background: "transparent",
                border: "1px solid var(--line)",
                color: "var(--danger)",
              }}
            >
              Remove
            </button>
          )}
          <button
            onClick={onClose}
            className="h-[26px] px-[10px] rounded-[5px] text-[11px]"
            style={{
              background: "transparent",
              border: "1px solid var(--line)",
              color: "var(--text-dim)",
            }}
          >
            Cancel
          </button>
          <button
            onClick={() => submit(value.trim())}
            className="h-[26px] px-[10px] rounded-[5px] text-[11px] font-medium"
            style={{
              background: "var(--accent)",
              color: "var(--accent-fg)",
              border: "1px solid var(--accent)",
            }}
          >
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

// ---- helpers -------------------------------------------------------------

function onTitleKeyDown(
  e: ReactKeyboardEvent<HTMLInputElement>,
  editor: Editor | null,
) {
  // Enter or ↓ jumps focus into the editor body - matches the prototype's
  if (e.key === "Enter" || e.key === "ArrowDown") {
    e.preventDefault();
    editor?.commands.focus("start");
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

// ---- toolbar primitives --------------------------------------------------

function TBGroup({ children }: { children: ReactNode }) {
  return <div className="flex gap-px">{children}</div>;
}

function TBSep() {
  return (
    <div
      className="w-px h-4 mx-[4px]"
      style={{ background: "var(--line)" }}
      aria-hidden
    />
  );
}

interface TBBtnProps {
  children: ReactNode;
  active?: boolean;
  disabled?: boolean;
  onClick: () => void;
  title: string;
}

function TBBtn({ children, active, disabled, onClick, title }: TBBtnProps) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      disabled={disabled}
      className="w-[26px] h-[26px] inline-flex items-center justify-center rounded-[4px] p-0"
      style={{
        background: active ? "var(--row-active)" : "transparent",
        border: `1px solid ${active ? "var(--line)" : "transparent"}`,
        color: active ? "var(--accent)" : "var(--text-dim)",
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.3 : 1,
      }}
    >
      {children}
    </button>
  );
}

function TBTextBtn({
  children,
  active,
  onClick,
  title,
}: Omit<TBBtnProps, "disabled">) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      className="h-[26px] px-[6px] min-w-[26px] inline-flex items-center justify-center rounded-[4px]"
      style={{
        background: active ? "var(--row-active)" : "transparent",
        border: `1px solid ${active ? "var(--line)" : "transparent"}`,
        color: active ? "var(--accent)" : "var(--text-dim)",
      }}
    >
      {children}
    </button>
  );
}

function Svg({ children }: { children: ReactNode }) {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {children}
    </svg>
  );
}

function ParagraphSelect({ editor }: { editor: Editor | null }) {
  const value = editor?.isActive("heading", { level: 1 })
    ? "h1"
    : editor?.isActive("heading", { level: 2 })
      ? "h2"
      : editor?.isActive("heading", { level: 3 })
        ? "h3"
        : "p";

  // Inline SVG chevron - a `background-image: url(…)` data URI keeps this
  const chevron =
    "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6' viewBox='0 0 10 6'%3E%3Cpath fill='none' stroke='%238a8a8a' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round' d='M1 1l4 4 4-4'/%3E%3C/svg%3E";

  return (
    <select
      value={value}
      onChange={(e) => {
        const v = e.target.value as "p" | "h1" | "h2" | "h3";
        if (!editor) return;
        if (v === "p") editor.chain().focus().setParagraph().run();
        else {
          const level = Number(v[1]) as 1 | 2 | 3;
          editor.chain().focus().toggleHeading({ level }).run();
        }
      }}
      // `appearance: none` removes the macOS native widget chrome that
      style={{
        appearance: "none",
        WebkitAppearance: "none",
        MozAppearance: "none",
        background: `var(--surface-2) url("${chevron}") no-repeat right 8px center`,
        backgroundSize: "10px 6px",
        border: "1px solid var(--line)",
        borderRadius: 4,
        color: "var(--text)",
        fontFamily: "var(--sans)",
        fontSize: 12,
        lineHeight: "24px",
        height: 26,
        cursor: "pointer",
        minWidth: 112,
        paddingLeft: 10,
        paddingRight: 24,
        paddingTop: 0,
        paddingBottom: 0,
      }}
    >
      <option value="p">Paragraph</option>
      <option value="h1">Heading 1</option>
      <option value="h2">Heading 2</option>
      <option value="h3">Heading 3</option>
    </select>
  );
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
