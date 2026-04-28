import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Icon } from "../../components/Icon";
import atlasIconUrl from "../../assets/atlas-icon.png";
import {
  addWatcher,
  detectEditors,
  discoverProjects,
  getSettings,
  listTemplates,
  listWatchers,
  removeTemplate,
  removeWatcher,
  setSettings,
  upsertTemplate,
} from "../../ipc";
import { useUiStore } from "../../state/store";
import type { SettingsSection } from "../../state/store";
import type {
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
    mutationFn: (path: string) => removeWatcher(path),
    onSuccess: (_res, path) => {
      queryClient.invalidateQueries({ queryKey: ["watchRoots"] });
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      pushToast("info", `Stopped watching ${path}`);
    },
    onError: (err) => pushToast("error", `Remove failed: ${String(err)}`),
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
      {watchers.map((w) => (
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
            disabled={removeMut.isPending}
            onClick={() => removeMut.mutate(w.path)}
          >
            Remove
          </button>
        </div>
      ))}
      <button
        onClick={onAdd}
        disabled={addMut.isPending}
        style={{ ...GHOST_BTN, marginTop: 12 }}
      >
        <Icon name="plus" size={11} />
        {addMut.isPending ? "Adding…" : "Add watcher…"}
      </button>
    </div>
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

function AdvancedSection({ settings }: { settings?: Settings }) {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);
  const adv = settings?.advanced;

  const mutation = useMutation({
    mutationFn: (patch: Partial<Settings["advanced"]>) =>
      setSettings({
        advanced: {
          useSpotlight: adv?.useSpotlight ?? false,
          crashReports: adv?.crashReports ?? false,
          shell: adv?.shell ?? "/bin/zsh",
          ...patch,
        },
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["settings"] }),
    onError: (err) => pushToast("error", `Save failed: ${String(err)}`),
  });

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
