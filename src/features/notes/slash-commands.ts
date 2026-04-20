import type { ChainedCommands, Editor } from "@tiptap/react";

// Atlas - slash menu command palette for the Tiptap note editor.

export interface SlashCommand {
  /** Short filter id, e.g. `"h1"`, `"bullet"`, `"todo"`. */
  id: string;
  /** Human-readable label shown on the first line of the item. */
  label: string;
  /** Secondary hint line. */
  hint: string;
  /** Compact glyph shown in the 26×26 icon tile (prefer monospace). */
  kbd: string;
  // Runs the command against the chain. Chain is already focused + has
  run: (chain: ChainedCommands, editor: Editor) => void;
}

export const SLASH_COMMANDS: SlashCommand[] = [
  {
    id: "h1",
    label: "Heading 1",
    hint: "Large section",
    kbd: "H1",
    run: (c) => c.setNode("heading", { level: 1 }).run(),
  },
  {
    id: "h2",
    label: "Heading 2",
    hint: "Medium section",
    kbd: "H2",
    run: (c) => c.setNode("heading", { level: 2 }).run(),
  },
  {
    id: "h3",
    label: "Heading 3",
    hint: "Small section",
    kbd: "H3",
    run: (c) => c.setNode("heading", { level: 3 }).run(),
  },
  {
    id: "p",
    label: "Paragraph",
    hint: "Plain text",
    kbd: "¶",
    run: (c) => c.setParagraph().run(),
  },
  {
    id: "bullet",
    label: "Bullet list",
    hint: "Unordered list",
    kbd: "•",
    run: (c) => c.toggleBulletList().run(),
  },
  {
    id: "number",
    label: "Numbered list",
    hint: "Ordered list",
    kbd: "1.",
    run: (c) => c.toggleOrderedList().run(),
  },
  {
    id: "todo",
    label: "Task list",
    hint: "Checkboxes",
    kbd: "☐",
    run: (c) => c.toggleTaskList().run(),
  },
  {
    id: "quote",
    label: "Quote",
    hint: "Callout block",
    kbd: "❝",
    run: (c) => c.toggleBlockquote().run(),
  },
  {
    id: "code",
    label: "Code block",
    hint: "Monospaced block",
    kbd: "{ }",
    run: (c) => c.toggleCodeBlock().run(),
  },
  {
    id: "hr",
    label: "Divider",
    hint: "Horizontal rule",
    kbd: "—",
    run: (c) => c.setHorizontalRule().run(),
  },
  {
    id: "table",
    label: "Table",
    hint: "3 × 3 starter",
    kbd: "⊞",
    run: (c) => c.insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run(),
  },
  {
    id: "image",
    label: "Image",
    hint: "From URL",
    kbd: "img",
    run: (c) => {
      // `window.prompt` keeps parity with the prototype. A proper inline
      const url = window.prompt("Image URL");
      if (url) c.setImage({ src: url }).run();
    },
  },
  {
    id: "link",
    label: "Link",
    hint: "URL link on selection",
    kbd: "↗",
    run: (c) => {
      const url = window.prompt("URL");
      if (!url) return;
      c.extendMarkRange("link").setLink({ href: url }).run();
    },
  },
];

// Filter + rank the command list against a user-typed query (the chars
export function filterSlashCommands(
  commands: SlashCommand[],
  query: string,
): SlashCommand[] {
  const q = query.toLowerCase().trim();
  if (!q) return commands;
  const exact: SlashCommand[] = [];
  const rest: SlashCommand[] = [];
  for (const c of commands) {
    if (c.id.startsWith(q)) exact.push(c);
    else if (
      c.label.toLowerCase().includes(q) ||
      c.hint.toLowerCase().includes(q)
    )
      rest.push(c);
  }
  return [...exact, ...rest];
}
