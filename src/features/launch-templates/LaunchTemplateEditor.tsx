// Atlas - full-overlay editor for one Claude Code launch template.

import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Icon } from "../../components/Icon";
import { upsertLaunchTemplate } from "../../ipc";
import { useUiStore } from "../../state/store";
import {
  GHOST_BTN,
  INPUT_STYLE,
  PRIMARY_BTN,
} from "../settings/SettingsPanel";
import { htmlToMarkdown } from "../notes/note-clipboard";
import { RichTextEditor } from "../notes/RichTextEditor";
import type { SlashCommand } from "../notes/slash-commands";
import {
  extractVarKeys,
  findMalformedPlaceholders,
  syncVariables,
} from "./template-vars";
import type { LaunchTemplate, LaunchTemplateVar } from "../../types";

// Sits above the settings panel (z 400) and its list-level confirm dialogs
// (z 410); the delete confirm opened from THIS editor renders at z 430 so
// the editor survives underneath it. The body is the notion-like Tiptap
// editor; the variables panel below it live-syncs against the `{{key}}`
// placeholders detected in the body.

interface LaunchTemplateEditorProps {
  /** `null` = create a new template. */
  template: LaunchTemplate | null;
  onClose: () => void;
  /** Existing templates only - the section owns the confirm dialog; the
   *  editor stays mounted underneath it so Cancel keeps unsaved edits. */
  onDelete: (template: LaunchTemplate) => void;
  /** True while the section's delete-confirm dialog is stacked above this
   *  editor - the editor then leaves Escape to the dialog. */
  deleteConfirmOpen?: boolean;
}

// Extra slash command for this editor instance: inserts the literal
// placeholder text - the user then renames `name` in place.
const VARIABLE_SLASH_COMMANDS: SlashCommand[] = [
  {
    id: "variable",
    label: "Variable",
    hint: "Insert a {{name}} placeholder",
    kbd: "{ }",
    run: (c) => c.insertContent("{{name}}").run(),
  },
];

// Same palette as the scaffold TemplatesSection swatch row.
const SWATCHES = [
  "#3178C6",
  "#E0763C",
  "#3572A5",
  "#00ADD8",
  "#7c7fee",
  "#d97757",
  "#78c98a",
  "#c77eff",
  "#888",
];

const blankVar = (key: string): LaunchTemplateVar => ({
  key,
  label: "",
  default: "",
  hint: "",
  multiline: false,
  options: [],
  required: false,
});

export function LaunchTemplateEditor({
  template,
  onClose,
  onDelete,
  deleteConfirmOpen = false,
}: LaunchTemplateEditorProps) {
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();

  const [label, setLabel] = useState(template?.label ?? "");
  const [hint, setHint] = useState(template?.hint ?? "");
  const [color, setColor] = useState(template?.color || "#7c7fee");
  const [bodyHtml, setBodyHtml] = useState(template?.body ?? "");
  // Every var config ever seen this session - never pruned, so a key that
  // momentarily disappears mid-edit keeps its config when retyped. The
  // persisted list is the synced projection below.
  const [varConfigs, setVarConfigs] = useState<LaunchTemplateVar[]>(
    template?.variables ?? [],
  );

  const bodyMarkdown = useMemo(() => htmlToMarkdown(bodyHtml), [bodyHtml]);
  const detectedKeys = useMemo(
    () => extractVarKeys(bodyMarkdown),
    [bodyMarkdown],
  );
  // `{{...}}` snippets the parser cannot read (e.g. a bold/italic boundary
  // splitting the key) - warned about below so they never vanish silently.
  const malformed = useMemo(
    () => findMalformedPlaceholders(bodyMarkdown),
    [bodyMarkdown],
  );
  const variables = useMemo(
    () => syncVariables(varConfigs, detectedKeys),
    [varConfigs, detectedKeys],
  );

  const updateVar = (key: string, patch: Partial<LaunchTemplateVar>) => {
    setVarConfigs((prev) => {
      const base = prev.find((v) => v.key === key) ?? blankVar(key);
      return [
        ...prev.filter((v) => v.key !== key),
        { ...base, ...patch, key },
      ];
    });
  };

  const upsertMut = useMutation({
    mutationFn: upsertLaunchTemplate,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["launch-templates"] });
      onClose();
    },
    onError: (err) => pushToast("error", `Save failed: ${String(err)}`),
  });

  const canSave = label.trim().length > 0 && !upsertMut.isPending;

  const save = () => {
    if (!canSave) return;
    const now = new Date().toISOString();
    upsertMut.mutate({
      id: template?.id ?? crypto.randomUUID(),
      label: label.trim(),
      hint: hint.trim(),
      color,
      body: bodyHtml,
      variables,
      createdAt: template?.createdAt ?? now,
      updatedAt: now,
    });
  };

  // Esc closes the overlay - unless focus is inside the Tiptap body, which
  // uses Esc itself (same guard NoteEditor applies to its window listener).
  // Capture phase + stopPropagation: this editor stacks ABOVE the settings
  // panel, whose window-level (bubble) Escape handling in useGlobalShortcuts
  // would otherwise close the whole settings overlay and discard unsaved
  // edits on every Esc press.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      // The delete-confirm dialog is stacked above - Esc belongs to it.
      if (deleteConfirmOpen) return;
      // The link/image URL prompt owns Escape while it is MOUNTED (its own
      // window capture listener, registered after this one, closes it and
      // stops the event) - guard on existence, not focus: a native file
      // dialog can blur the prompt's input, leaving focus on <body>.
      if (document.querySelector("[data-url-prompt]")) return;
      e.stopPropagation();
      // Inside the prose, leave Esc to the editor (the slash-menu handler
      // is also capture-phase on window, so it still fires after this).
      const active = document.activeElement as HTMLElement | null;
      if (active && active.closest(".tt-prose")) return;
      e.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose, deleteConfirmOpen]);

  return createPortal(
    <div
      onClick={onClose}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 420,
        background: "rgba(0,0,0,0.45)",
        backdropFilter: "blur(4px)",
        WebkitBackdropFilter: "blur(4px)",
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        paddingTop: "6vh",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label={template ? "Edit session template" : "New session template"}
        style={{
          width: 720,
          maxWidth: "92vw",
          maxHeight: "86vh",
          display: "flex",
          flexDirection: "column",
          background: "var(--surface)",
          border: "1px solid var(--line)",
          borderRadius: 10,
          overflow: "hidden",
          boxShadow: "0 30px 80px rgba(0,0,0,0.55)",
          fontFamily: "var(--sans)",
          color: "var(--text)",
        }}
      >
        {/* Header */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "12px 16px",
            borderBottom: "1px solid var(--line)",
            background: "var(--chrome)",
            flexShrink: 0,
          }}
        >
          <Icon name="sparkle" size={13} stroke="var(--accent)" />
          <span style={{ fontSize: 13, fontWeight: 600, flex: 1 }}>
            {template ? "Edit session template" : "New session template"}
          </span>
          <button
            onClick={onClose}
            aria-label="Close"
            style={{
              background: "none",
              border: "none",
              color: "var(--text-dim)",
              cursor: "pointer",
              fontSize: 16,
              padding: "0 2px",
            }}
          >
            ×
          </button>
        </div>

        {/* Scrollable body */}
        <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: 16 }}>
          <div style={{ display: "flex", gap: 8, marginBottom: 8 }}>
            <input
              autoFocus
              value={label}
              onChange={(e) => setLabel(e.target.value)}
              placeholder="Template name"
              style={{ ...INPUT_STYLE, flex: 1 }}
            />
            <div style={{ display: "flex", gap: 3, alignItems: "center" }}>
              {SWATCHES.map((c) => (
                <button
                  key={c}
                  onClick={() => setColor(c)}
                  title={c}
                  style={{
                    width: 18,
                    height: 18,
                    borderRadius: "50%",
                    background: c,
                    border:
                      "2px solid " +
                      (color === c ? "var(--text)" : "transparent"),
                    cursor: "pointer",
                    padding: 0,
                  }}
                />
              ))}
            </div>
          </div>
          <input
            value={hint}
            onChange={(e) => setHint(e.target.value)}
            placeholder="Short description (optional — shown in the session dropdown)"
            style={{ ...INPUT_STYLE, width: "100%", marginBottom: 12 }}
          />

          {/* Prompt body */}
          <div
            style={{
              fontSize: 11,
              fontWeight: 600,
              letterSpacing: 0.4,
              textTransform: "uppercase",
              color: "var(--text-dim)",
              marginBottom: 6,
            }}
          >
            Prompt
          </div>
          <div
            style={{
              border: "1px solid var(--line)",
              borderRadius: 6,
              background: "var(--bg)",
              minHeight: 200,
              marginBottom: 6,
              overflow: "hidden",
            }}
          >
            <RichTextEditor
              initialHTML={template?.body ?? ""}
              onChange={setBodyHtml}
              placeholder="Write the prompt for the new session…"
              extraSlashCommands={VARIABLE_SLASH_COMMANDS}
              typography={false}
              // The menu portals to document.body, so it stacks against
              // this overlay (420) and the delete confirm (430), not
              // inside them - its default 200 would paint underneath.
              slashMenuZIndex={500}
            />
          </div>
          <div
            style={{
              fontSize: 11,
              color: "var(--text-dim)",
              marginBottom: 16,
            }}
          >
            Insert variables with /variable or type {"{{name}}"}
          </div>

          {/* Variables */}
          <div
            style={{
              fontSize: 11,
              fontWeight: 600,
              letterSpacing: 0.4,
              textTransform: "uppercase",
              color: "var(--text-dim)",
              marginBottom: 6,
            }}
          >
            Variables
          </div>
          {malformed.length > 0 && (
            <div
              style={{
                fontSize: 11,
                fontFamily: "var(--mono)",
                color: "var(--warn, #d97757)",
                padding: "2px 0 10px",
              }}
            >
              Not read as {malformed.length === 1 ? "a variable" : "variables"}
              : {malformed.join(", ")} — formatting inside the braces breaks
              the key. Clear the marks, or wrap it in code to keep it
              literal.
            </div>
          )}
          {variables.length === 0 && (
            <div
              style={{
                fontSize: 11,
                fontFamily: "var(--mono)",
                color: "var(--text-dimmer)",
                padding: "6px 0 10px",
              }}
            >
              No variables detected in the prompt.
            </div>
          )}
          {variables.map((v) => (
            <VarConfigRow
              key={v.key}
              variable={v}
              onChange={(patch) => updateVar(v.key, patch)}
            />
          ))}
        </div>

        {/* Footer */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "12px 16px",
            borderTop: "1px solid var(--line)",
            flexShrink: 0,
          }}
        >
          {template && (
            <button
              onClick={() => onDelete(template)}
              style={{
                ...GHOST_BTN,
                color: "var(--danger)",
                borderColor: "var(--danger)",
              }}
            >
              Delete…
            </button>
          )}
          <div style={{ flex: 1 }} />
          <button onClick={onClose} style={GHOST_BTN}>
            Cancel
          </button>
          <button
            onClick={save}
            disabled={!canSave}
            style={{
              ...PRIMARY_BTN,
              opacity: canSave ? 1 : 0.5,
              cursor: canSave ? "pointer" : "default",
            }}
          >
            {upsertMut.isPending
              ? "Saving…"
              : template
                ? "Save"
                : "Add template"}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

// ---- per-variable config row ----------------------------------------------

function VarConfigRow({
  variable,
  onChange,
}: {
  variable: LaunchTemplateVar;
  onChange: (patch: Partial<LaunchTemplateVar>) => void;
}) {
  const small: React.CSSProperties = {
    ...INPUT_STYLE,
    fontSize: 12,
    padding: "5px 8px",
  };
  return (
    <div
      style={{
        padding: "10px 12px",
        marginBottom: 8,
        borderRadius: 6,
        background: "var(--surface-2)",
        border: "1px solid var(--line-soft)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          marginBottom: 8,
        }}
      >
        <code
          style={{
            fontFamily: "var(--mono)",
            fontSize: 11,
            padding: "2px 7px",
            borderRadius: 3,
            background: "var(--bg)",
            color: "var(--accent)",
          }}
        >
          {`{{${variable.key}}}`}
        </code>
        <div style={{ flex: 1 }} />
        <FlagToggle
          label="multiline"
          on={variable.multiline}
          onChange={(multiline) => onChange({ multiline })}
        />
        <FlagToggle
          label="required"
          on={variable.required}
          onChange={(required) => onChange({ required })}
        />
      </div>
      <div style={{ display: "flex", gap: 6, marginBottom: 6 }}>
        <input
          value={variable.label}
          onChange={(e) => onChange({ label: e.target.value })}
          placeholder="Label (shown in the wizard)"
          style={{ ...small, flex: 1 }}
        />
        <input
          value={variable.default}
          onChange={(e) => onChange({ default: e.target.value })}
          placeholder="Default value"
          style={{ ...small, flex: 1, fontFamily: "var(--mono)" }}
        />
      </div>
      <div style={{ display: "flex", gap: 6 }}>
        <input
          value={variable.hint}
          onChange={(e) => onChange({ hint: e.target.value })}
          placeholder="Hint (optional helper text)"
          style={{ ...small, flex: 1 }}
        />
        <OptionsInput
          options={variable.options}
          onCommit={(options) => onChange({ options })}
          style={{ ...small, flex: 1, fontFamily: "var(--mono)" }}
        />
      </div>
    </div>
  );
}

// Comma-separated editor over `options: string[]`. Local raw-text state so
// a trailing comma survives while typing; the parsed array is committed on
// every change.
function OptionsInput({
  options,
  onCommit,
  style,
}: {
  options: string[];
  onCommit: (options: string[]) => void;
  style: React.CSSProperties;
}) {
  const [raw, setRaw] = useState(options.join(", "));
  return (
    <input
      value={raw}
      onChange={(e) => {
        setRaw(e.target.value);
        onCommit(
          e.target.value
            .split(",")
            .map((s) => s.trim())
            .filter(Boolean),
        );
      }}
      placeholder="Options, comma-separated (renders a dropdown)"
      style={style}
    />
  );
}

function FlagToggle({
  label,
  on,
  onChange,
}: {
  label: string;
  on: boolean;
  onChange: (next: boolean) => void;
}) {
  return (
    <button
      type="button"
      onClick={() => onChange(!on)}
      aria-pressed={on}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 4,
        background: "none",
        border: "none",
        cursor: "pointer",
        padding: 0,
        fontFamily: "var(--mono)",
        fontSize: 10,
        textTransform: "uppercase",
        letterSpacing: 0.5,
        color: on ? "var(--accent)" : "var(--text-dim)",
      }}
    >
      <Icon
        name={on ? "square-check" : "square"}
        size={11}
        stroke={on ? "var(--accent)" : "var(--text-dim)"}
      />
      {label}
    </button>
  );
}
