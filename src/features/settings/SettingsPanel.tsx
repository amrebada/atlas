import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { check as checkForUpdate } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { Icon } from "../../components/Icon";
import atlasIconUrl from "../../assets/atlas-icon.png";
import {
  addWatcher,
  agentPairEnvelope,
  agentPairingInfo,
  detectEditors,
  discoverProjects,
  getSettings,
  listProviders,
  listTemplates,
  listWatchers,
  removeTemplate,
  removeWatcher,
  setSettings,
  upsertTemplate,
  type ProviderInfo,
} from "../../ipc";
import { QRCodeSVG } from "qrcode.react";
import { useUiStore } from "../../state/store";
import type { SettingsSection } from "../../state/store";
import type {
  AgentSettings,
  EditorEntry,
  Settings,
  Template,
  WatchRoot,
} from "../../types";

// Atlas - Settings panel.

const SECTIONS: Array<{
  id: SettingsSection;
  icon:
    | "gear"
    | "code"
    | "git"
    | "folder"
    | "plus"
    | "cmd"
    | "term"
    | "sparkle";
  label: string;
}> = [
  { id: "general", icon: "gear", label: "General" },
  { id: "editors", icon: "code", label: "Editors" },
  { id: "providers", icon: "sparkle", label: "AI providers" },
  { id: "git", icon: "git", label: "Git" },
  { id: "watchers", icon: "folder", label: "Folder watchers" },
  { id: "templates", icon: "plus", label: "Templates" },
  { id: "shortcuts", icon: "cmd", label: "Shortcuts" },
  { id: "advanced", icon: "term", label: "Advanced" },
  { id: "about", icon: "sparkle", label: "About" },
];

export function SettingsPanel() {
  const state = useUiStore((s) => s.settingsOpen);
  const close = useUiStore((s) => s.closeSettings);
  const [section, setSection] = useState<SettingsSection>(
    state?.section ?? "general",
  );

  // Sync section when opened from different entrypoints (⌘, vs. gear vs.
  useEffect(() => {
    if (state) setSection(state.section);
  }, [state]);

  // Load settings once per open. `retry: false` because D5's `settings_get`
  const { data: settings } = useQuery<Settings>({
    queryKey: ["settings"],
    queryFn: getSettings,
    enabled: state != null,
    retry: false,
  });

  if (!state) return null;

  return createPortal(
    <div
      onClick={close}
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
        paddingTop: "7vh",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        style={{
          width: 780,
          height: 560,
          position: "relative",
          background: "var(--surface)",
          border: "1px solid var(--line)",
          borderRadius: 10,
          overflow: "hidden",
          boxShadow: "0 30px 80px rgba(0,0,0,0.5)",
          display: "flex",
          fontFamily: "var(--sans)",
          color: "var(--text)",
        }}
      >
        {/* Left nav */}
        <div
          style={{
            width: 180,
            borderRight: "1px solid var(--line)",
            background: "var(--chrome)",
            padding: "12px 0",
            display: "flex",
            flexDirection: "column",
          }}
        >
          <div
            style={{
              padding: "8px 14px",
              fontSize: 12,
              fontWeight: 600,
            }}
          >
            Settings
          </div>
          {SECTIONS.map((s) => {
            const active = s.id === section;
            return (
              <button
                key={s.id}
                type="button"
                onClick={() => setSection(s.id)}
                aria-label={`${s.label} settings`}
                aria-pressed={active}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  height: 28,
                  padding: "0 14px",
                  cursor: "pointer",
                  fontSize: 12,
                  background: active ? "var(--row-active)" : "transparent",
                  color: active ? "var(--text)" : "var(--text-dim)",
                  borderLeft: active
                    ? "2px solid var(--accent)"
                    : "2px solid transparent",
                  border: "none",
                  textAlign: "left",
                  fontFamily: "inherit",
                }}
              >
                <Icon name={s.icon} size={12} />
                <span>{s.label}</span>
              </button>
            );
          })}
          <div style={{ flex: 1 }} />
        </div>

        {/* Right pane */}
        <div
          style={{
            flex: 1,
            padding: 22,
            overflowY: "auto",
          }}
        >
          {section === "general" && <GeneralSection settings={settings} />}
          {section === "editors" && <EditorsSection settings={settings} />}
          {section === "providers" && <ProvidersSection settings={settings} />}
          {section === "git" && <GitSection settings={settings} />}
          {section === "watchers" && <WatchersSection />}
          {section === "templates" && <TemplatesSection />}
          {section === "shortcuts" && (
            <ShortcutsSection settings={settings} />
          )}
          {section === "advanced" && <AdvancedSection settings={settings} />}
          {section === "about" && <AboutSection />}
        </div>

        <button
          onClick={close}
          aria-label="Close"
          style={{
            position: "absolute",
            top: 10,
            right: 12,
            background: "none",
            border: "none",
            color: "var(--text-dim)",
            cursor: "pointer",
            fontSize: 18,
            width: 26,
            height: 26,
          }}
        >
          ×
        </button>
      </div>
    </div>,
    document.body,
  );
}

// -----------------------------------------------------------------------------

function GeneralSection({ settings }: { settings?: Settings }) {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);
  // The theme is driven from two places: the persisted backend setting
  const setTheme = useUiStore((s) => s.setTheme);
  const setTerminalTheme = useUiStore((s) => s.setTerminalTheme);
  const general = settings?.general;

  const mutation = useMutation({
    mutationFn: (patch: Partial<Settings["general"]>) =>
      setSettings({ general: { ...(general ?? {}), ...patch } }),
    onSuccess: (_data, patch) => {
      queryClient.invalidateQueries({ queryKey: ["settings"] });
      if (patch.theme) setTheme(patch.theme);
      if (patch.terminalTheme) setTerminalTheme(patch.terminalTheme);
    },
    onError: (err) => pushToast("error", `Save failed: ${String(err)}`),
  });

  return (
    <div>
      <SectionHdr>General</SectionHdr>
      <SettingsRow
        label="Launch at login"
        hint="Atlas starts when you log in"
      >
        <Toggle
          on={general?.launchAtLogin ?? false}
          onChange={(v) => mutation.mutate({ launchAtLogin: v })}
        />
      </SettingsRow>
      <SettingsRow
        label="Menu bar agent"
        hint="Keep a status item with quick project switcher"
      >
        <Toggle
          on={general?.menuBarAgent ?? false}
          onChange={(v) => mutation.mutate({ menuBarAgent: v })}
        />
      </SettingsRow>
      <SettingsRow label="Default project location">
        <code style={CODE_STYLE}>
          {general?.defaultProjectLocation ?? "~/code"}
        </code>
      </SettingsRow>
      <SettingsRow label="Theme">
        <select
          value={general?.theme ?? "system"}
          onChange={(e) =>
            mutation.mutate({
              theme: e.target.value as Settings["general"]["theme"],
            })
          }
          style={SELECT_STYLE}
        >
          <option value="dark">dark</option>
          <option value="light">light</option>
          <option value="system">match system</option>
        </select>
      </SettingsRow>
      <SettingsRow
        label="Terminal theme"
        hint="Independent from the app theme — pick a different look for shells."
      >
        <select
          value={general?.terminalTheme ?? "system"}
          onChange={(e) =>
            mutation.mutate({
              terminalTheme: e.target
                .value as Settings["general"]["terminalTheme"],
            })
          }
          style={SELECT_STYLE}
        >
          <option value="dark">dark</option>
          <option value="light">light</option>
          <option value="system">match system</option>
        </select>
      </SettingsRow>
    </div>
  );
}

function EditorsSection({ settings }: { settings?: Settings }) {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);

  // Prefer the live detection if it's registered;
  const { data: live } = useQuery<EditorEntry[]>({
    queryKey: ["editors"],
    queryFn: detectEditors,
    retry: false,
  });

  const editors: EditorEntry[] =
    live && live.length > 0 ? live : (settings?.editors.detected ?? []);
  const defaultId = settings?.editors.defaultId ?? null;

  const makeDefault = useMutation({
    mutationFn: (id: string) =>
      setSettings({
        editors: {
          detected: editors,
          defaultId: id,
        },
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["settings"] }),
    onError: (err) => pushToast("error", `Save failed: ${String(err)}`),
  });

  return (
    <div>
      <SectionHdr>Editors</SectionHdr>
      <div
        style={{
          fontSize: 11,
          color: "var(--text-dim)",
          marginBottom: 10,
        }}
      >
        Detected on PATH · one is the default for <code style={CODE_STYLE}>Open</code>{" "}
        action.
      </div>
      {editors.length === 0 && (
        <div
          style={{
            fontSize: 11,
            fontFamily: "var(--mono)",
            color: "var(--text-dimmer)",
            padding: "10px 0",
          }}
        >
          No editors detected yet.
        </div>
      )}
      {editors.map((e) => {
        const isDefault = defaultId === e.id;
        return (
          <div
            key={e.id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 10,
              padding: "10px 0",
              borderBottom: "1px solid var(--line-soft)",
            }}
          >
            <Icon
              name={isDefault ? "dot" : "code"}
              size={14}
              stroke={
                isDefault ? "var(--accent)" : "var(--text-dim)"
              }
            />
            <span style={{ fontSize: 13, flexShrink: 0 }}>{e.name}</span>
            <code
              style={{
                ...CODE_STYLE,
                // Row is a flex container; let the path truncate with
                flex: 1,
                minWidth: 0,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
              title={e.cmd}
            >
              {e.cmd}
            </code>
            {isDefault ? (
              <span
                style={{
                  fontSize: 10,
                  fontFamily: "var(--mono)",
                  color: "var(--accent)",
                  textTransform: "uppercase",
                }}
              >
                default
              </span>
            ) : (
              <button
                style={GHOST_BTN}
                disabled={!e.present}
                onClick={() => makeDefault.mutate(e.id)}
              >
                Make default
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}

function ProvidersSection({ settings }: { settings?: Settings }) {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);

  const { data: providers = [] } = useQuery<ProviderInfo[]>({
    queryKey: ["providers"],
    queryFn: listProviders,
    retry: false,
    // refetch when settings change so the toggle/default badges stay in sync
    enabled: settings != null,
  });

  const persisted = settings?.providers;

  const mutate = useMutation({
    mutationFn: (patch: Partial<Settings["providers"]>) =>
      setSettings({
        providers: { ...(persisted ?? {}), ...patch },
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["settings"] });
      queryClient.invalidateQueries({ queryKey: ["providers"] });
    },
    onError: (err) => pushToast("error", `Save failed: ${String(err)}`),
  });

  const enabledMap = persisted?.enabled ?? {};
  const defaultId = persisted?.defaultId ?? "claude";

  const toggle = (id: string, on: boolean) =>
    mutate.mutate({
      enabled: { ...enabledMap, [id]: on },
    });

  const setDefault = (id: string) => {
    mutate.mutate({ defaultId: id });
  };

  return (
    <div>
      <SectionHdr>AI providers</SectionHdr>
      <div
        style={{
          fontSize: 11,
          color: "var(--text-dim)",
          marginBottom: 12,
        }}
      >
        Toggle which CLI agents Atlas surfaces in the Sessions tab. The
        default is used by the <code style={CODE_STYLE}>+ new session</code>{" "}
        button.
      </div>
      {providers.length === 0 && (
        <div
          style={{
            fontSize: 11,
            fontFamily: "var(--mono)",
            color: "var(--text-dimmer)",
            padding: "10px 0",
          }}
        >
          No providers registered yet.
        </div>
      )}
      {providers.map((p) => {
        const enabled = enabledMap[p.id] ?? true;
        const isDefault = defaultId === p.id;
        return (
          <div
            key={p.id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 10,
              padding: "10px 0",
              borderBottom: "1px solid var(--line-soft)",
            }}
          >
            <Icon
              name="sparkle"
              size={14}
              stroke={
                p.available ? "var(--accent)" : "var(--text-dimmer)"
              }
            />
            <div style={{ flex: 1, minWidth: 0 }}>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                }}
              >
                <span
                  style={{
                    fontSize: 13,
                    color: "var(--text)",
                  }}
                >
                  {p.label}
                </span>
                {!p.available && (
                  <span
                    style={{
                      fontSize: 9,
                      fontFamily: "var(--mono)",
                      color: "var(--warn, #d97757)",
                      textTransform: "uppercase",
                      letterSpacing: 0.5,
                      padding: "1px 5px",
                      border: "1px solid var(--warn, #d97757)",
                      borderRadius: 2,
                    }}
                  >
                    not installed
                  </span>
                )}
                {isDefault && enabled && (
                  <span
                    style={{
                      fontSize: 9,
                      fontFamily: "var(--mono)",
                      color: "var(--accent)",
                      textTransform: "uppercase",
                      letterSpacing: 0.5,
                    }}
                  >
                    default
                  </span>
                )}
              </div>
              <code
                style={{
                  ...CODE_STYLE,
                  display: "inline-block",
                  marginTop: 2,
                }}
                title={`Binary on PATH: ${p.binaryName}`}
              >
                {p.binaryName}
              </code>
            </div>
            {!isDefault && enabled && p.available && (
              <button
                style={GHOST_BTN}
                onClick={() => setDefault(p.id)}
              >
                Make default
              </button>
            )}
            <Toggle
              on={enabled}
              onChange={(v) => toggle(p.id, v)}
            />
          </div>
        );
      })}
    </div>
  );
}

function GitSection({ settings }: { settings?: Settings }) {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);
  const git = settings?.git;

  const mutation = useMutation({
    mutationFn: (patch: Partial<Settings["git"]>) =>
      setSettings({
        git: {
          pollInterval: git?.pollInterval ?? "30s",
          showAuthor: git?.showAuthor ?? false,
          defaultCloneDepth: git?.defaultCloneDepth ?? "full",
          sshKey: git?.sshKey ?? "",
          ...patch,
        },
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["settings"] }),
    onError: (err) => pushToast("error", `Save failed: ${String(err)}`),
  });

  return (
    <div>
      <SectionHdr>Git</SectionHdr>
      <SettingsRow
        label="Poll interval"
        hint="How often Atlas refreshes branch & dirty status"
      >
        <select
          value={git?.pollInterval ?? "30s"}
          onChange={(e) =>
            mutation.mutate({
              pollInterval: e.target
                .value as Settings["git"]["pollInterval"],
            })
          }
          style={SELECT_STYLE}
        >
          <option value="10s">10s</option>
          <option value="30s">30s</option>
          <option value="1m">1m</option>
          <option value="off">off</option>
        </select>
      </SettingsRow>
      <SettingsRow
        label="Show commit author in row"
        hint="Column appears in the Project list."
      >
        <Toggle
          on={git?.showAuthor ?? false}
          onChange={(v) => mutation.mutate({ showAuthor: v })}
        />
      </SettingsRow>
      <SettingsRow label="Default clone depth">
        <select
          value={
            git?.defaultCloneDepth === "full" || git?.defaultCloneDepth == null
              ? "full"
              : String(git.defaultCloneDepth)
          }
          onChange={(e) => {
            const v = e.target.value;
            mutation.mutate({
              defaultCloneDepth: v === "full" ? "full" : Number(v),
            });
          }}
          style={SELECT_STYLE}
        >
          <option value="full">full</option>
          <option value="1">depth 1 (shallow)</option>
          <option value="10">depth 10</option>
          <option value="50">depth 50</option>
        </select>
      </SettingsRow>
      <SettingsRow label="SSH key" hint="Used for clone operations">
        <DebouncedInput
          value={git?.sshKey ?? ""}
          placeholder="~/.ssh/id_ed25519"
          onCommit={(sshKey) => mutation.mutate({ sshKey })}
        />
      </SettingsRow>
    </div>
  );
}

function WatchersSection() {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);
  const [pendingRemove, setPendingRemove] = useState<WatchRoot | null>(null);
  const [resyncingPath, setResyncingPath] = useState<string | null>(null);

  const { data: watchers = [] } = useQuery<WatchRoot[]>({
    queryKey: ["watchRoots"],
    queryFn: listWatchers,
    retry: false,
  });

  const addMut = useMutation({
    mutationFn: (path: string) => addWatcher(path, 3),
    onSuccess: (_res, path) => {
      queryClient.invalidateQueries({ queryKey: ["watchRoots"] });
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      discoverProjects(path, 3).catch(() => {});
    },
    onError: (err) => pushToast("error", `Add failed: ${String(err)}`),
  });

  const removeMut = useMutation({
    mutationFn: ({ path, cascade }: { path: string; cascade: boolean }) =>
      removeWatcher(path, cascade).then((removed) => ({ path, removed })),
    onSuccess: ({ path, removed }, vars) => {
      queryClient.invalidateQueries({ queryKey: ["watchRoots"] });
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      const msg = vars.cascade
        ? `Stopped watching ${path} · removed ${removed} project${removed === 1 ? "" : "s"}`
        : `Stopped watching ${path}`;
      pushToast("info", msg);
      setPendingRemove(null);
    },
    onError: (err) => {
      pushToast("error", `Remove failed: ${String(err)}`);
      setPendingRemove(null);
    },
  });

  const resyncMut = useMutation({
    mutationFn: ({ path, depth }: { path: string; depth: number }) =>
      discoverProjects(path, depth).then((ids) => ({ path, ids })),
    onMutate: ({ path }) => setResyncingPath(path),
    onSettled: () => setResyncingPath(null),
    onSuccess: ({ path, ids }) => {
      queryClient.invalidateQueries({ queryKey: ["watchRoots"] });
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      if (ids.length === 0) {
        pushToast("info", `Resynced ${path} · no new projects`);
      } else {
        pushToast(
          "success",
          `Resynced ${path} · ${ids.length} new project${ids.length === 1 ? "" : "s"} discovered`,
        );
      }
    },
    onError: (err) => pushToast("error", `Resync failed: ${String(err)}`),
  });

  const onAdd = async () => {
    try {
      const picked = await openDialog({ directory: true, multiple: false });
      if (typeof picked === "string") addMut.mutate(picked);
    } catch (err) {
      pushToast("error", `Folder picker failed: ${String(err)}`);
    }
  };

  return (
    <div>
      <SectionHdr>Folder watchers</SectionHdr>
      <div
        style={{
          fontSize: 11,
          color: "var(--text-dim)",
          marginBottom: 10,
        }}
      >
        Atlas scans these folders and picks up new git repos automatically.
      </div>
      {watchers.length === 0 && (
        <div
          style={{
            fontSize: 11,
            fontFamily: "var(--mono)",
            color: "var(--text-dimmer)",
            padding: "10px 0",
          }}
        >
          No watchers configured.
        </div>
      )}
      {watchers.map((w) => {
        const isResyncing = resyncingPath === w.path;
        return (
          <div
            key={w.path}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 10,
              padding: "10px 0",
              borderBottom: "1px solid var(--line-soft)",
            }}
          >
            <Icon name="folder" size={14} stroke="var(--text-dim)" />
            <code style={{ ...CODE_STYLE, flex: 1 }}>{w.path}</code>
            <span
              style={{
                fontSize: 11,
                fontFamily: "var(--mono)",
                color: "var(--text-dim)",
              }}
            >
              depth {w.depth}
            </span>
            <span
              style={{
                fontSize: 11,
                fontFamily: "var(--mono)",
                color: "var(--text-dim)",
              }}
            >
              {w.repoCount} repos
            </span>
            <button
              style={GHOST_BTN}
              disabled={isResyncing}
              onClick={() => resyncMut.mutate({ path: w.path, depth: w.depth })}
              title="Re-scan this folder for new git repos"
            >
              {isResyncing ? "Resyncing…" : "Resync"}
            </button>
            <button
              style={GHOST_BTN}
              disabled={removeMut.isPending}
              onClick={() => setPendingRemove(w)}
            >
              Remove
            </button>
          </div>
        );
      })}
      <button
        onClick={onAdd}
        disabled={addMut.isPending}
        style={{ ...GHOST_BTN, marginTop: 12 }}
      >
        <Icon name="plus" size={11} />
        {addMut.isPending ? "Adding…" : "Add watcher…"}
      </button>
      {pendingRemove && (
        <RemoveWatcherDialog
          watcher={pendingRemove}
          pending={removeMut.isPending}
          onCancel={() => setPendingRemove(null)}
          onConfirm={(cascade) =>
            removeMut.mutate({ path: pendingRemove.path, cascade })
          }
        />
      )}
    </div>
  );
}

// Confirmation modal for removing a watcher. Two paths: keep the indexed
// projects (default) or unindex them along with the watcher. Filesystem is
// never touched either way.
function RemoveWatcherDialog({
  watcher,
  pending,
  onCancel,
  onConfirm,
}: {
  watcher: WatchRoot;
  pending: boolean;
  onCancel: () => void;
  onConfirm: (cascade: boolean) => void;
}) {
  const repoLabel = `${watcher.repoCount} project${
    watcher.repoCount === 1 ? "" : "s"
  }`;
  return createPortal(
    <div
      onClick={pending ? undefined : onCancel}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 410,
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
        aria-label="Remove watcher"
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
          Stop watching this folder?
        </div>
        <div
          style={{
            fontSize: 12,
            color: "var(--text-dim)",
            marginBottom: 6,
            fontFamily: "var(--mono)",
            wordBreak: "break-all",
          }}
        >
          <span style={{ color: "var(--text)" }}>{watcher.path}</span>
        </div>
        <div
          style={{
            fontSize: 12,
            color: "var(--text-dim)",
            marginBottom: 14,
          }}
        >
          {watcher.repoCount > 0
            ? `Atlas has ${repoLabel} indexed under this folder. Files on disk are never touched — only the index entries are removed.`
            : "Files on disk are never touched."}
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
            onClick={() => onConfirm(false)}
            disabled={pending}
            style={GHOST_BTN}
          >
            Keep projects
          </button>
          <button
            type="button"
            onClick={() => onConfirm(true)}
            disabled={pending || watcher.repoCount === 0}
            style={{
              ...GHOST_BTN,
              color: "var(--danger)",
              borderColor: "var(--danger)",
              opacity: watcher.repoCount === 0 ? 0.5 : 1,
            }}
          >
            {pending ? "Removing…" : `Remove ${repoLabel}`}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

function TemplatesSection() {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);

  const { data: templates = [] } = useQuery<Template[]>({
    queryKey: ["templates"],
    queryFn: listTemplates,
    retry: false,
  });

  const [adding, setAdding] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Template>({
    id: "",
    label: "",
    color: "#7c7fee",
    hint: "",
    path: "",
    builtin: false,
  });

  const swatches = [
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

  const upsertMut = useMutation({
    mutationFn: upsertTemplate,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["templates"] }),
    onError: (err) => pushToast("error", `Save failed: ${String(err)}`),
  });
  const removeMut = useMutation({
    mutationFn: removeTemplate,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["templates"] }),
    onError: (err) => pushToast("error", `Remove failed: ${String(err)}`),
  });

  const beginAdd = () => {
    setDraft({
      id: "",
      label: "",
      color: "#7c7fee",
      hint: "",
      path: "",
      builtin: false,
    });
    setAdding(true);
    setEditingId(null);
  };
  const beginEdit = (t: Template) => {
    setDraft({ ...t });
    setEditingId(t.id);
    setAdding(false);
  };
  const cancel = () => {
    setAdding(false);
    setEditingId(null);
  };
  const save = () => {
    const label = draft.label.trim();
    const path = draft.path.trim();
    if (!label || !path) {
      cancel();
      return;
    }
    const id =
      editingId ??
      label.toLowerCase().replace(/[^a-z0-9]+/g, "-").slice(0, 40) +
        "-" +
        Date.now().toString(36).slice(-3);
    upsertMut.mutate({
      ...draft,
      id,
      label,
      path,
      hint: draft.hint || path,
      builtin: false,
    });
    cancel();
  };

  const browseFolder = async () => {
    try {
      const picked = await openDialog({ directory: true, multiple: false });
      if (typeof picked === "string") setDraft((d) => ({ ...d, path: picked }));
    } catch (err) {
      pushToast("error", `Folder picker failed: ${String(err)}`);
    }
  };

  const EditorForm = (
    <div
      style={{
        padding: 12,
        marginBottom: 10,
        borderRadius: 6,
        background: "var(--surface-2)",
        border: "1px solid var(--accent)",
      }}
    >
      <div style={{ display: "flex", gap: 8, marginBottom: 8 }}>
        <input
          autoFocus
          value={draft.label}
          onChange={(e) => setDraft({ ...draft, label: e.target.value })}
          placeholder="Template name"
          style={{ ...INPUT_STYLE, flex: 1 }}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
            if (e.key === "Escape") cancel();
          }}
        />
        <div style={{ display: "flex", gap: 3, alignItems: "center" }}>
          {swatches.map((c) => (
            <button
              key={c}
              onClick={() => setDraft({ ...draft, color: c })}
              title={c}
              style={{
                width: 18,
                height: 18,
                borderRadius: "50%",
                background: c,
                border:
                  "2px solid " +
                  (draft.color === c ? "var(--text)" : "transparent"),
                cursor: "pointer",
                padding: 0,
              }}
            />
          ))}
        </div>
      </div>
      <div style={{ display: "flex", gap: 6, marginBottom: 8 }}>
        <input
          value={draft.path}
          onChange={(e) => setDraft({ ...draft, path: e.target.value })}
          placeholder="Folder path (e.g. ~/code/templates/my-template)"
          style={{
            ...INPUT_STYLE,
            flex: 1,
            fontFamily: "var(--mono)",
            fontSize: 12,
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
            if (e.key === "Escape") cancel();
          }}
        />
        <button onClick={browseFolder} style={GHOST_BTN}>
          Browse…
        </button>
      </div>
      <input
        value={draft.hint}
        onChange={(e) => setDraft({ ...draft, hint: e.target.value })}
        placeholder="Short description (optional — shown in the New Project picker)"
        style={{ ...INPUT_STYLE, width: "100%", marginBottom: 10 }}
        onKeyDown={(e) => {
          if (e.key === "Enter") save();
          if (e.key === "Escape") cancel();
        }}
      />
      <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
        <button onClick={cancel} style={GHOST_BTN}>
          Cancel
        </button>
        <button onClick={save} style={PRIMARY_BTN}>
          {editingId ? "Save" : "Add template"}
        </button>
      </div>
    </div>
  );

  return (
    <div>
      <SectionHdr>Templates</SectionHdr>
      <div
        style={{
          fontSize: 11,
          color: "var(--text-dim)",
          marginBottom: 12,
        }}
      >
        Point Atlas at folders you want as starting points. They appear in
        the New Project picker and are copied into the project's location.
      </div>

      {adding && EditorForm}

      {templates.map((t) => {
        if (editingId === t.id) return <div key={t.id}>{EditorForm}</div>;
        return (
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
                background: t.color,
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
                {t.builtin && (
                  <span
                    style={{
                      fontSize: 9,
                      fontFamily: "var(--mono)",
                      color: "var(--text-dimmer)",
                      textTransform: "uppercase",
                      letterSpacing: 0.5,
                      padding: "1px 5px",
                      border: "1px solid var(--line)",
                      borderRadius: 2,
                      flexShrink: 0,
                    }}
                  >
                    built-in
                  </span>
                )}
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
                {t.path || t.hint}
              </div>
            </div>
            {!t.builtin ? (
              <>
                <button style={GHOST_BTN} onClick={() => beginEdit(t)}>
                  Edit
                </button>
                <button
                  style={{ ...GHOST_BTN, color: "var(--danger)" }}
                  onClick={() => removeMut.mutate(t.id)}
                >
                  Remove
                </button>
              </>
            ) : (
              <span
                style={{
                  fontSize: 10,
                  fontFamily: "var(--mono)",
                  color: "var(--text-dimmer)",
                }}
              >
                read-only
              </span>
            )}
          </div>
        );
      })}

      {!adding && (
        <button onClick={beginAdd} style={{ ...GHOST_BTN, marginTop: 14 }}>
          <Icon name="plus" size={11} /> Add template folder…
        </button>
      )}
    </div>
  );
}

function ShortcutsSection({ settings }: { settings?: Settings }) {
  const rows = useMemo<Array<[string, string[]]>>(() => {
    // Display canonical set from prototype; merge any user overrides from
    const defaults: Array<[string, string[]]> = [
      ["Open command palette", ["⌘", "K"]],
      ["New project", ["⌘", "N"]],
      ["Clone from git", ["⌘", "⇧", "N"]],
      ["Open settings", ["⌘", ","]],
      ["Open selected in editor", ["⌘", "E"]],
      ["Toggle terminal", ["⌃", "`"]],
      ["Toggle Today Plan", ["⌘", "T"]],
      ["Focus search", ["/"]],
      ["Toggle pin", ["P"]],
      ["Archive", ["⌘", "⇧", "A"]],
    ];
    const user = settings?.shortcuts ?? {};
    return defaults.map(([label, keys]) => {
      const override = user[label];
      return [label, override ? override.split("+") : keys];
    });
  }, [settings]);

  return (
    <div>
      <SectionHdr>Shortcuts</SectionHdr>
      {rows.map(([label, keys]) => (
        <div
          key={label}
          style={{
            display: "flex",
            alignItems: "center",
            padding: "9px 0",
            borderBottom: "1px solid var(--line-soft)",
            fontSize: 13,
          }}
        >
          <span style={{ flex: 1 }}>{label}</span>
          <span style={{ display: "flex", gap: 3 }}>
            {keys.map((k, j) => (
              <KbdInline key={j}>{k}</KbdInline>
            ))}
          </span>
        </div>
      ))}
    </div>
  );
}

function PilotCard({
  pushToast,
}: {
  pushToast: (kind: "success" | "error", message: string) => void;
}) {
  const [busy, setBusy] = useState(false);

  const openWindow = async () => {
    try {
      await invoke("pilot_open_window");
    } catch (e) {
      pushToast("error", `Could not open Atlas Pilot: ${String(e)}`);
    }
  };

  const installSkill = async () => {
    setBusy(true);
    try {
      const dir = await invoke<string>("pilot_install_skill");
      pushToast("success", `atlas skill installed to ${dir}`);
    } catch (e) {
      pushToast("error", `Install failed: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div
        style={{
          marginTop: 22,
          marginBottom: 8,
          fontSize: 12,
          fontWeight: 600,
          letterSpacing: 0.4,
          textTransform: "uppercase",
          color: "var(--text-dim)",
        }}
      >
        Atlas Pilot
      </div>
      <SettingsRow
        label="Pilot window"
        hint="Automated project lifecycle — plan, then build epic by epic"
      >
        <button onClick={openWindow} style={GHOST_BTN}>
          Open Atlas Pilot
        </button>
      </SettingsRow>
      <SettingsRow
        label="atlas skill"
        hint="Installs the atlas skill into ~/.claude/skills/ — required for pilot sessions"
      >
        <button onClick={installSkill} disabled={busy} style={GHOST_BTN}>
          {busy ? "Installing…" : "Install atlas skill"}
        </button>
      </SettingsRow>
    </>
  );
}

function AdvancedSection({ settings }: { settings?: Settings }) {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);
  const adv = settings?.advanced;
  const mcp = adv?.mcp ?? { enabled: false, port: 8765, token: "" };
  const agent = adv?.agent ?? {
    enabled: false,
    relayUrl: "ws://localhost:9000/agent",
    token: "",
  };

  const mutation = useMutation({
    mutationFn: (patch: Partial<Settings["advanced"]>) =>
      setSettings({
        advanced: {
          useSpotlight: adv?.useSpotlight ?? false,
          crashReports: adv?.crashReports ?? false,
          shell: adv?.shell ?? "/bin/zsh",
          crashLog: adv?.crashLog ?? false,
          mcp,
          agent,
          ...patch,
        },
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["settings"] }),
    onError: (err) => pushToast("error", `Save failed: ${String(err)}`),
  });

  // mcp.token is generated on first enable so the user can immediately copy
  // it into a Claude Code / Codex MCP config. Restart Atlas to apply.
  const setMcp = (patch: Partial<typeof mcp>) =>
    mutation.mutate({ mcp: { ...mcp, ...patch } });

  const enableMcp = (on: boolean) => {
    if (on && !mcp.token) {
      const token =
        typeof crypto !== "undefined" && crypto.randomUUID
          ? crypto.randomUUID().replace(/-/g, "")
          : Math.random().toString(36).slice(2) + Date.now().toString(36);
      setMcp({ enabled: true, token });
      pushToast(
        "success",
        "MCP server enabled — restart Atlas to apply. Token generated.",
      );
    } else {
      setMcp({ enabled: on });
      if (on) {
        pushToast("success", "Restart Atlas to apply MCP server changes.");
      }
    }
  };

  const regenerateToken = () => {
    const token =
      typeof crypto !== "undefined" && crypto.randomUUID
        ? crypto.randomUUID().replace(/-/g, "")
        : Math.random().toString(36).slice(2) + Date.now().toString(36);
    setMcp({ token });
    pushToast("success", "New MCP token generated — restart Atlas to apply.");
  };

  const copyToken = async () => {
    if (!mcp.token) return;
    try {
      await navigator.clipboard.writeText(mcp.token);
      pushToast("success", "Token copied");
    } catch (err) {
      pushToast("error", `Copy failed: ${String(err)}`);
    }
  };

  const resetAll = () => {
    // Best-effort: we don't know the full shape, so patch with an empty
    setSettings({})
      .then(() => queryClient.invalidateQueries({ queryKey: ["settings"] }))
      .then(() => pushToast("success", "Settings reset"))
      .catch((err) => pushToast("error", `Reset failed: ${String(err)}`));
  };

  return (
    <div>
      <SectionHdr>Advanced</SectionHdr>
      <SettingsRow
        label="Use native Spotlight indexer"
        hint="Faster search but adds to Finder index"
      >
        <Toggle
          on={adv?.useSpotlight ?? false}
          onChange={(v) => mutation.mutate({ useSpotlight: v })}
        />
      </SettingsRow>
      <SettingsRow label="Allow anonymous crash reports">
        <Toggle
          on={adv?.crashReports ?? false}
          onChange={(v) => mutation.mutate({ crashReports: v })}
        />
      </SettingsRow>
      <SettingsRow label="Terminal shell" hint="Absolute path to the binary">
        <DebouncedInput
          value={adv?.shell ?? ""}
          placeholder="/bin/zsh"
          onCommit={(shell) => mutation.mutate({ shell })}
        />
      </SettingsRow>

      <div
        style={{
          marginTop: 22,
          marginBottom: 8,
          fontSize: 12,
          fontWeight: 600,
          letterSpacing: 0.4,
          textTransform: "uppercase",
          color: "var(--text-dim)",
        }}
      >
        Remote control (MCP)
      </div>
      <SettingsRow
        label="Embedded MCP server"
        hint="Loopback-only HTTP endpoint for local AI CLIs. Restart required."
      >
        <Toggle on={mcp.enabled} onChange={enableMcp} />
      </SettingsRow>
      {mcp.enabled && (
        <>
          <SettingsRow label="Port" hint="127.0.0.1 only">
            <DebouncedInput
              value={String(mcp.port || 8765)}
              placeholder="8765"
              onCommit={(v) => {
                const n = parseInt(v, 10);
                if (Number.isFinite(n) && n > 0 && n < 65536) {
                  setMcp({ port: n });
                }
              }}
            />
          </SettingsRow>
          <SettingsRow
            label="Bearer token"
            hint="Paste this into your AI CLI's MCP config"
          >
            <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
              <code
                style={{
                  fontSize: 11,
                  fontFamily: "var(--mono)",
                  background: "var(--surface-2)",
                  border: "1px solid var(--line)",
                  borderRadius: 5,
                  padding: "5px 8px",
                  maxWidth: 260,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
                title={mcp.token}
              >
                {mcp.token || "(none)"}
              </code>
              <button onClick={copyToken} style={GHOST_BTN}>
                Copy
              </button>
              <button onClick={regenerateToken} style={GHOST_BTN}>
                Regenerate
              </button>
            </div>
          </SettingsRow>
        </>
      )}

      <AgentSection
        agent={agent}
        onChange={(patch) =>
          mutation.mutate({ agent: { ...agent, ...patch } })
        }
        pushToast={pushToast}
      />

      <AgentPairingCard />

      <PilotCard pushToast={pushToast} />

      <div style={{ marginTop: 22 }}>
        <button
          onClick={resetAll}
          style={{
            ...GHOST_BTN,
            color: "var(--danger)",
            borderColor: "var(--danger)",
          }}
        >
          Reset all settings…
        </button>
      </div>
    </div>
  );
}

function AgentSection({
  agent,
  onChange,
  pushToast,
}: {
  agent: AgentSettings;
  onChange: (patch: Partial<AgentSettings>) => void;
  pushToast: (kind: "success" | "error", message: string) => void;
}) {
  const setEnabled = (on: boolean) => {
    onChange({ enabled: on });
    if (on) {
      pushToast("success", "Restart Atlas to apply Atlas Agent changes.");
    }
  };

  const copyToken = async () => {
    if (!agent.token) return;
    try {
      await navigator.clipboard.writeText(agent.token);
      pushToast("success", "Relay token copied");
    } catch (err) {
      pushToast("error", `Copy failed: ${String(err)}`);
    }
  };

  return (
    <>
      <div
        style={{
          marginTop: 22,
          marginBottom: 8,
          fontSize: 12,
          fontWeight: 600,
          letterSpacing: 0.4,
          textTransform: "uppercase",
          color: "var(--text-dim)",
        }}
      >
        Atlas Agent (relay connection)
      </div>
      <SettingsRow
        label="Connect to relay"
        hint="Outbound WebSocket to the relay backend. Restart required."
      >
        <Toggle on={agent.enabled} onChange={setEnabled} />
      </SettingsRow>
      {agent.enabled && (
        <>
          <SettingsRow
            label="Relay URL"
            hint="WebSocket endpoint. Use the local stub for dev."
          >
            <DebouncedInput
              value={agent.relayUrl}
              placeholder="ws://localhost:9000/agent"
              onCommit={(relayUrl) => onChange({ relayUrl })}
            />
          </SettingsRow>
          <SettingsRow
            label="Bearer token"
            hint="Sent in Authorization on the WS upgrade. Separate from device signing key."
          >
            <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
              <DebouncedInput
                value={agent.token}
                placeholder="(none)"
                onCommit={(token) => onChange({ token })}
              />
              <button onClick={copyToken} style={GHOST_BTN}>
                Copy
              </button>
            </div>
          </SettingsRow>
        </>
      )}
    </>
  );
}

function AgentPairingCard() {
  const pushToast = useUiStore((s) => s.pushToast);
  const [showQr, setShowQr] = useState(false);
  const { data: pairing } = useQuery({
    queryKey: ["agent", "pairing"],
    queryFn: agentPairingInfo,
    staleTime: Infinity,
    retry: false,
  });

  // Pre-fetch the envelope and keep it fresh — `clipboard.writeText`
  // must be called synchronously inside the click handler to keep
  // user-activation alive (any preceding `await` revokes the implicit
  // permission and the writeText rejects). 30s refetchInterval keeps
  // the envelope inside the relay's 60s freshness window without
  // making the IPC hot.
  const { data: pairEnv } = useQuery({
    queryKey: ["agent", "pair-envelope"],
    queryFn: agentPairEnvelope,
    staleTime: 0,
    refetchInterval: 30_000,
    refetchOnWindowFocus: false,
    retry: false,
  });

  if (!pairing) return null;

  // Mobile sends `envelopeJson` verbatim to `${relayBaseUrl}/pair`.
  // Re-encoding the envelope would break canonical-JSON byte equality
  // and the signature wouldn't verify.
  const qrPayload = pairEnv
    ? JSON.stringify({
        relayBaseUrl: pairEnv.relayBaseUrl,
        envelopeJson: pairEnv.envelopeJson,
      })
    : "";

  const copyJson = () => {
    if (!pairEnv) {
      pushToast("error", "Pair envelope not ready — try again in a second");
      return;
    }
    // Synchronous launch keeps user-activation; await the result purely
    // for the toast feedback.
    navigator.clipboard
      .writeText(qrPayload)
      .then(() => pushToast("success", "Pairing JSON copied"))
      .catch((err) => pushToast("error", `Copy failed: ${String(err)}`));
  };

  return (
    <>
      <div
        style={{
          marginTop: 22,
          marginBottom: 8,
          fontSize: 12,
          fontWeight: 600,
          letterSpacing: 0.4,
          textTransform: "uppercase",
          color: "var(--text-dim)",
        }}
      >
        Atlas Agent (mobile pairing)
      </div>
      <SettingsRow
        label="Device ID"
        hint="Stable across restarts. First 8 bytes of the agent's public key."
      >
        <code
          style={{
            fontSize: 12,
            fontFamily: "var(--mono)",
            background: "var(--surface-2)",
            border: "1px solid var(--line)",
            borderRadius: 5,
            padding: "5px 8px",
          }}
        >
          {pairing.deviceId}
        </code>
      </SettingsRow>
      <SettingsRow
        label="Pair a device"
        hint="Signed pair envelope refreshes every 30s while open. Don't share publicly."
      >
        <div style={{ display: "flex", gap: 6 }}>
          <button onClick={() => setShowQr(true)} style={GHOST_BTN}>
            Show QR
          </button>
          <button
            onClick={copyJson}
            style={GHOST_BTN}
            disabled={!pairEnv}
          >
            Copy JSON
          </button>
        </div>
      </SettingsRow>
      {showQr && (
        <PairingQrModal
          payload={qrPayload}
          onClose={() => setShowQr(false)}
        />
      )}
    </>
  );
}

function PairingQrModal({
  payload,
  onClose,
}: {
  payload: string;
  onClose: () => void;
}) {
  return (
    <div
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.45)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 1100,
      }}
    >
      <div
        style={{
          width: 380,
          background: "var(--surface)",
          border: "1px solid var(--line)",
          borderRadius: 10,
          padding: 22,
          boxShadow: "0 20px 60px rgba(0,0,0,0.35)",
          fontFamily: "var(--sans)",
          color: "var(--text)",
          textAlign: "center",
        }}
      >
        <div
          style={{
            fontSize: 11,
            fontWeight: 600,
            letterSpacing: 0.4,
            textTransform: "uppercase",
            color: "var(--text-dim)",
            marginBottom: 14,
          }}
        >
          Pair this Atlas with your phone
        </div>
        <div
          style={{
            background: "white",
            padding: 16,
            borderRadius: 8,
            display: "inline-block",
          }}
        >
          <QRCodeSVG value={payload} size={280} level="M" />
        </div>
        <div
          style={{
            fontSize: 11,
            color: "var(--text-dim)",
            marginTop: 14,
            lineHeight: 1.5,
          }}
        >
          The mobile app reads {`{deviceId, publicKey, relayUrl}`} from this
          code and registers with the relay. Anyone with this QR can pair —
          regenerate via Reset Pairing if it leaks.
        </div>
        <div style={{ marginTop: 16 }}>
          <button onClick={onClose} style={GHOST_BTN}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

// About section - crediting the author and providing direct links. Also
function AboutSection() {
  const AUTHOR = {
    name: "Amr Ebada",
    email: "amr.app.engine@gmail.com",
    website: "https://amrebada.com",
    linkedin: "https://www.linkedin.com/in/amrebada/",
  };
  return (
    <div>
      <SectionHdr>About Atlas</SectionHdr>
      <div
        style={{
          padding: "16px 0 22px 0",
          borderBottom: "1px solid var(--line-soft)",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <img
            src={atlasIconUrl}
            alt="Atlas"
            width={42}
            height={42}
            style={{ borderRadius: 10, display: "block" }}
          />
          <div>
            <div style={{ fontSize: 16, fontWeight: 600 }}>Atlas</div>
            <div
              style={{
                fontSize: 12,
                color: "var(--text-dim)",
                fontFamily: "var(--mono)",
              }}
            >
              Desktop-native command hub for local git projects
            </div>
          </div>
        </div>
      </div>

      <SectionHdr>Version</SectionHdr>
      <UpdateChecker />

      <SectionHdr>Author</SectionHdr>
      <SettingsRow label="Name">
        <span style={{ fontSize: 13 }}>{AUTHOR.name}</span>
      </SettingsRow>
      <SettingsRow label="Website">
        <ExternalLink href={AUTHOR.website}>{AUTHOR.website}</ExternalLink>
      </SettingsRow>
      <SettingsRow label="Email">
        <ExternalLink href={`mailto:${AUTHOR.email}`}>
          {AUTHOR.email}
        </ExternalLink>
      </SettingsRow>
      <SettingsRow label="LinkedIn">
        <ExternalLink href={AUTHOR.linkedin}>{AUTHOR.linkedin}</ExternalLink>
      </SettingsRow>
    </div>
  );
}

type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "up_to_date" }
  | { kind: "available"; version: string; notes?: string }
  | { kind: "downloading"; version: string }
  | { kind: "ready"; version: string }
  | { kind: "error"; message: string };

// Manual update check — shows the running version and lets the user
// trigger a check on demand. Useful for verifying an update flows from
// `latest.json` end-to-end without waiting for the once-per-launch hook.
function UpdateChecker() {
  const pushToast = useUiStore((s) => s.pushToast);
  const [version, setVersion] = useState<string>("…");
  const [state, setState] = useState<UpdateState>({ kind: "idle" });

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion("unknown"));
  }, []);

  const onCheck = async () => {
    setState({ kind: "checking" });
    try {
      const update = await checkForUpdate();
      if (!update) {
        setState({ kind: "up_to_date" });
        return;
      }
      setState({
        kind: "available",
        version: update.version,
        notes: update.body ?? undefined,
      });
      // Begin downloading immediately so the user can verify install +
      // restart in one click cycle.
      setState({ kind: "downloading", version: update.version });
      await update.downloadAndInstall();
      setState({ kind: "ready", version: update.version });
      pushToast(
        "success",
        `Atlas ${update.version} installed — restarting…`,
        2_500,
      );
      setTimeout(() => {
        void relaunch();
      }, 1_500);
    } catch (err) {
      const message = String(err);
      setState({ kind: "error", message });
      pushToast("error", `Update check failed: ${message}`);
    }
  };

  const busy = state.kind === "checking" || state.kind === "downloading";

  return (
    <>
      <SettingsRow label="Current version">
        <code style={CODE_STYLE}>{version}</code>
      </SettingsRow>
      <SettingsRow
        label="Check for updates"
        hint="Pulls the latest manifest and installs if a newer build is available."
      >
        <button
          onClick={onCheck}
          disabled={busy}
          style={{
            ...PRIMARY_BTN,
            opacity: busy ? 0.6 : 1,
            cursor: busy ? "default" : "pointer",
          }}
        >
          {state.kind === "checking" && "Checking…"}
          {state.kind === "downloading" && "Downloading…"}
          {state.kind !== "checking" && state.kind !== "downloading" &&
            "Check now"}
        </button>
      </SettingsRow>
      {state.kind !== "idle" && (
        <div
          style={{
            fontSize: 11,
            fontFamily: "var(--mono)",
            color:
              state.kind === "error"
                ? "var(--danger)"
                : state.kind === "up_to_date"
                  ? "var(--text-dim)"
                  : "var(--accent)",
            padding: "8px 0 14px",
          }}
        >
          {state.kind === "up_to_date" && "You're on the latest version."}
          {state.kind === "available" &&
            `Update ${state.version} available — preparing install…`}
          {state.kind === "downloading" &&
            `Downloading ${state.version}…`}
          {state.kind === "ready" &&
            `Atlas ${state.version} installed — restarting.`}
          {state.kind === "error" && state.message}
        </div>
      )}
    </>
  );
}

// Opens a URL (or mailto:) via the Tauri opener plugin so it lands in
function ExternalLink({
  href,
  children,
}: {
  href: string;
  children: React.ReactNode;
}) {
  const handle = async (e: React.MouseEvent) => {
    e.preventDefault();
    try {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(href);
    } catch {
      window.open(href, "_blank", "noopener,noreferrer");
    }
  };
  return (
    <a
      href={href}
      onClick={handle}
      style={{
        fontSize: 12,
        fontFamily: "var(--mono)",
        color: "var(--accent)",
        textDecoration: "none",
      }}
      onMouseEnter={(e) =>
        ((e.target as HTMLAnchorElement).style.textDecoration = "underline")
      }
      onMouseLeave={(e) =>
        ((e.target as HTMLAnchorElement).style.textDecoration = "none")
      }
    >
      {children}
    </a>
  );
}

// -----------------------------------------------------------------------------

function SectionHdr({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        fontSize: 18,
        fontWeight: 600,
        marginBottom: 14,
        letterSpacing: -0.2,
      }}
    >
      {children}
    </div>
  );
}

function SettingsRow({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        padding: "12px 0",
        borderBottom: "1px solid var(--line-soft)",
      }}
    >
      <div style={{ flex: 1 }}>
        <div style={{ fontSize: 13, color: "var(--text)" }}>{label}</div>
        {hint && (
          <div
            style={{
              fontSize: 11,
              color: "var(--text-dim)",
              marginTop: 2,
            }}
          >
            {hint}
          </div>
        )}
      </div>
      {children}
    </div>
  );
}

function Toggle({
  on,
  onChange,
}: {
  on: boolean;
  onChange: (next: boolean) => void;
}) {
  return (
    <button
      onClick={() => onChange(!on)}
      style={{
        width: 32,
        height: 18,
        borderRadius: 10,
        background: on ? "var(--accent)" : "var(--line)",
        border: "none",
        cursor: "pointer",
        position: "relative",
        padding: 0,
        transition: "background 120ms",
      }}
    >
      <div
        style={{
          position: "absolute",
          top: 2,
          left: on ? 16 : 2,
          width: 14,
          height: 14,
          borderRadius: "50%",
          background: on ? "var(--accent-fg)" : "var(--text-dim)",
          transition: "left 120ms",
        }}
      />
    </button>
  );
}

// Local Kbd mirror - imported Kbd uses Tailwind classes, but this panel
function KbdInline({ children }: { children: React.ReactNode }) {
  return (
    <span
      style={{
        display: "inline-flex",
        minWidth: 18,
        height: 18,
        alignItems: "center",
        justifyContent: "center",
        padding: "0 5px",
        border: "1px solid var(--line)",
        borderRadius: 3,
        background: "var(--kbd-bg)",
        fontFamily: "var(--mono)",
        fontSize: 11,
        fontWeight: 500,
        color: "var(--text-dim)",
      }}
    >
      {children}
    </span>
  );
}

// Text input that persists edits via a 400 ms debounce + commit-on-blur
function DebouncedInput({
  value,
  placeholder,
  onCommit,
}: {
  value: string;
  placeholder?: string;
  onCommit: (v: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  // Keep the draft in sync when the upstream value changes (e.g. after
  useEffect(() => {
    setDraft(value);
  }, [value]);
  // Debounced commit - 400 ms after the last keystroke.
  useEffect(() => {
    if (draft === value) return;
    const t = window.setTimeout(() => onCommit(draft), 400);
    return () => window.clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [draft]);
  return (
    <input
      type="text"
      value={draft}
      placeholder={placeholder}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        if (draft !== value) onCommit(draft);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          if (draft !== value) onCommit(draft);
        }
      }}
      style={{
        width: 260,
        padding: "5px 9px",
        fontSize: 12,
        fontFamily: "var(--mono)",
        background: "var(--surface-2)",
        border: "1px solid var(--line)",
        borderRadius: 5,
        color: "var(--text)",
        outline: "none",
      }}
    />
  );
}

const INPUT_STYLE: React.CSSProperties = {
  width: "100%",
  padding: "7px 10px",
  fontSize: 13,
  background: "var(--bg)",
  border: "1px solid var(--line)",
  borderRadius: 5,
  color: "var(--text)",
  outline: "none",
  fontFamily: "var(--sans)",
};
const GHOST_BTN: React.CSSProperties = {
  padding: "6px 12px",
  fontSize: 12,
  height: 28,
  background: "transparent",
  border: "1px solid var(--line)",
  borderRadius: 5,
  color: "var(--text)",
  cursor: "pointer",
  fontFamily: "var(--sans)",
  display: "inline-flex",
  alignItems: "center",
  gap: 6,
  // A long sibling (like a full editor path in `<code>`) was squeezing
  flexShrink: 0,
  whiteSpace: "nowrap",
};
const PRIMARY_BTN: React.CSSProperties = {
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
const CODE_STYLE: React.CSSProperties = {
  fontFamily: "var(--mono)",
  fontSize: 11,
  padding: "2px 7px",
  borderRadius: 3,
  background: "var(--surface-2)",
  color: "var(--text-dim)",
};
const SELECT_STYLE: React.CSSProperties = {
  padding: "4px 8px",
  fontSize: 12,
  background: "var(--surface-2)",
  border: "1px solid var(--line)",
  borderRadius: 4,
  color: "var(--text)",
  fontFamily: "var(--sans)",
};
