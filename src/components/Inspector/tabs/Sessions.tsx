import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useQuery } from "@tanstack/react-query";
import { Icon, type IconName } from "../../Icon";
import { TabEmpty, TabError, TabSkeleton } from "../TabStates";
import {
  listLaunchTemplates,
  listProviders,
  listSessions,
  providerNewInvocation,
  sessionResumeInfo,
  type ProviderInfo,
} from "../../../ipc";
import { useUiStore } from "../../../state/store";
import { spawnSessionPane } from "../../../features/terminal/TerminalStrip";
import { LaunchTemplateWizard } from "../../../features/launch-templates/LaunchTemplateWizard";
import type {
  LaunchTemplate,
  Project,
  Session,
  SessionStatus,
} from "../../../types";

// Inspector / Sessions tab. Lists CLI-agent sessions discovered across
// every enabled provider (Claude, Codex, OpenCode, …). The provider that
// owns each session is shown as a small badge and used to drive the
// per-provider "open in <provider>" actions.

interface SessionsProps {
  project: Project;
}

type ProviderFilter = "all" | string;

export function Sessions({ project }: SessionsProps) {
  const pushToast = useUiStore((s) => s.pushToast);
  const [filter, setFilter] = useState<ProviderFilter>("all");
  // Launch template picked from the split-button menu; non-null mounts the
  // variable-filling wizard.
  const [wizardTemplate, setWizardTemplate] = useState<LaunchTemplate | null>(
    null,
  );

  const { data: providers = [] } = useQuery<ProviderInfo[]>({
    queryKey: ["providers"],
    queryFn: listProviders,
    retry: false,
  });

  const enabledProviders = useMemo(
    () => providers.filter((p) => p.enabled),
    [providers],
  );

  const { data, isLoading, isFetching, error, refetch } = useQuery<Session[]>({
    queryKey: ["sessions", project.id],
    queryFn: () => listSessions(project.id),
    staleTime: 15_000,
    retry: false,
  });

  const allSessions = data ?? [];
  const sessions =
    filter === "all"
      ? allSessions
      : allSessions.filter((s) => s.provider === filter);

  // If the user picks a filter for a provider that loses its sessions,
  // snap back to All so they're not staring at an empty list.
  useEffect(() => {
    if (filter === "all") return;
    if (!enabledProviders.some((p) => p.id === filter)) {
      setFilter("all");
    }
  }, [filter, enabledProviders]);

  const providerLabel = (id: string) =>
    providers.find((p) => p.id === id)?.label ?? id;

  const openInProvider = async (session: Session) => {
    await resumeToast(session, providerLabel(session.provider), pushToast);
  };

  const openInTerminal = async (session: Session) => {
    try {
      const info = await sessionResumeInfo(session.id, session.provider);
      const id = await spawnSessionPane({
        sessionId: session.id,
        cwd: info.cwd || project.path,
        command: info.command,
        cmdArgs: info.args,
        title: session.title
          ? `${session.id.slice(0, 6)} · ${session.title}`
          : `session ${session.id.slice(0, 8)}`,
        branch: session.branch ?? project.branch ?? null,
        projectId: project.id,
        projectLabel: project.name,
      });
      if (!id) {
        await resumeToast(session, providerLabel(session.provider), pushToast);
      } else {
        pushToast(
          "success",
          `Resuming ${providerLabel(session.provider)} session ${session.id.slice(0, 8)} in terminal`,
        );
      }
    } catch (err) {
      pushToast("error", `Resume failed: ${String(err)}`);
    }
  };

  const startNewSession = async (providerId: string) => {
    const provider = providers.find((p) => p.id === providerId);
    if (!provider) {
      pushToast("error", `Unknown provider: ${providerId}`);
      return;
    }
    if (!provider.available) {
      pushToast(
        "warn",
        `${provider.label} binary (${provider.binaryName}) is not on PATH`,
      );
      return;
    }
    try {
      const inv = await providerNewInvocation(providerId, project.id);
      const id = await spawnSessionPane({
        sessionId: `new-${providerId}-${Date.now().toString(36)}`,
        cwd: inv.cwd || project.path,
        command: inv.command,
        cmdArgs: inv.args,
        title: `new ${provider.label} · ${project.name}`,
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
    } catch (err) {
      pushToast("error", `Could not start ${provider.label}: ${String(err)}`);
    }
  };

  // Default for the main click. When a specific provider pill is active,
  // the button starts that provider — matches the user's mental model
  // ("filter to OpenCode" → "+ new OpenCode"). When the filter is "all",
  // fall back to the user-configured default, then to first available.
  const defaultProvider = useMemo<ProviderInfo | null>(() => {
    if (filter !== "all") {
      const picked = enabledProviders.find((p) => p.id === filter);
      if (picked) return picked;
    }
    const def = providers.find((p) => p.isDefault);
    if (def && def.enabled && def.available) return def;
    return (
      enabledProviders.find((p) => p.available) ??
      enabledProviders[0] ??
      null
    );
  }, [filter, providers, enabledProviders]);

  return (
    <div className="flex flex-col h-full">
      <div className="px-[14px] pt-[14px] pb-[10px] flex items-center gap-2 shrink-0 flex-wrap">
        <ProviderPills
          providers={enabledProviders}
          value={filter}
          onChange={setFilter}
          counts={countByProvider(allSessions)}
        />
        <div className="flex-1" />
        <button
          type="button"
          onClick={() => refetch()}
          disabled={isFetching}
          title="Rescan sessions"
          aria-label="Rescan sessions"
          className="inline-flex items-center gap-[5px] px-[8px] py-[3px] font-mono text-[10px] text-text-dim border border-line rounded-[3px] hover:text-text disabled:opacity-50"
        >
          <Icon name="repeat" size={10} stroke="currentColor" />
          {isFetching ? "scanning…" : "rescan"}
        </button>
        <NewSessionSplitButton
          providers={enabledProviders}
          defaultProvider={defaultProvider}
          onStart={startNewSession}
          onPickTemplate={setWizardTemplate}
        />
      </div>

      <div className="flex-1 min-h-0 px-[14px] pb-[14px] overflow-y-auto">
        {isLoading && !data && <TabSkeleton rows={3} />}
        {error && (
          <TabError
            message={error instanceof Error ? error.message : String(error)}
            onRetry={() => void refetch()}
          />
        )}
        {!isLoading && !error && sessions.length === 0 && (
          <TabEmpty
            icon="term"
            title={
              filter === "all"
                ? "No sessions yet"
                : `No ${providerLabel(filter)} sessions yet`
            }
            hint={
              defaultProvider
                ? `Press + new ${defaultProvider.label.split(" ")[0]} to start a session`
                : "Enable a provider in Settings → AI providers"
            }
          />
        )}
        {sessions.map((s) => (
          <SessionCard
            key={`${s.provider}:${s.id}`}
            session={s}
            providerLabel={providerLabel(s.provider)}
            onOpenInProvider={() => void openInProvider(s)}
            onOpenInTerminal={() => void openInTerminal(s)}
          />
        ))}
      </div>

      {wizardTemplate && (
        <LaunchTemplateWizard
          template={wizardTemplate}
          project={project}
          onClose={() => setWizardTemplate(null)}
        />
      )}
    </div>
  );
}

function countByProvider(sessions: Session[]): Record<string, number> {
  const m: Record<string, number> = {};
  for (const s of sessions) {
    m[s.provider] = (m[s.provider] ?? 0) + 1;
  }
  return m;
}

// ---- pills + split button ------------------------------------------------

function ProviderPills({
  providers,
  value,
  onChange,
  counts,
}: {
  providers: ProviderInfo[];
  value: ProviderFilter;
  onChange: (v: ProviderFilter) => void;
  counts: Record<string, number>;
}) {
  // Collapse to a single "All" pill when only one provider is enabled —
  // there's nothing to filter between.
  if (providers.length <= 1) {
    return (
      <span className="font-mono text-[10px] text-text-dim uppercase tracking-[0.6px]">
        {providers[0]?.label ?? "Sessions"}
      </span>
    );
  }
  return (
    <div className="flex border border-line rounded-[4px] overflow-hidden">
      <PillButton
        active={value === "all"}
        onClick={() => onChange("all")}
        first
      >
        All
      </PillButton>
      {providers.map((p) => (
        <PillButton
          key={p.id}
          active={value === p.id}
          onClick={() => onChange(p.id)}
        >
          {p.label.split(" ")[0]}
          {counts[p.id] != null && (
            <span className="ml-[4px] text-text-dimmer">
              {counts[p.id]}
            </span>
          )}
        </PillButton>
      ))}
    </div>
  );
}

function PillButton({
  active,
  onClick,
  first,
  children,
}: {
  active: boolean;
  onClick: () => void;
  first?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="px-[9px] py-[3px] font-mono text-[10px] uppercase tracking-[0.5px]"
      style={{
        background: active ? "var(--surface-2)" : "transparent",
        color: active ? "var(--text)" : "var(--text-dim)",
        borderLeft: first ? undefined : "1px solid var(--line)",
      }}
    >
      {children}
    </button>
  );
}

function NewSessionSplitButton({
  providers,
  defaultProvider,
  onStart,
  onPickTemplate,
}: {
  providers: ProviderInfo[];
  defaultProvider: ProviderInfo | null;
  onStart: (id: string) => void;
  onPickTemplate: (template: LaunchTemplate) => void;
}) {
  const openSettings = useUiStore((s) => s.openSettings);
  const [open, setOpen] = useState(false);

  const { data: templates = [] } = useQuery<LaunchTemplate[]>({
    queryKey: ["launch-templates"],
    queryFn: listLaunchTemplates,
    retry: false,
  });
  const wrapRef = useRef<HTMLDivElement>(null);
  const chevronRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [menuPos, setMenuPos] = useState<{ top: number; right: number } | null>(
    null,
  );

  // Close on outside click. The menu lives in a portal, so check both
  // the trigger wrapper *and* the menu node.
  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      const t = e.target as Node;
      if (wrapRef.current?.contains(t)) return;
      if (menuRef.current?.contains(t)) return;
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    const id = window.setTimeout(() => {
      document.addEventListener("mousedown", onDoc);
      document.addEventListener("keydown", onKey);
    }, 0);
    return () => {
      window.clearTimeout(id);
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  // Anchor the portal menu under the chevron, right-aligned. Recalculated
  // on open + on scroll/resize so it stays glued while the user pans.
  useLayoutEffect(() => {
    if (!open) {
      setMenuPos(null);
      return;
    }
    const compute = () => {
      const btn = chevronRef.current ?? wrapRef.current;
      if (!btn) return;
      const r = btn.getBoundingClientRect();
      setMenuPos({
        top: r.bottom + 4,
        right: window.innerWidth - r.right,
      });
    };
    compute();
    window.addEventListener("scroll", compute, true);
    window.addEventListener("resize", compute);
    return () => {
      window.removeEventListener("scroll", compute, true);
      window.removeEventListener("resize", compute);
    };
  }, [open]);

  const disabled = providers.length === 0;
  // The menu always carries the template group (at minimum "Manage
  // templates…"), so the chevron is shown even with a single provider.
  const showChevron = true;

  return (
    <div ref={wrapRef} style={{ position: "relative" }}>
      <div className="flex">
        <button
          type="button"
          disabled={disabled || defaultProvider == null}
          onClick={() => {
            if (defaultProvider) onStart(defaultProvider.id);
          }}
          title={
            defaultProvider
              ? `Start a ${defaultProvider.label} session`
              : "No provider available"
          }
          className="inline-flex items-center gap-[5px] px-[8px] py-[3px] font-mono text-[10px] font-semibold disabled:opacity-50"
          style={{
            background: "var(--accent)",
            color: "var(--accent-fg)",
            border: "none",
            borderTopLeftRadius: 3,
            borderBottomLeftRadius: 3,
            borderTopRightRadius: showChevron ? 0 : 3,
            borderBottomRightRadius: showChevron ? 0 : 3,
          }}
        >
          <Icon name="plus" size={10} stroke="var(--accent-fg)" />
          new {defaultProvider ? defaultProvider.label.split(" ")[0] : "session"}
        </button>
        {showChevron && (
          <button
            ref={chevronRef}
            type="button"
            onClick={() => setOpen((v) => !v)}
            aria-label="Choose provider"
            aria-haspopup="menu"
            aria-expanded={open}
            className="inline-flex items-center justify-center font-mono text-[10px] font-semibold disabled:opacity-50"
            style={{
              background: "var(--accent)",
              color: "var(--accent-fg)",
              border: "none",
              borderLeft: "1px solid rgba(0,0,0,0.18)",
              borderTopRightRadius: 3,
              borderBottomRightRadius: 3,
              padding: "3px 6px",
            }}
          >
            <Icon name="chevron-d" size={10} stroke="var(--accent-fg)" />
          </button>
        )}
      </div>
      {open && menuPos &&
        createPortal(
          <div
            ref={menuRef}
            role="menu"
            aria-label="Choose provider"
            onClick={(e) => e.stopPropagation()}
            className="rounded-[5px] p-[4px]"
            style={{
              position: "fixed",
              top: menuPos.top,
              right: menuPos.right,
              minWidth: 220,
              background: "var(--surface)",
              border: "1px solid var(--line)",
              boxShadow: "0 12px 28px rgba(0,0,0,0.35)",
              zIndex: 1000,
            }}
          >
            {providers.map((p) => (
              <div
                key={p.id}
                role="menuitem"
                tabIndex={0}
                onClick={() => {
                  onStart(p.id);
                  setOpen(false);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    onStart(p.id);
                    setOpen(false);
                  }
                }}
                className="flex items-center gap-[8px] px-[8px] py-[6px] rounded-[3px] cursor-pointer text-[12px] hover:bg-[var(--row-active)]"
                style={{
                  color: p.available ? "var(--text)" : "var(--text-dim)",
                }}
              >
                <Icon
                  name="sparkle"
                  size={11}
                  stroke={p.available ? "var(--accent)" : "var(--text-dimmer)"}
                />
                <span className="flex-1">{p.label}</span>
                {!p.available && (
                  <span
                    className="font-mono text-[9px] uppercase"
                    style={{ color: "var(--warn, #d97757)" }}
                  >
                    not installed
                  </span>
                )}
                {p.isDefault && p.available && (
                  <span
                    className="font-mono text-[9px] uppercase"
                    style={{ color: "var(--accent)" }}
                  >
                    default
                  </span>
                )}
              </div>
            ))}

            {/* Launch templates - separator, then one row per template
                (skipped entirely when none exist), then the manage entry. */}
            <div
              className="h-px my-[3px]"
              style={{ background: "var(--line)" }}
            />
            {templates.length > 0 && (
              <>
                <div
                  className="px-[8px] pt-[4px] pb-[2px] font-mono text-[9px] uppercase tracking-[0.8px]"
                  style={{ color: "var(--text-dimmer)" }}
                >
                  From template
                </div>
                {templates.map((t) => (
                  <div
                    key={t.id}
                    role="menuitem"
                    tabIndex={0}
                    title={t.hint || undefined}
                    onClick={() => {
                      onPickTemplate(t);
                      setOpen(false);
                    }}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        onPickTemplate(t);
                        setOpen(false);
                      }
                    }}
                    className="flex items-center gap-[8px] px-[8px] py-[6px] rounded-[3px] cursor-pointer text-[12px] hover:bg-[var(--row-active)]"
                    style={{ color: "var(--text)" }}
                  >
                    <span
                      className="w-[8px] h-[8px] rounded-full shrink-0"
                      style={{ background: t.color || "var(--accent)" }}
                    />
                    <span className="flex-1 truncate">{t.label}</span>
                  </div>
                ))}
              </>
            )}
            <div
              role="menuitem"
              tabIndex={0}
              onClick={() => {
                openSettings("launch-templates");
                setOpen(false);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  openSettings("launch-templates");
                  setOpen(false);
                }
              }}
              className="flex items-center gap-[8px] px-[8px] py-[6px] rounded-[3px] cursor-pointer text-[12px] hover:bg-[var(--row-active)]"
              style={{ color: "var(--text-dim)" }}
            >
              <Icon name="gear" size={11} stroke="var(--text-dim)" />
              <span className="flex-1">Manage templates…</span>
            </div>
          </div>,
          document.body,
        )}
    </div>
  );
}

// ---- card ----------------------------------------------------------------

interface SessionCardProps {
  session: Session;
  providerLabel: string;
  onOpenInProvider: () => void;
  onOpenInTerminal: () => void;
}

function SessionCard({
  session,
  providerLabel,
  onOpenInProvider,
  onOpenInTerminal,
}: SessionCardProps) {
  const pushToast = useUiStore((s) => s.pushToast);
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  // Close the overflow menu on outside click.
  useEffect(() => {
    if (!menuOpen) return;
    const onDoc = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    const t = window.setTimeout(
      () => document.addEventListener("click", onDoc),
      0,
    );
    return () => {
      window.clearTimeout(t);
      document.removeEventListener("click", onDoc);
    };
  }, [menuOpen]);

  const statusColor = getStatusColor(session.status);
  const statusGlow =
    session.status === "active" ? "0 0 6px var(--accent)" : "none";
  const borderColor =
    session.status === "active"
      ? "oklch(0.78 0.17 145 / 0.3)"
      : "transparent";

  const copyId = async () => {
    try {
      await navigator.clipboard.writeText(session.id);
      pushToast("info", `Copied session ID: ${session.id.slice(0, 8)}…`);
    } catch {
      pushToast("error", "Clipboard unavailable");
    }
    setMenuOpen(false);
  };

  const archive = () => {
    pushToast("info", "Session archival is not available yet");
    setMenuOpen(false);
  };
  const del = () => {
    pushToast("info", "Deleting sessions is not available yet");
    setMenuOpen(false);
  };

  return (
    <div
      className="session-card group relative px-[12px] py-[10px] rounded-[5px] mb-[8px]"
      style={{
        background: "var(--surface-2)",
        border: `1px solid ${borderColor}`,
        transition: "background 120ms, border-color 120ms",
      }}
    >
      <div className="flex items-center gap-[6px] mb-[6px]">
        <span
          className="w-[6px] h-[6px] rounded-full shrink-0"
          style={{ background: statusColor, boxShadow: statusGlow }}
        />
        <ProviderBadge id={session.provider} />
        <span className="text-[12px] font-semibold text-text flex-1 truncate">
          {session.title || "Untitled session"}
        </span>
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            setMenuOpen((v) => !v);
          }}
          className="w-[22px] h-[22px] inline-flex items-center justify-center rounded-[3px]"
          style={{
            background: "transparent",
            border: "none",
            color: "var(--text-dim)",
          }}
          title="More actions"
          aria-label="More actions"
          aria-haspopup="menu"
          aria-expanded={menuOpen}
        >
          <Icon name="more" size={12} stroke="var(--text-dim)" />
        </button>
      </div>

      <div
        className="text-[11px] leading-snug text-text-dim mb-[8px]"
        style={{
          display: "-webkit-box",
          WebkitLineClamp: 2,
          WebkitBoxOrient: "vertical",
          overflow: "hidden",
        }}
      >
        {session.last || (
          <span className="text-text-dimmer italic">No messages yet</span>
        )}
      </div>

      <div className="font-mono text-[10px] text-text-dimmer flex flex-wrap items-center gap-[6px]">
        <span className="inline-flex items-center gap-[3px]">
          <Icon name="clock" size={10} stroke="var(--text-dimmer)" />
          {session.when ? formatRelative(session.when) : "—"}
        </span>
        <span>·</span>
        <span>{session.turns} turns</span>
        <span>·</span>
        <span>{session.duration || "—"}</span>
        {session.branch && (
          <>
            <span>·</span>
            <span className="inline-flex items-center gap-[3px]">
              <Icon
                name="branch"
                size={10}
                stroke="var(--text-dimmer)"
              />
              {session.branch}
            </span>
          </>
        )}
      </div>

      {/* hover-reveal actions */}
      <div
        className="flex gap-[4px] mt-[8px] opacity-0 group-hover:opacity-100 transition-opacity"
      >
        <button
          type="button"
          onClick={onOpenInProvider}
          className="inline-flex items-center gap-[5px] px-[8px] py-[3px] rounded-[3px] font-mono text-[10px] font-semibold"
          style={{
            background: "var(--accent)",
            color: "var(--accent-fg)",
            border: "none",
          }}
        >
          <Icon name="sparkle" size={10} stroke="var(--accent-fg)" />
          Open in {providerLabel.split(" ")[0]}
        </button>
        <button
          type="button"
          onClick={onOpenInTerminal}
          className="inline-flex items-center gap-[5px] px-[8px] py-[3px] rounded-[3px] font-mono text-[10px]"
          style={{
            background: "transparent",
            color: "var(--text-dim)",
            border: "1px solid var(--line)",
          }}
        >
          <Icon name="term" size={10} stroke="var(--text-dim)" />
          Open in terminal
        </button>
      </div>

      {menuOpen && (
        <div
          ref={menuRef}
          role="menu"
          aria-label="Session actions"
          onClick={(e) => e.stopPropagation()}
          className="absolute top-[30px] right-[8px] z-20 rounded-[5px] p-[4px] text-[12px]"
          style={{
            minWidth: 200,
            background: "var(--surface)",
            border: "1px solid var(--line)",
            boxShadow: "0 8px 20px rgba(0,0,0,0.3)",
          }}
        >
          <MenuRow
            icon="sparkle"
            label={`Open in ${providerLabel}`}
            onClick={() => {
              onOpenInProvider();
              setMenuOpen(false);
            }}
          />
          <MenuRow
            icon="term"
            label="Open in terminal tab"
            hint="↵"
            onClick={() => {
              onOpenInTerminal();
              setMenuOpen(false);
            }}
          />
          <MenuRow icon="copy" label="Copy session ID" onClick={copyId} />
          <div
            className="h-px my-[3px]"
            style={{ background: "var(--line)" }}
          />
          <MenuRow icon="arch" label="Archive" onClick={archive} />
          <MenuRow icon="trash" label="Delete" danger onClick={del} />
        </div>
      )}

      <style>{`
        .session-card:hover {
          background: var(--surface) !important;
        }
      `}</style>
    </div>
  );
}

function ProviderBadge({ id }: { id: string }) {
  return (
    <span
      className="font-mono text-[9px] uppercase tracking-[0.5px] px-[5px] py-[1px] rounded-[2px] shrink-0"
      style={{
        background: "var(--surface)",
        color: "var(--text-dim)",
        border: "1px solid var(--line)",
      }}
      title={`Session from ${id}`}
    >
      {id}
    </span>
  );
}

// ---- small primitives ----------------------------------------------------

function MenuRow({
  icon,
  label,
  hint,
  onClick,
  danger,
}: {
  icon: IconName;
  label: string;
  hint?: string;
  onClick: () => void;
  danger?: boolean;
}) {
  return (
    <div
      onClick={onClick}
      role="menuitem"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick();
        }
      }}
      className="flex items-center gap-[8px] px-[8px] py-[5px] rounded-[3px] cursor-pointer text-[12px] hover:bg-[var(--row-active)]"
      style={{ color: danger ? "var(--danger)" : "var(--text)" }}
    >
      <Icon
        name={icon}
        size={11}
        stroke={danger ? "var(--danger)" : "var(--text-dim)"}
      />
      <span className="flex-1">{label}</span>
      {hint && (
        <span
          className="font-mono text-[10px]"
          style={{ color: "var(--text-dimmer)" }}
        >
          {hint}
        </span>
      )}
    </div>
  );
}

// ---- helpers -------------------------------------------------------------

function getStatusColor(status: SessionStatus): string {
  switch (status) {
    case "active":
      return "var(--accent)";
    case "idle":
      return "var(--text-dim)";
    case "archived":
    default:
      return "var(--text-dimmer)";
  }
}

async function resumeToast(
  session: Session,
  providerLabel: string,
  pushToast: (
    kind: "info" | "success" | "warn" | "error",
    message: string,
  ) => void,
): Promise<void> {
  try {
    const info = await sessionResumeInfo(session.id, session.provider);
    const argv = [info.command, ...info.args].map((tok) =>
      tok.includes(" ") ? JSON.stringify(tok) : tok,
    );
    const cmd = argv.join(" ");
    pushToast(
      "info",
      `Open in ${providerLabel}: \`${cmd}\` in ${info.cwd}`,
    );
  } catch (err) {
    pushToast(
      "info",
      `Open in ${providerLabel}: ${err instanceof Error ? err.message : String(err)}`,
    );
  }
}

function formatRelative(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const diffMs = Date.now() - d.getTime();
  const mins = Math.floor(diffMs / 60_000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  return d.toISOString().slice(0, 10);
}
