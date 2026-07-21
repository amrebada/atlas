// Atlas - launch-template variable helpers (pure functions).

import { htmlToMarkdown } from "../notes/note-clipboard";
import type { LaunchTemplateVar } from "../../types";

// Matches `{{key}}` placeholders (whitespace-tolerant). Global regexes are
// stateful (`lastIndex`), so every helper builds a fresh instance from the
// exported source instead of iterating this object directly.
export const VAR_RE = /\{\{\s*([A-Za-z0-9_][A-Za-z0-9_.-]*)\s*\}\}/g;

// Fenced code blocks and inline code spans are opaque: a `{{...}}` inside
// them is literal text the model should see verbatim (Jinja, Ansible, Go
// templates, ...), never a variable. This doubles as the escape hatch for
// writing a literal placeholder in a template body.
const CODE_RE = /```[\s\S]*?(?:```|$)|`[^`\n]+`/g;

interface CodeSegment {
  text: string;
  code: boolean;
}

/** Split markdown into alternating segments, flagging fenced code blocks
 *  and inline code spans so callers can leave them verbatim. */
function segmentByCode(markdown: string): CodeSegment[] {
  const re = new RegExp(CODE_RE.source, CODE_RE.flags);
  const out: CodeSegment[] = [];
  let last = 0;
  for (const m of markdown.matchAll(re)) {
    const at = m.index ?? 0;
    if (at > last) out.push({ text: markdown.slice(last, at), code: false });
    out.push({ text: m[0], code: true });
    last = at + m[0].length;
  }
  if (last < markdown.length) {
    out.push({ text: markdown.slice(last), code: false });
  }
  return out;
}

// A Tiptap mark boundary falling inside the braces serializes emphasis
// tokens into the placeholder (bolding just the key yields `{{**issue**}}`),
// which VAR_RE's key charset rejects - strip markers hugging the key so the
// variable still parses. Backticks are deliberately NOT stripped: an inline
// code mark means "literal", consistent with CODE_RE above.
function normalizeMarks(text: string): string {
  return text
    .replace(/\{\{\s*[*_~=]+/g, "{{")
    .replace(/[*_~=]+\s*\}\}/g, "}}");
}

/** Ordered, deduped `{{key}}` names appearing in `markdown` outside code
 *  regions (tolerating emphasis marks that hug the key). */
export function extractVarKeys(markdown: string): string[] {
  const keys: string[] = [];
  const seen = new Set<string>();
  for (const seg of segmentByCode(markdown)) {
    if (seg.code) continue;
    const re = new RegExp(VAR_RE.source, VAR_RE.flags);
    for (const m of normalizeMarks(seg.text).matchAll(re)) {
      const key = m[1];
      if (!seen.has(key)) {
        seen.add(key);
        keys.push(key);
      }
    }
  }
  return keys;
}

/** Replace every `{{key}}` outside code regions with `values[key]`.
 *  Placeholders inside code and placeholders without a provided value are
 *  left verbatim so nothing is silently dropped. */
export function substituteVars(
  markdown: string,
  values: Record<string, string>,
): string {
  return segmentByCode(markdown)
    .map((seg) => {
      if (seg.code) return seg.text;
      const re = new RegExp(VAR_RE.source, VAR_RE.flags);
      return normalizeMarks(seg.text).replace(re, (match, key: string) =>
        Object.prototype.hasOwnProperty.call(values, key)
          ? values[key]
          : match,
      );
    })
    .join("");
}

/** Raw `{{...}}` snippets outside code regions that VAR_RE cannot parse
 *  even after mark normalization (e.g. a mark boundary splitting the key,
 *  or an illegal key charset). Surfaced by the editor's Variables panel so
 *  a mangled placeholder never disappears silently. */
export function findMalformedPlaceholders(markdown: string): string[] {
  const codeRanges: Array<[number, number]> = [];
  const codeRe = new RegExp(CODE_RE.source, CODE_RE.flags);
  for (const m of markdown.matchAll(codeRe)) {
    const at = m.index ?? 0;
    codeRanges.push([at, at + m[0].length]);
  }
  const out: string[] = [];
  for (const m of markdown.matchAll(/\{\{[^{}\n]*\}\}/g)) {
    const at = m.index ?? 0;
    const end = at + m[0].length;
    // Fully inside a code region = an intended literal, not a mistake.
    if (codeRanges.some(([s, e]) => at >= s && end <= e)) continue;
    const re = new RegExp(VAR_RE.source, VAR_RE.flags);
    if (!re.test(normalizeMarks(m[0]))) out.push(m[0]);
  }
  return out;
}

/** Tiptap HTML body -> final prompt: markdown conversion, then variable
 *  substitution, then trim. This exact string becomes the one positional
 *  argv element passed to the `claude` CLI. */
export function renderLaunchPrompt(
  bodyHtml: string,
  values: Record<string, string>,
): string {
  return substituteVars(htmlToMarkdown(bodyHtml), values).trim();
}

/** One entry per detected key, in key order. Existing config is kept for
 *  keys that survive; new keys get a blank config. */
export function syncVariables(
  existing: LaunchTemplateVar[],
  keys: string[],
): LaunchTemplateVar[] {
  const byKey = new Map(existing.map((v) => [v.key, v]));
  return keys.map(
    (key) =>
      byKey.get(key) ?? {
        key,
        label: "",
        default: "",
        hint: "",
        multiline: false,
        options: [],
        required: false,
      },
  );
}
