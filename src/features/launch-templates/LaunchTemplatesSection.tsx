// Atlas - Settings section for Claude Code launch templates.

import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Icon } from "../../components/Icon";
import { listLaunchTemplates, removeLaunchTemplate } from "../../ipc";
import { useUiStore } from "../../state/store";
import {
  GHOST_BTN,
  SectionHdr,
} from "../settings/SettingsPanel";
import { LaunchTemplateEditor } from "./LaunchTemplateEditor";
import type { LaunchTemplate } from "../../types";

// CRUD list over `launch_templates.json`. Rows open the full-overlay
// LaunchTemplateEditor; deletes go through a confirm dialog modeled on
// RemoveWatcherDialog. Query key is `['launch-templates']` - never the
// scaffold-templates `['templates']` key.

export function LaunchTemplatesSection() {
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();

  const { data: templates = [] } = useQuery<LaunchTemplate[]>({
    queryKey: ["launch-templates"],
    queryFn: listLaunchTemplates,
    retry: false,
  });

  // `editor` non-null mounts the overlay; inner `template: null` = new.
  const [editor, setEditor] = useState<{
    template: LaunchTemplate | null;
  } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<LaunchTemplate | null>(
    null,
  );

  const removeMut = useMutation({
    mutationFn: removeLaunchTemplate,
    onSuccess: (_data, id) => {
      queryClient.invalidateQueries({ queryKey: ["launch-templates"] });
      setPendingDelete(null);
      // A template deleted from inside its own editor takes the (still
      // mounted) editor down with it.
      setEditor((cur) => (cur?.template?.id === id ? null : cur));
    },
    onError: (err) => {
      pushToast("error", `Remove failed: ${String(err)}`);
      setPendingDelete(null);
    },
  });

  return (
    <div>
      <SectionHdr>Session Templates</SectionHdr>
      <div
        style={{
          fontSize: 11,
          color: "var(--text-dim)",
          marginBottom: 12,
        }}
      >
        Reusable prompts for new Claude Code sessions. Declare variables as{" "}
        <code
          style={{
            fontFamily: "var(--mono)",
            fontSize: 11,
            padding: "1px 5px",
            borderRadius: 3,
            background: "var(--surface-2)",
            color: "var(--text-dim)",
          }}
        >
          {"{{name}}"}
        </code>{" "}
        and fill them in a step-by-step wizard when launching.
      </div>

      {templates.length === 0 && (
        <div
          style={{
            fontSize: 11,
            fontFamily: "var(--mono)",
            color: "var(--text-dimmer)",
            padding: "10px 0",
          }}
        >
          No session templates yet.
        </div>
      )}

      {templates.map((t) => (
        <div
          key={t.id}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            padding: "10px 2px",
            borderBottom: "1px solid var(--line-soft)",
          }}
        >
          <div
            style={{
              width: 10,
              height: 10,
              borderRadius: "50%",
              background: t.color || "var(--accent)",
              flexShrink: 0,
            }}
          />
          <div style={{ flex: 1, minWidth: 0 }}>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                minWidth: 0,
              }}
            >
              <span
                style={{
                  fontSize: 13,
                  color: "var(--text)",
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                }}
              >
                {t.label}
              </span>
            </div>
            <div
              style={{
                fontSize: 11,
                fontFamily: "var(--mono)",
                color: "var(--text-dim)",
                marginTop: 2,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {t.hint || "—"}
            </div>
          </div>
          <span
            style={{
              fontSize: 11,
              fontFamily: "var(--mono)",
              color: "var(--text-dim)",
              flexShrink: 0,
            }}
          >
            {t.variables.length} variable{t.variables.length === 1 ? "" : "s"}
          </span>
          <span
            style={{
              fontSize: 11,
              fontFamily: "var(--mono)",
              color: "var(--text-dimmer)",
              flexShrink: 0,
            }}
            title={`Updated ${t.updatedAt}`}
          >
            {t.updatedAt.slice(0, 10)}
          </span>
          <button
            style={GHOST_BTN}
            onClick={() => setEditor({ template: t })}
          >
            Edit
          </button>
          <button
            style={{ ...GHOST_BTN, color: "var(--danger)" }}
            onClick={() => setPendingDelete(t)}
          >
            Delete
          </button>
        </div>
      ))}

      <button
        onClick={() => setEditor({ template: null })}
        style={{ ...GHOST_BTN, marginTop: 14 }}
      >
        <Icon name="plus" size={11} /> New template
      </button>

      {editor && (
        <LaunchTemplateEditor
          template={editor.template}
          onClose={() => setEditor(null)}
          // The editor STAYS mounted - the confirm renders above it (z 430
          // vs 420), so Cancel returns to the editor with edits intact.
          onDelete={(t) => setPendingDelete(t)}
          deleteConfirmOpen={pendingDelete != null}
        />
      )}

      {pendingDelete && (
        <DeleteLaunchTemplateDialog
          template={pendingDelete}
          pending={removeMut.isPending}
          zIndex={editor ? 430 : 410}
          onCancel={() => setPendingDelete(null)}
          onConfirm={() => removeMut.mutate(pendingDelete.id)}
        />
      )}
    </div>
  );
}

// Confirmation modal for deleting a launch template. Modeled on
// RemoveWatcherDialog - same backdrop and button layout. Renders at 410
// (above the settings panel at 400) from the list, and at 430 when opened
// from the z-420 editor so the editor survives underneath.
function DeleteLaunchTemplateDialog({
  template,
  pending,
  zIndex,
  onCancel,
  onConfirm,
}: {
  template: LaunchTemplate;
  pending: boolean;
  zIndex: number;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  // Topmost layer while mounted: Esc cancels just this dialog. Capture
  // phase + stopPropagation keeps the editor underneath (whose own capture
  // listener yields while the dialog is open) and useGlobalShortcuts from
  // also closing their overlays on the same press.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      if (!pending) onCancel();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [pending, onCancel]);

  return createPortal(
    <div
      onClick={pending ? undefined : onCancel}
      style={{
        position: "fixed",
        inset: 0,
        zIndex,
        background: "rgba(0,0,0,0.45)",
        backdropFilter: "blur(3px)",
        WebkitBackdropFilter: "blur(3px)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label="Delete session template"
        style={{
          width: 460,
          padding: 20,
          background: "var(--surface)",
          border: "1px solid var(--line)",
          borderRadius: 10,
          boxShadow: "0 20px 60px rgba(0,0,0,0.4)",
          color: "var(--text)",
        }}
      >
        <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>
          Delete this session template?
        </div>
        <div
          style={{
            fontSize: 12,
            color: "var(--text-dim)",
            marginBottom: 6,
          }}
        >
          <span style={{ color: "var(--text)" }}>{template.label}</span>
        </div>
        <div
          style={{
            fontSize: 12,
            color: "var(--text-dim)",
            marginBottom: 14,
          }}
        >
          Existing sessions started from it are not affected. This cannot be
          undone.
        </div>
        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
          <button
            type="button"
            onClick={onCancel}
            disabled={pending}
            style={GHOST_BTN}
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={pending}
            style={{
              ...GHOST_BTN,
              color: "var(--danger)",
              borderColor: "var(--danger)",
            }}
          >
            {pending ? "Deleting…" : "Delete template"}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
