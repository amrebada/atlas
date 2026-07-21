import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
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

import {
  SLASH_COMMANDS,
  filterSlashCommands,
  type SlashCommand,
} from "./slash-commands";
import { SlashMenu } from "./SlashMenu";

// Atlas - reusable Tiptap rich-text editor core: extension setup, formatting
// toolbar, slash-command menu and the prose body. Extracted from
// NoteEditor.tsx so other features (e.g. launch templates) can embed the
// same editor without the note-specific chrome.

export interface RichTextEditorProps {
  /** Initial document HTML. The editor is uncontrolled after mount. */
  initialHTML: string;
  /** Called on every doc change with `editor.getHTML()`. */
  onChange: (html: string) => void;
  /** Empty-document placeholder (defaults to "Start writing…"). */
  placeholder?: string;
  /** Appended to SLASH_COMMANDS for this editor instance only. */
  extraSlashCommands?: SlashCommand[];
  /** Default true; pass false to omit the Typography extension. */
  typography?: boolean;
  autoFocus?: boolean;
  /**
   * z-index for the slash-command menu (portaled to document.body, so it
   * stacks against the host's overlay, not inside it). Hosts rendered in a
   * high-z overlay must pass a value above their overlay layer or the menu
   * paints underneath it while still capturing keys. Default 200.
   */
  slashMenuZIndex?: number;
  /**
   * Optional slot rendered inside the scrollable column, above the prose,
   * so it scrolls with the document. Receives a callback that focuses the
   * editor body. NoteEditor uses this for its title input + meta row.
   */
  docHeader?: (focusBody: (pos?: "start" | "end") => void) => ReactNode;
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

// In-app replacement for `window.prompt()`, which WKWebView (the macOS
// webview Tauri embeds) silently ignores.
interface UrlPromptState {
  kind: "link" | "image";
  label: string;
  initial: string;
  placeholder: string;
  onSubmit: (url: string) => void;
}

export function RichTextEditor({
  initialHTML,
  onChange,
  placeholder,
  extraSlashCommands,
  typography = true,
  autoFocus = false,
  slashMenuZIndex,
  docHeader,
}: RichTextEditorProps) {
  const [wordCount, setWordCount] = useState(0);
  const [charCount, setCharCount] = useState(0);
  const [slash, setSlash] = useState<SlashState | null>(null);
  // In-app URL prompt - replaces `window.prompt()` which WKWebView breaks.
  const [urlPrompt, setUrlPrompt] = useState<UrlPromptState | null>(null);

  const slashRef = useRef<SlashState | null>(null);
  slashRef.current = slash;

  // Latest onChange without rebuilding the editor when the parent re-renders.
  const onChangeRef = useRef(onChange);
  useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);

  // Per-instance command palette: the shared set plus any host-provided
  // extras (e.g. the launch-template "variable" command).
  const commands = useMemo(
    () =>
      extraSlashCommands && extraSlashCommands.length > 0
        ? [...SLASH_COMMANDS, ...extraSlashCommands]
        : SLASH_COMMANDS,
    [extraSlashCommands],
  );

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

  // Tiptap editor setup. `StarterKit` includes paragraph, headings, bold,
  // italic, lists, history and the other core nodes/marks.
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
      ...(typography ? [Typography] : []),
      Placeholder.configure({
        placeholder: placeholder ?? "Start writing…",
      }),
    ],
    content: initialHTML || "<p></p>",
    autofocus: autoFocus ? "end" : false,
    onUpdate: ({ editor }) => {
      const text = editor.getText();
      setCharCount(text.length);
      setWordCount(text.trim() ? text.trim().split(/\s+/).length : 0);
      onChangeRef.current(editor.getHTML());
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

  const focusBody = useCallback(
    (pos: "start" | "end" = "start") => {
      editor?.commands.focus(pos);
    },
    [editor],
  );

  const filteredSlash = useMemo(() => {
    if (!slash) return [];
    return filterSlashCommands(commands, slash.query);
  }, [slash, commands]);

  const runSlashCommand = useCallback(
    (cmd: SlashCommand) => {
      if (!editor) return;
      const s = slashRef.current;
      if (!s) return;
      // Delete the `/query` characters, then run the command on the clean
      // document.
      const chain = editor.chain().focus().deleteRange({ from: s.from, to: s.to });
      cmd.run(chain, editor);
      setSlash(null);
    },
    [editor],
  );

  // Capture-phase key handler for the slash menu. Must fire BEFORE Tiptap's
  // own keydown handling; stopPropagation also keeps host-level Escape
  // listeners (e.g. NoteEditor's overlay close) from reacting while open.
  useEffect(() => {
    if (!slash) return;
    const onKey = (e: KeyboardEvent) => {
      const s = slashRef.current;
      if (!s) return;
      const items = filterSlashCommands(commands, s.query);
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
  }, [slash, runSlashCommand, commands]);

  // ---- render ------------------------------------------------------------

  const ed = editor;
  const is = (name: string, attrs?: Record<string, unknown>) =>
    ed?.isActive(name, attrs) ?? false;

  return (
    <div className="flex-1 min-h-0 flex flex-col">
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
          {docHeader?.(focusBody)}
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
          zIndex={slashMenuZIndex}
        />
      )}

      {urlPrompt && (
        <UrlPromptModal
          state={urlPrompt}
          onClose={() => setUrlPrompt(null)}
        />
      )}
    </div>
  );
}

// Centered modal that collects a URL for link/image insertion. Rendered
// in place of `window.prompt()`, which WKWebView does not support.
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

  // The prompt owns Escape while mounted, no matter where focus sits (a
  // native file dialog can blur the input, leaving activeElement on body).
  // Capture phase + stopPropagation keeps host overlays (note editor,
  // launch-template editor, global shortcuts) from also closing on the
  // same press — Esc dismisses just this prompt. Hosts with their own
  // capture-phase Escape listeners must yield while `[data-url-prompt]`
  // is in the DOM, since their listeners registered first and fire first.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  const submit = (v: string) => {
    state.onSubmit(v);
    onClose();
  };

  return (
    <div
      data-url-prompt=""
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
              // Normally unreachable - the window capture listener above
              // fires first - but kept as a belt-and-braces fallback.
              e.preventDefault();
              e.stopPropagation();
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
                    // asset: URL the webview is allowed to load.
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
  // free of extra asset files.
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
      // otherwise ignores our colors.
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
