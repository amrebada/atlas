// Atlas - note clipboard helpers: Tiptap HTML -> Markdown + rich copy.

import { clipboardWriteHtml, clipboardWriteText } from "../../ipc";

// Notes are stored as Tiptap HTML. Copying "as markdown" runs the body
// through the converter below; copying "formatted" puts the HTML itself on
// the clipboard (with the markdown as the text/plain fallback) so rich
// editors paste headings/lists/tables intact. Writes go through the Rust
// clipboard commands, not `navigator.clipboard` - WKWebView's JS clipboard
// layer is invisible to macOS clipboard managers.

export type NoteCopyFormat = "markdown" | "formatted";

export async function copyNoteBody(
  html: string,
  format: NoteCopyFormat,
): Promise<void> {
  const markdown = htmlToMarkdown(html);
  if (format === "markdown") {
    await clipboardWriteText(markdown);
  } else {
    await clipboardWriteHtml(html, markdown);
  }
}

// ---- HTML -> Markdown ----------------------------------------------------

// Covers the node set the note editor can produce: h1-h3, paragraphs,
// bold/italic/strike/underline/highlight/inline code, links, images, bullet /
// ordered / task lists (nested), blockquotes, code blocks, tables, and rules.

export function htmlToMarkdown(html: string): string {
  if (!html) return "";
  const doc = new DOMParser().parseFromString(html, "text/html");
  const md = blocksToMarkdown(doc.body, "");
  // Collapse the blank-line padding each block emits into single separators.
  return md.replace(/\n{3,}/g, "\n\n").trim();
}

function blocksToMarkdown(parent: Node, indent: string): string {
  let out = "";
  parent.childNodes.forEach((node) => {
    out += blockToMarkdown(node, indent);
  });
  return out;
}

function blockToMarkdown(node: Node, indent: string): string {
  if (node.nodeType === Node.TEXT_NODE) {
    const text = node.textContent ?? "";
    return text.trim() ? text : "";
  }
  if (node.nodeType !== Node.ELEMENT_NODE) return "";
  const el = node as HTMLElement;
  const tag = el.tagName.toLowerCase();

  switch (tag) {
    case "h1":
    case "h2":
    case "h3":
    case "h4":
    case "h5":
    case "h6": {
      const level = Number(tag[1]);
      return `${"#".repeat(level)} ${inline(el)}\n\n`;
    }
    case "p": {
      const text = inline(el);
      return text ? `${text}\n\n` : "";
    }
    case "ul":
    case "ol":
      return `${listToMarkdown(el, indent)}\n`;
    case "blockquote": {
      const inner = blocksToMarkdown(el, "").trim();
      const quoted = inner
        .split("\n")
        .map((line) => (line ? `> ${line}` : ">"))
        .join("\n");
      return `${quoted}\n\n`;
    }
    case "pre": {
      const code = el.querySelector("code");
      const lang =
        code?.className.match(/language-([\w+-]+)/)?.[1] ?? "";
      const body = (code ?? el).textContent?.replace(/\n$/, "") ?? "";
      return `\`\`\`${lang}\n${body}\n\`\`\`\n\n`;
    }
    case "hr":
      return "---\n\n";
    case "table":
      return `${tableToMarkdown(el)}\n`;
    case "img":
      return `${imageMd(el)}\n\n`;
    default:
      // Unknown wrapper (div, figure, ...) - recurse into its children.
      return blocksToMarkdown(el, indent);
  }
}

function listToMarkdown(list: HTMLElement, indent: string): string {
  const ordered = list.tagName.toLowerCase() === "ol";
  const isTaskList = list.getAttribute("data-type") === "taskList";
  let out = "";
  let idx = 1;
  list.querySelectorAll(":scope > li").forEach((li) => {
    const marker = ordered ? `${idx}. ` : "- ";
    idx += 1;
    // Tiptap task items carry data-checked on the <li>; the checkbox input
    // itself is presentation-only and skipped by `inline()`.
    const check = isTaskList
      ? li.getAttribute("data-checked") === "true"
        ? "[x] "
        : "[ ] "
      : "";

    // An <li> mixes inline content with nested lists / paragraphs. Render
    // the inline head first, then any nested lists one indent deeper.
    let head = "";
    let nested = "";
    const append = (piece: string) => {
      if (!piece.trim()) return;
      head += (head.trim() ? " " : "") + piece;
    };
    li.childNodes.forEach((child) => {
      const childTag =
        child.nodeType === Node.ELEMENT_NODE
          ? (child as HTMLElement).tagName.toLowerCase()
          : "";
      if (childTag === "ul" || childTag === "ol") {
        nested += listToMarkdown(child as HTMLElement, indent + "  ");
      } else if (child.nodeType === Node.ELEMENT_NODE && childTag !== "p") {
        // Task items wrap content in <div><p>...</p></div>.
        const innerLists = (child as HTMLElement).querySelectorAll(
          ":scope > ul, :scope > ol",
        );
        if (innerLists.length > 0) {
          append(inlineOfDirectNonList(child as HTMLElement));
          innerLists.forEach((l) => {
            nested += listToMarkdown(l as HTMLElement, indent + "  ");
          });
        } else {
          append(inline(child as HTMLElement));
        }
      } else {
        append(inline(child));
      }
    });
    out += `${indent}${marker}${check}${head.trim()}\n`;
    out += nested;
  });
  return out;
}

// Inline content of an element, ignoring its direct nested lists (they are
// rendered separately as indented list blocks).
function inlineOfDirectNonList(el: HTMLElement): string {
  let out = "";
  el.childNodes.forEach((child) => {
    const tag =
      child.nodeType === Node.ELEMENT_NODE
        ? (child as HTMLElement).tagName.toLowerCase()
        : "";
    if (tag === "ul" || tag === "ol") return;
    const piece = inline(child);
    if (piece.trim()) out += (out.trim() ? " " : "") + piece;
  });
  return out;
}

function tableToMarkdown(table: HTMLElement): string {
  const rows: string[][] = [];
  table.querySelectorAll("tr").forEach((tr) => {
    const cells: string[] = [];
    tr.querySelectorAll("th, td").forEach((cell) => {
      cells.push(inline(cell as HTMLElement).replace(/\|/g, "\\|").trim());
    });
    if (cells.length > 0) rows.push(cells);
  });
  if (rows.length === 0) return "";
  const width = Math.max(...rows.map((r) => r.length));
  const pad = (r: string[]) =>
    `| ${Array.from({ length: width }, (_, i) => r[i] ?? "").join(" | ")} |`;
  const sep = `| ${Array.from({ length: width }, () => "---").join(" | ")} |`;
  return [pad(rows[0]), sep, ...rows.slice(1).map(pad)].join("\n") + "\n";
}

function imageMd(el: HTMLElement): string {
  const src = el.getAttribute("src") ?? "";
  const alt = el.getAttribute("alt") ?? "";
  return `![${alt}](${src})`;
}

function inline(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent ?? "";
  if (node.nodeType !== Node.ELEMENT_NODE) return "";
  const el = node as HTMLElement;
  const tag = el.tagName.toLowerCase();
  const children = () => {
    let out = "";
    el.childNodes.forEach((c) => {
      out += inline(c);
    });
    return out;
  };

  switch (tag) {
    case "strong":
    case "b":
      return wrap(children(), "**");
    case "em":
    case "i":
      return wrap(children(), "*");
    case "s":
    case "del":
    case "strike":
      return wrap(children(), "~~");
    case "code":
      return wrap(children(), "`");
    case "mark":
      return wrap(children(), "==");
    case "u":
    case "span":
      return children();
    case "a": {
      const href = el.getAttribute("href") ?? "";
      const text = children() || href;
      return href ? `[${text}](${href})` : text;
    }
    case "img":
      return imageMd(el);
    case "br":
      return "\n";
    case "input":
    case "label":
      // Task-item checkbox chrome - the state comes from data-checked.
      return "";
    case "p":
    case "div":
      return children();
    default:
      return children();
  }
}

// Emphasis markers must hug non-whitespace or renderers ignore them - move
// the wrapped text's edge spaces outside the markers.
function wrap(text: string, marker: string): string {
  if (!text.trim()) return text;
  const lead = text.match(/^\s*/)?.[0] ?? "";
  const trail = text.match(/\s*$/)?.[0] ?? "";
  return `${lead}${marker}${text.trim()}${marker}${trail}`;
}
