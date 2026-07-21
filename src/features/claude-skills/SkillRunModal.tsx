// Atlas - free-form input modal that launches a Claude Code session
// invoking a discovered skill.

import { useEffect, useState } from "react";
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
import { buildSkillPrompt } from "./skill-prompt";
import type { ClaudeSkill, Project } from "../../types";

// Asks for optional free-form input, then starts a `claude` terminal
// session whose initial positional prompt invokes the skill with that
// input. Modal chrome + launch flow mirror LaunchTemplateWizard.

interface SkillRunModalProps {
  skill: ClaudeSkill;
  project: Project;
  onClose: () => void;
}

export function SkillRunModal({ skill, project, onClose }: SkillRunModalProps) {
  const pushToast = useUiStore((s) => s.pushToast);
  // Overlays/modes that stack above this modal: while any is open, Esc and
  // ⌘↵ belong to THEM (useGlobalShortcuts / their own handlers act on the
  // same bubbled event) - reacting here too would double-close and discard
  // the entered input. Same guard list as LaunchTemplateWizard.
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

  const [input, setInput] = useState("");
  const [launching, setLaunching] = useState(false);

  // Skill invocations are Claude Code prose prompts: only the `claude` CLI
  // takes a positional prompt argument, so the launch is pinned to the
  // Claude provider - mirrors LaunchTemplateWizard's launch exactly.
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
      const prompt = buildSkillPrompt(skill, input);
      const id = await spawnSessionPane({
        sessionId: `new-${providerId}-${Date.now().toString(36)}`,
        cwd: inv.cwd || project.path,
        command: inv.command,
        cmdArgs: prompt ? [...inv.args, prompt] : inv.args,
        title: skill.name,
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

  // Esc cancels, ⌘/Ctrl+Enter launches.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Ignore key auto-repeat: a held ⌘↵ would otherwise re-fire the
      // launch before the first spawn resolves.
      if (e.repeat) return;
      if (hasHigherOverlay) return;
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        void launch();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const scopeBadge =
    skill.scope === "plugin" ? (skill.plugin ?? "plugin") : skill.scope;

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
        aria-label={`Run ${skill.name} skill`}
        style={{
          width: 480,
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
              fontFamily: "var(--mono)",
              fontSize: 13,
              fontWeight: 600,
              flex: 1,
              minWidth: 0,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {skill.name}
          </span>
          <span
            style={{
              fontFamily: "var(--mono)",
              fontSize: 10,
              color: "var(--text-dim)",
              textTransform: "uppercase",
              letterSpacing: 0.5,
              flexShrink: 0,
            }}
          >
            {scopeBadge}
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
          {skill.description && (
            <div
              style={{
                fontSize: 11,
                lineHeight: 1.5,
                color: "var(--text-dim)",
                marginBottom: 12,
                overflowWrap: "break-word",
              }}
            >
              {skill.description}
            </div>
          )}
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
            Input
          </div>
          <textarea
            autoFocus
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder="What should this skill work on? (optional)"
            rows={4}
            style={{
              ...FIELD_STYLE,
              width: "100%",
              resize: "vertical",
              minHeight: 80,
              lineHeight: 1.5,
            }}
          />
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
            esc cancel · ⌘↵ launch
          </span>
          <div style={{ flex: 1 }} />
          <button onClick={onClose} style={GHOST_BTN_LOCAL}>
            Cancel
          </button>
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
        </div>
      </div>
    </div>,
    document.body,
  );
}

// Local style consts - copied from LaunchTemplateWizard so the modal
// matches the app's modal chrome without cross-importing style objects.
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
