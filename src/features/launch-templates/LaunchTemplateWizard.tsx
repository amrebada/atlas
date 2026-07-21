// Atlas - step-by-step launch wizard for a Claude Code session template.

import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useQuery } from "@tanstack/react-query";
import { Icon } from "../../components/Icon";
import {
  listProviders,
  providerNewInvocation,
  type ProviderInfo,
} from "../../ipc";
import { useUiStore } from "../../state/store";
import { spawnSessionPane } from "../terminal/TerminalStrip";
import { htmlToMarkdown } from "../notes/note-clipboard";
import {
  extractVarKeys,
  renderLaunchPrompt,
  syncVariables,
} from "./template-vars";
import type {
  LaunchTemplate,
  LaunchTemplateVar,
  Project,
} from "../../types";

// One step per template variable (text / textarea / select), then a Preview
// step showing the rendered markdown before launch. The final prompt is
// passed to the `claude` CLI as ONE positional argv element appended after
// its new-session args - launches are pinned to the Claude provider, since
// the other CLIs read a positional as a path, not a prompt (opencode's is
// the project directory). Modal chrome mirrors NewProjectModal.

interface LaunchTemplateWizardProps {
  template: LaunchTemplate;
  project: Project;
  onClose: () => void;
}

export function LaunchTemplateWizard({
  template,
  project,
  onClose,
}: LaunchTemplateWizardProps) {
  const pushToast = useUiStore((s) => s.pushToast);
  // Overlays/modes that stack above the wizard: while any is open, Esc and
  // ⌘↵ belong to THEM (useGlobalShortcuts / their own handlers act on the
  // same bubbled event) - reacting here too would double-close and discard
  // the entered variable values.
  const hasHigherOverlay = useUiStore(
    (s) =>
      s.paletteOpen ||
      s.settingsOpen != null ||
      s.todayOpen ||
      s.timelineOpen ||
      s.newProjectOpen != null ||
      s.openNote != null ||
      s.contextMenu != null ||
      s.multiSelect.active,
  );

  const { data: providers = [] } = useQuery<ProviderInfo[]>({
    queryKey: ["providers"],
    queryFn: listProviders,
    retry: false,
  });

  // Steps come from the *synced* variable list, so stale configs for keys
  // no longer present in the body never surface, and keys added since the
  // last save still get a (blank-config) step.
  const vars = useMemo(
    () =>
      syncVariables(
        template.variables,
        extractVarKeys(htmlToMarkdown(template.body)),
      ),
    [template],
  );

  // Step 0..vars.length-1 are variable steps; vars.length is the Preview.
  // Zero-variable templates land straight on the Preview.
  const [step, setStep] = useState(0);
  const [values, setValues] = useState<Record<string, string>>(() => {
    const init: Record<string, string> = {};
    for (const v of vars) init[v.key] = v.default || (v.options[0] ?? "");
    return init;
  });
  const [launching, setLaunching] = useState(false);

  const previewStep = vars.length;
  const totalSteps = vars.length + 1;
  const isPreview = step === previewStep;
  const current = isPreview ? null : vars[step];

  const rendered = useMemo(
    () => (isPreview ? renderLaunchPrompt(template.body, values) : ""),
    [isPreview, template, values],
  );

  const canAdvance =
    current == null ||
    !current.required ||
    (values[current.key] ?? "").trim() !== "";

  // Launch templates are Claude Code prompts: only the `claude` CLI takes
  // the rendered markdown as a positional prompt argument (codex/opencode
  // interpret positionals as paths - opencode's is the project directory),
  // so the launch is pinned to the Claude provider rather than resolving
  // the user's default like startNewSession does.
  const launch = async () => {
    if (launching) return;
    const provider = providers.find((p) => p.id === "claude") ?? null;
    if (provider && !provider.available) {
      pushToast(
        "warn",
        `${provider.label} binary (${provider.binaryName}) is not on PATH`,
      );
      return;
    }
    const providerId = "claude";
    setLaunching(true);
    try {
      const inv = await providerNewInvocation(providerId, project.id);
      const id = await spawnSessionPane({
        sessionId: `new-${providerId}-${Date.now().toString(36)}`,
        cwd: inv.cwd || project.path,
        command: inv.command,
        // An empty body must spawn the plain no-argument invocation, not
        // pass an empty-string positional prompt.
        cmdArgs: rendered ? [...inv.args, rendered] : inv.args,
        title: template.label,
        branch: project.branch ?? null,
        projectId: project.id,
        projectLabel: project.name,
      });
      if (!id) {
        pushToast(
          "warn",
          "Terminal backend unavailable — try again after restart",
        );
      }
      onClose();
    } catch (err) {
      pushToast(
        "error",
        `Could not start ${provider?.label ?? providerId}: ${String(err)}`,
      );
      setLaunching(false);
    }
  };

  const advance = () => {
    if (isPreview) {
      void launch();
      return;
    }
    if (canAdvance) setStep((s) => s + 1);
  };
  const back = () => setStep((s) => Math.max(0, s - 1));

  // Esc cancels, ⌘/Ctrl+Enter advances (and launches from the Preview).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Ignore key auto-repeat: a held ⌘↵ would otherwise machine-gun
      // through every step and launch straight past the preview.
      if (e.repeat) return;
      if (hasHigherOverlay) return;
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        advance();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const setValue = (key: string, value: string) =>
    setValues((prev) => ({ ...prev, [key]: value }));

  return createPortal(
    <div
      onClick={onClose}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 400,
        background: "rgba(0,0,0,0.45)",
        backdropFilter: "blur(4px)",
        WebkitBackdropFilter: "blur(4px)",
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        paddingTop: "10vh",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label={`Launch ${template.label}`}
        style={{
          width: 560,
          maxWidth: "92vw",
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
          }}
        >
          <span
            style={{
              width: 8,
              height: 8,
              borderRadius: "50%",
              background: template.color || "var(--accent)",
              flexShrink: 0,
            }}
          />
          <span style={{ fontSize: 13, fontWeight: 600, flex: 1 }}>
            {template.label}
          </span>
          <span
            style={{
              fontFamily: "var(--mono)",
              fontSize: 10,
              color: "var(--text-dim)",
              textTransform: "uppercase",
              letterSpacing: 0.5,
            }}
          >
            Step {step + 1} of {totalSteps}
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

        {/* Body */}
        <div style={{ padding: 18 }}>
          {current && (
            <VarStep
              key={current.key}
              variable={current}
              value={values[current.key] ?? ""}
              onChange={(v) => setValue(current.key, v)}
            />
          )}
          {isPreview && (
            <>
              <div
                style={{
                  fontSize: 11,
                  fontWeight: 600,
                  letterSpacing: 0.4,
                  textTransform: "uppercase",
                  color: "var(--text-dim)",
                  marginBottom: 8,
                }}
              >
                Preview
              </div>
              <div
                style={{
                  fontFamily: "var(--mono)",
                  fontSize: 12,
                  lineHeight: 1.55,
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-word",
                  maxHeight: 280,
                  overflowY: "auto",
                  padding: "10px 12px",
                  background: "var(--surface-2)",
                  border: "1px solid var(--line)",
                  borderRadius: 6,
                  color: "var(--text)",
                }}
              >
                {rendered || (
                  <span style={{ color: "var(--text-dimmer)" }}>
                    (empty prompt)
                  </span>
                )}
              </div>
              <div
                style={{
                  fontSize: 11,
                  color: "var(--text-dim)",
                  marginTop: 8,
                }}
              >
                Sent to{" "}
                <span style={{ fontFamily: "var(--mono)" }}>claude</span> as a
                single prompt argument in{" "}
                <span style={{ fontFamily: "var(--mono)" }}>
                  {project.name}
                </span>
                .
              </div>
            </>
          )}
        </div>

        {/* Footer */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "12px 16px",
            borderTop: "1px solid var(--line)",
          }}
        >
          <span
            style={{
              fontFamily: "var(--mono)",
              fontSize: 10,
              color: "var(--text-dimmer)",
            }}
          >
            esc cancel · ⌘↵ {isPreview ? "launch" : "next"}
          </span>
          <div style={{ flex: 1 }} />
          {step > 0 && (
            <button onClick={back} style={GHOST_BTN_LOCAL}>
              Back
            </button>
          )}
          {!isPreview && (
            <button
              onClick={advance}
              disabled={!canAdvance}
              style={{
                ...PRIMARY_BTN_LOCAL,
                opacity: canAdvance ? 1 : 0.5,
                cursor: canAdvance ? "pointer" : "default",
              }}
            >
              Next
            </button>
          )}
          {isPreview && (
            <button
              onClick={() => void launch()}
              disabled={launching}
              style={{
                ...PRIMARY_BTN_LOCAL,
                display: "inline-flex",
                alignItems: "center",
                gap: 6,
                opacity: launching ? 0.6 : 1,
                cursor: launching ? "default" : "pointer",
              }}
            >
              <Icon name="play" size={10} stroke="var(--accent-fg)" />
              {launching ? "Launching…" : "Launch"}
            </button>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}

// ---- one variable step ----------------------------------------------------

function VarStep({
  variable,
  value,
  onChange,
}: {
  variable: LaunchTemplateVar;
  value: string;
  onChange: (v: string) => void;
}) {
  const hasOptions = variable.options.length > 0;
  // Keep the select renderable even if the stored default isn't one of the
  // declared options.
  const options =
    hasOptions && value && !variable.options.includes(value)
      ? [value, ...variable.options]
      : variable.options;

  return (
    <div>
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: 8,
          marginBottom: 4,
        }}
      >
        <span style={{ fontSize: 13, fontWeight: 600 }}>
          {variable.label || variable.key}
        </span>
        <code
          style={{
            fontFamily: "var(--mono)",
            fontSize: 10,
            color: "var(--text-dimmer)",
          }}
        >
          {`{{${variable.key}}}`}
        </code>
        {variable.required && (
          <span
            style={{
              fontFamily: "var(--mono)",
              fontSize: 9,
              textTransform: "uppercase",
              letterSpacing: 0.5,
              color: "var(--warn, #d97757)",
            }}
          >
            required
          </span>
        )}
      </div>
      {variable.hint && (
        <div
          style={{
            fontSize: 11,
            color: "var(--text-dim)",
            marginBottom: 8,
          }}
        >
          {variable.hint}
        </div>
      )}
      <div style={{ marginTop: variable.hint ? 0 : 8 }}>
        {hasOptions ? (
          <select
            autoFocus
            value={value}
            onChange={(e) => onChange(e.target.value)}
            style={{ ...FIELD_STYLE, width: "100%" }}
          >
            {options.map((o) => (
              <option key={o} value={o}>
                {o}
              </option>
            ))}
          </select>
        ) : variable.multiline ? (
          <textarea
            autoFocus
            value={value}
            onChange={(e) => onChange(e.target.value)}
            placeholder={variable.default}
            rows={5}
            style={{
              ...FIELD_STYLE,
              width: "100%",
              resize: "vertical",
              minHeight: 96,
              lineHeight: 1.5,
            }}
          />
        ) : (
          <input
            autoFocus
            value={value}
            onChange={(e) => onChange(e.target.value)}
            placeholder={variable.default}
            style={{ ...FIELD_STYLE, width: "100%" }}
          />
        )}
      </div>
    </div>
  );
}

// Local style consts - mirror the SettingsPanel primitives so the wizard
// matches the app's modal chrome without importing the settings slice into
// the Inspector bundle path.
const FIELD_STYLE: React.CSSProperties = {
  padding: "7px 10px",
  fontSize: 13,
  background: "var(--bg)",
  border: "1px solid var(--line)",
  borderRadius: 5,
  color: "var(--text)",
  outline: "none",
  fontFamily: "var(--sans)",
};
const GHOST_BTN_LOCAL: React.CSSProperties = {
  padding: "6px 12px",
  fontSize: 12,
  height: 28,
  background: "transparent",
  border: "1px solid var(--line)",
  borderRadius: 5,
  color: "var(--text)",
  cursor: "pointer",
  fontFamily: "var(--sans)",
};
const PRIMARY_BTN_LOCAL: React.CSSProperties = {
  padding: "6px 14px",
  fontSize: 12,
  height: 28,
  background: "var(--accent)",
  color: "var(--accent-fg)",
  border: "none",
  borderRadius: 5,
  cursor: "pointer",
  fontWeight: 600,
  fontFamily: "var(--sans)",
};
