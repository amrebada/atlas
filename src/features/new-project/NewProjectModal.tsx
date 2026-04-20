import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Icon, LangDot } from "../../components/Icon";
import {
  createProjectFromTemplate,
  discoverProjects,
  listTemplates,
} from "../../ipc";
import { useUiStore } from "../../state/store";
import type { NewProjectTab } from "../../state/store";
import type { Template } from "../../types";

// Atlas - New Project modal.

const DEFAULT_TEMPLATES: Template[] = [
  {
    id: "node-ts",
    label: "Node + TypeScript",
    color: "#3178C6",
    hint: "pnpm · vitest · tsup",
    path: "",
    builtin: true,
  },
  {
    id: "rust-cli",
    label: "Rust CLI",
    color: "#E0763C",
    hint: "cargo · clap · anyhow",
    path: "",
    builtin: true,
  },
  {
    id: "python-uv",
    label: "Python (uv)",
    color: "#3572A5",
    hint: "uv · ruff · pytest",
    path: "",
    builtin: true,
  },
  {
    id: "go-svc",
    label: "Go service",
    color: "#00ADD8",
    hint: "mod · chi · testify",
    path: "",
    builtin: true,
  },
  {
    id: "empty",
    label: "Empty folder",
    color: "#888",
    hint: "no scaffolding",
    path: "",
    builtin: true,
  },
];

export function NewProjectModal() {
  const state = useUiStore((s) => s.newProjectOpen);
  const close = useUiStore((s) => s.closeNewProject);
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();

  const [tab, setTab] = useState<NewProjectTab>(state?.tab ?? "new");

  // Tolerate `templates_list` not being registered yet.
  const { data: fetchedTemplates } = useQuery<Template[]>({
    queryKey: ["templates"],
    queryFn: listTemplates,
    retry: false,
    enabled: state != null,
  });
  const templates =
    fetchedTemplates && fetchedTemplates.length > 0
      ? fetchedTemplates
      : DEFAULT_TEMPLATES;

  // New tab state.
  const [name, setName] = useState("");
  const [location, setLocation] = useState("~/code");
  const [templateId, setTemplateId] = useState("node-ts");
  const [initGit, setInitGit] = useState(true);
  const [createEnv, setCreateEnv] = useState(true);
  const [openInCode, setOpenInCode] = useState(true);

  // Clone tab state.
  const [gitUrl, setGitUrl] = useState("");
  const [cloneDest, setCloneDest] = useState("~/code");
  const [shallow, setShallow] = useState(false);
  const [submodules, setSubmodules] = useState(false);

  // Import tab state.
  const [importPath, setImportPath] = useState<string | null>(null);
  const [importing, setImporting] = useState(false);

  // New-tab submission state.
  const [creating, setCreating] = useState(false);

  // Sync tab with external open() calls.
  useEffect(() => {
    if (state) setTab(state.tab);
  }, [state]);

  if (!state) return null;

  const pickFolder = async (setter: (path: string) => void) => {
    try {
      const picked = await openDialog({ directory: true, multiple: false });
      if (typeof picked === "string") setter(picked);
    } catch (err) {
      pushToast("error", `Folder picker failed: ${String(err)}`);
    }
  };

  const handleImport = async () => {
    if (!importPath) return;
    try {
      setImporting(true);
      // discovery pipeline (picks up `.git` + writes `.atlas/project.json`).
      await discoverProjects(importPath, 0);
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      pushToast("success", `Imported ${importPath}`);
      close();
    } catch (err) {
      pushToast("error", `Import failed: ${String(err)}`);
    } finally {
      setImporting(false);
    }
  };

  const setSelectedProjectId = useUiStore((s) => s.setSelectedProjectId);
  const handleCreate = async () => {
    const trimmed = name.trim();
    const parent = location.trim();
    if (!trimmed || !parent) return;
    try {
      setCreating(true);
      const newId = await createProjectFromTemplate({
        name: trimmed,
        parent,
        templateId,
        initGit,
        createEnv,
        openInEditor: openInCode ? "vscode" : null,
      });
      await queryClient.invalidateQueries({ queryKey: ["projects"] });
      setSelectedProjectId(newId);
      pushToast("success", `Created ${trimmed}`);
      close();
    } catch (err) {
      pushToast("error", `Create failed: ${String(err)}`);
    } finally {
      setCreating(false);
    }
  };

  const handleClone = () => {
    if (!gitUrl.trim() || !cloneDest.trim()) return;
    pushToast(
      "info",
      `Cloning is not available yet. ${gitUrl} → ${cloneDest}${shallow ? " (depth 1)" : ""}`,
    );
    close();
  };

  const createDisabled = !name.trim() || !location.trim() || creating;
  const cloneDisabled = !gitUrl.trim() || !cloneDest.trim();
  const importDisabled = !importPath || importing;

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
        paddingTop: "10vh",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label="New project"
        style={{
          width: 560,
          background: "var(--surface)",
          border: "1px solid var(--line)",
          borderRadius: 10,
          overflow: "hidden",
          boxShadow: "0 30px 80px rgba(0,0,0,0.55)",
          fontFamily: "var(--sans)",
          color: "var(--text)",
        }}
      >
        {/* Tab strip */}
        <div
          style={{
            display: "flex",
            padding: "4px 4px 0",
            borderBottom: "1px solid var(--line)",
            background: "var(--chrome)",
          }}
        >
          {(
            [
              { id: "new", icon: "plus", label: "New from template" },
              { id: "clone", icon: "clone", label: "Clone from git" },
              { id: "import", icon: "import", label: "Import folder" },
            ] as const
          ).map((t) => (
            <button
              key={t.id}
              onClick={() => setTab(t.id)}
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 6,
                padding: "10px 14px",
                background: "none",
                border: "none",
                borderBottom:
                  "2px solid " +
                  (tab === t.id ? "var(--accent)" : "transparent"),
                color: tab === t.id ? "var(--text)" : "var(--text-dim)",
                fontSize: 12,
                cursor: "pointer",
                fontFamily: "var(--sans)",
              }}
            >
              <Icon name={t.icon} size={12} />
              {t.label}
            </button>
          ))}
          <span style={{ flex: 1 }} />
          <button
            onClick={close}
            aria-label="Close"
            style={{
              background: "none",
              border: "none",
              color: "var(--text-dim)",
              cursor: "pointer",
              padding: "10px 14px",
              fontSize: 14,
            }}
          >
            ×
          </button>
        </div>

        {/* Body */}
        <div style={{ padding: 18 }}>
          {tab === "new" && (
            <>
              <Field label="Project name">
                <input
                  autoFocus
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="my-next-thing"
                  style={INPUT_STYLE}
                />
              </Field>
              <Field label="Location">
                <div style={{ display: "flex", gap: 6 }}>
                  <input
                    value={location}
                    onChange={(e) => setLocation(e.target.value)}
                    style={{
                      ...INPUT_STYLE,
                      flex: 1,
                      fontFamily: "var(--mono)",
                    }}
                  />
                  <button
                    style={GHOST_BTN}
                    onClick={() => pickFolder(setLocation)}
                  >
                    Browse…
                  </button>
                </div>
              </Field>
              <Field label="Template">
                <div
                  style={{
                    display: "grid",
                    gridTemplateColumns: "repeat(2, 1fr)",
                    gap: 6,
                  }}
                >
                  {templates.map((t) => {
                    const active = templateId === t.id;
                    return (
                      <div
                        key={t.id}
                        onClick={() => setTemplateId(t.id)}
                        style={{
                          padding: "10px 12px",
                          border:
                            "1px solid " +
                            (active ? "var(--accent)" : "var(--line)"),
                          borderRadius: 5,
                          cursor: "pointer",
                          background: active
                            ? "var(--row-active)"
                            : "transparent",
                        }}
                      >
                        <div
                          style={{
                            display: "flex",
                            alignItems: "center",
                            gap: 8,
                            marginBottom: 2,
                          }}
                        >
                          <LangDot color={t.color} />
                          <span style={{ fontSize: 12, fontWeight: 500 }}>
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
                              }}
                            >
                              built-in
                            </span>
                          )}
                        </div>
                        <div
                          style={{
                            fontSize: 10,
                            fontFamily: "var(--mono)",
                            color: "var(--text-dim)",
                          }}
                        >
                          {t.hint}
                        </div>
                      </div>
                    );
                  })}
                </div>
              </Field>
              <Field label="Also">
                <div
                  style={{
                    display: "flex",
                    gap: 14,
                    fontSize: 12,
                    color: "var(--text-dim)",
                  }}
                >
                  <Check
                    label="Initialize git"
                    checked={initGit}
                    onChange={setInitGit}
                  />
                  <Check
                    label="Create .env"
                    checked={createEnv}
                    onChange={setCreateEnv}
                  />
                  <Check
                    label="Open in VSCode"
                    checked={openInCode}
                    onChange={setOpenInCode}
                  />
                </div>
              </Field>
            </>
          )}

          {tab === "clone" && (
            <>
              <Field label="Git URL">
                <input
                  autoFocus
                  value={gitUrl}
                  onChange={(e) => setGitUrl(e.target.value)}
                  placeholder="git@github.com:acme/widget.git"
                  style={{ ...INPUT_STYLE, fontFamily: "var(--mono)" }}
                />
              </Field>
              <Field label="Destination">
                <div style={{ display: "flex", gap: 6 }}>
                  <input
                    value={cloneDest}
                    onChange={(e) => setCloneDest(e.target.value)}
                    style={{
                      ...INPUT_STYLE,
                      flex: 1,
                      fontFamily: "var(--mono)",
                    }}
                  />
                  <button
                    style={GHOST_BTN}
                    onClick={() => pickFolder(setCloneDest)}
                  >
                    Browse…
                  </button>
                </div>
              </Field>
              <Field label="Shallow clone">
                <div
                  style={{
                    display: "flex",
                    gap: 14,
                    fontSize: 12,
                    color: "var(--text-dim)",
                  }}
                >
                  <Check
                    label="depth 1"
                    checked={shallow}
                    onChange={setShallow}
                  />
                  <Check
                    label="recurse submodules"
                    checked={submodules}
                    onChange={setSubmodules}
                  />
                </div>
              </Field>
            </>
          )}

          {tab === "import" && (
            <ImportTab
              path={importPath}
              onPick={async () => {
                try {
                  const picked = await openDialog({
                    directory: true,
                    multiple: false,
                  });
                  if (typeof picked === "string") setImportPath(picked);
                } catch (err) {
                  pushToast("error", `Folder picker failed: ${String(err)}`);
                }
              }}
              onDrop={(p) => setImportPath(p)}
            />
          )}
        </div>

        {/* Footer */}
        <div
          style={{
            display: "flex",
            gap: 8,
            padding: 14,
            borderTop: "1px solid var(--line)",
            background: "var(--chrome)",
            justifyContent: "flex-end",
          }}
        >
          <button onClick={close} style={GHOST_BTN}>
            Cancel
          </button>
          {tab === "new" && (
            <button
              onClick={handleCreate}
              disabled={createDisabled}
              style={{
                ...PRIMARY_BTN,
                opacity: createDisabled ? 0.5 : 1,
                cursor: createDisabled ? "not-allowed" : "pointer",
              }}
            >
              {creating ? "Creating…" : "Create project"}
            </button>
          )}
          {tab === "clone" && (
            <button
              onClick={handleClone}
              disabled={cloneDisabled}
              style={{
                ...PRIMARY_BTN,
                opacity: cloneDisabled ? 0.5 : 1,
                cursor: cloneDisabled ? "not-allowed" : "pointer",
              }}
            >
              Clone
            </button>
          )}
          {tab === "import" && (
            <button
              onClick={handleImport}
              disabled={importDisabled}
              style={{
                ...PRIMARY_BTN,
                opacity: importDisabled ? 0.5 : 1,
                cursor: importDisabled ? "not-allowed" : "pointer",
              }}
            >
              {importing ? "Importing…" : "Import"}
            </button>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}

function ImportTab({
  path,
  onPick,
  onDrop,
}: {
  path: string | null;
  onPick: () => void;
  onDrop: (path: string) => void;
}) {
  const dropRef = useRef<HTMLDivElement | null>(null);
  const [hover, setHover] = useState(false);

  // HTML5 drag-drop fallback for files dropped from Finder/Explorer. Tauri
  useEffect(() => {
    const el = dropRef.current;
    if (!el) return;
    const onDragOver = (e: DragEvent) => {
      e.preventDefault();
      setHover(true);
    };
    const onDragLeave = () => setHover(false);
    const onDropEvt = (e: DragEvent) => {
      e.preventDefault();
      setHover(false);
      const f = e.dataTransfer?.files?.[0];
      if (!f) return;
      // Tauri 2 webview exposes `path` on dropped-file objects; TS-land hides
      const p = (f as File & { path?: string }).path;
      if (p) onDrop(p);
      else onDrop(f.name);
    };
    el.addEventListener("dragover", onDragOver);
    el.addEventListener("dragleave", onDragLeave);
    el.addEventListener("drop", onDropEvt);
    return () => {
      el.removeEventListener("dragover", onDragOver);
      el.removeEventListener("dragleave", onDragLeave);
      el.removeEventListener("drop", onDropEvt);
    };
  }, [onDrop]);

  // Best-effort metadata preview. A proper backend scan (package.json /
  const hints = useMemo(() => {
    if (!path) return [];
    return [
      { label: "Path", value: path },
      {
        label: "Registers via",
        value: "projects_discover(path, 0)",
      },
    ];
  }, [path]);

  return (
    <>
      <Field label="Folder">
        <div
          ref={dropRef}
          onClick={onPick}
          style={{
            padding: 22,
            border:
              "1px dashed " + (hover ? "var(--accent)" : "var(--line)"),
            borderRadius: 6,
            textAlign: "center",
            color: "var(--text-dim)",
            fontSize: 12,
            cursor: "pointer",
            background: hover ? "var(--row-active)" : "transparent",
          }}
        >
          <Icon name="folder" size={22} stroke="var(--text-dim)" />
          <div style={{ marginTop: 8 }}>
            {path ? (
              <span style={{ fontFamily: "var(--mono)" }}>{path}</span>
            ) : (
              <>
                Drag a folder here, or{" "}
                <span
                  style={{
                    color: "var(--accent)",
                    textDecoration: "underline",
                  }}
                >
                  browse
                </span>
              </>
            )}
          </div>
        </div>
      </Field>
      {path && (
        <Field label="Detected">
          <div
            style={{
              fontSize: 11,
              fontFamily: "var(--mono)",
              color: "var(--text-dim)",
              lineHeight: 1.8,
            }}
          >
            {hints.map((h) => (
              <div key={h.label}>
                · {h.label}: {h.value}
              </div>
            ))}
          </div>
        </Field>
      )}
    </>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div style={{ marginBottom: 14 }}>
      <div
        style={{
          fontSize: 10,
          fontFamily: "var(--mono)",
          color: "var(--text-dim)",
          textTransform: "uppercase",
          letterSpacing: 0.6,
          marginBottom: 5,
        }}
      >
        {label}
      </div>
      {children}
    </div>
  );
}

function Check({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (next: boolean) => void;
}) {
  return (
    <label
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        cursor: "pointer",
        userSelect: "none",
      }}
      onClick={(e) => {
        e.preventDefault();
        onChange(!checked);
      }}
    >
      <Icon
        name={checked ? "square-check" : "square"}
        size={13}
        stroke={checked ? "var(--accent)" : "var(--text-dim)"}
      />
      <span>{label}</span>
    </label>
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
