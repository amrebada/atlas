import {
  useEffect,
  useMemo,
  useState,
  type KeyboardEvent,
  type ChangeEvent,
} from "react";
import { createPortal } from "react-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Icon } from "../../Icon";
import { TabEmpty, TabError, TabSkeleton } from "../TabStates";
import { useUiStore } from "../../../state/store";
import {
  deleteScript as ipcDeleteScript,
  listScripts,
  scriptsRun,
  scriptsRunWithEnv,
  upsertScript,
} from "../../../ipc";
import { newScriptId } from "../../../features/inspector/ids";
import { spawnScriptPane } from "../../../features/terminal/TerminalStrip";
import { useTerminalStore, makePane } from "../../../features/terminal/layout";
import type {
  Project,
  Script,
  ScriptEnvVar,
  ScriptGroup,
} from "../../../types";

// Atlas - Inspector / Scripts tab.

interface ScriptsProps {
  project: Project;
}

const GROUPS: { key: ScriptGroup; label: string }[] = [
  { key: "run", label: "run" },
  { key: "build", label: "build" },
  { key: "check", label: "checks" },
  { key: "util", label: "utilities" },
];

interface Draft {
  name: string;
  cmd: string;
  group: ScriptGroup;
  envDefaults: ScriptEnvVar[];
}

const EMPTY_DRAFT: Draft = { name: "", cmd: "", group: "run", envDefaults: [] };

export function Scripts({ project }: ScriptsProps) {
  const queryClient = useQueryClient();
  const pushToast = useUiStore((s) => s.pushToast);
  const queryKey = useMemo(
    () => ["scripts", project.id] as const,
    [project.id],
  );

  // editingId: null = idle, '__new' = adding, otherwise script id being edited.
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  // The script whose env the user is currently editing before launch.
  const [runEnvFor, setRunEnvFor] = useState<Script | null>(null);

  useEffect(() => {
    setEditingId(null);
    setDraft(EMPTY_DRAFT);
    setSelected(new Set());
    setRunEnvFor(null);
  }, [project.id]);

  const { data, isLoading, error, refetch } = useQuery<Script[]>({
    queryKey: [...queryKey],
    queryFn: () => listScripts(project.id),
    staleTime: 5_000,
    retry: false,
  });

  const scripts = data ?? [];

  const upsert = useMutation({
    mutationFn: (script: Script) => upsertScript(project.id, script),
    onMutate: async (script) => {
      await queryClient.cancelQueries({ queryKey: [...queryKey] });
      const previous = queryClient.getQueryData<Script[]>([...queryKey]) ?? [];
      const next = previous.some((s) => s.id === script.id)
        ? previous.map((s) => (s.id === script.id ? script : s))
        : [...previous, script];
      queryClient.setQueryData<Script[]>([...queryKey], next);
      return { previous };
    },
    onError: (err, _script, ctx) => {
      if (ctx?.previous)
        queryClient.setQueryData<Script[]>([...queryKey], ctx.previous);
      pushToast(
        "error",
        `Couldn't save script: ${err instanceof Error ? err.message : String(err)}`,
      );
    },
  });

  const remove = useMutation({
    mutationFn: (id: string) => ipcDeleteScript(project.id, id),
    onMutate: async (id) => {
      await queryClient.cancelQueries({ queryKey: [...queryKey] });
      const previous = queryClient.getQueryData<Script[]>([...queryKey]) ?? [];
      queryClient.setQueryData<Script[]>(
        [...queryKey],
        previous.filter((s) => s.id !== id),
      );
      return { previous };
    },
    onError: (err, _id, ctx) => {
      if (ctx?.previous)
        queryClient.setQueryData<Script[]>([...queryKey], ctx.previous);
      pushToast(
        "error",
        `Couldn't delete script: ${err instanceof Error ? err.message : String(err)}`,
      );
    },
  });

  const toggleSelect = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const selectAll = () => setSelected(new Set(scripts.map((s) => s.id)));
  const clearSelection = () => setSelected(new Set());

  // Central run dispatcher. Tries `scripts_run` first and
  const runScripts = async (targets: Script[]) => {
    if (targets.length === 0) return;

    let paneIds: string[] | null = null;
    try {
      paneIds = await scriptsRun(
        project.id,
        targets.map((s) => s.id),
      );
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error("[atlas] scripts_run failed:", err);
      pushToast("error", `scripts_run failed: ${String(err)}`);
      paneIds = null;
    }

    if (paneIds && paneIds.length === targets.length) {
      const addPane = useTerminalStore.getState().addPane;
      targets.forEach((s, i) => {
        addPane(
          makePane(paneIds![i], "script", project.path, s.name, {
            scriptId: s.id,
            status: "running",
            branch: project.branch,
            projectId: project.id,
            projectLabel: project.name,
            // Stash the shell invocation so the pane-header "rerun"
            command: "sh",
            args: ["-lc", s.cmd],
          }),
        );
      });
      return;
    }

    // Fallback: spawn via generic terminal_open per script. Keeps the UX
    let spawned = 0;
    for (const s of targets) {
      const id = await spawnScriptPane({
        projectId: project.id,
        projectLabel: project.name,
        cwd: project.path,
        scriptId: s.id,
        scriptName: s.name,
        cmd: s.cmd,
        branch: project.branch,
      });
      if (id) spawned++;
    }
    if (spawned === 0) {
      pushToast(
        "warn",
        "Couldn't launch scripts — terminal backend may not be ready",
      );
    }
  };

  // Launch a single script with user-supplied env overrides (from the
  // run-env modal). Uses the dedicated IPC so the backend can map
  // `[script_id, env]` tuples onto `scripts::runner::run` per invocation.
  const runWithEnv = async (
    script: Script,
    env: Array<[string, string]>,
  ) => {
    let paneIds: string[] | null = null;
    try {
      paneIds = await scriptsRunWithEnv(project.id, [
        { scriptId: script.id, env },
      ]);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error("[atlas] scripts_run_with_env failed:", err);
      pushToast(
        "error",
        `scripts_run_with_env failed: ${String(err)}`,
      );
      return;
    }
    const paneId = paneIds?.[0];
    if (!paneId) {
      pushToast("warn", "Script didn't return a pane id");
      return;
    }
    useTerminalStore.getState().addPane(
      makePane(paneId, "script", project.path, script.name, {
        scriptId: script.id,
        status: "running",
        branch: project.branch,
        projectId: project.id,
        projectLabel: project.name,
        command: "sh",
        args: ["-lc", script.cmd],
      }),
    );
  };

  const runSelected = () => {
    const arr = scripts.filter((s) => selected.has(s.id));
    if (arr.length === 0) return;
    void runScripts(arr);
    clearSelection();
  };
  const runAll = () => void runScripts(scripts);
  // Run a single script. If it declares env defaults, surface the modal
  // so the user can edit values before launch; otherwise run straight
  // through with the stored defaults already applied by the backend.
  const runOne = (s: Script) => {
    if ((s.env_defaults ?? []).length > 0) {
      setRunEnvFor(s);
      return;
    }
    void runScripts([s]);
  };

  const beginAdd = () => {
    setDraft(EMPTY_DRAFT);
    setEditingId("__new");
  };
  const beginEdit = (s: Script) => {
    setDraft({
      name: s.name,
      cmd: s.cmd,
      group: s.group,
      envDefaults: (s.env_defaults ?? []).map((v) => ({ ...v })),
    });
    setEditingId(s.id);
  };
  const cancel = () => {
    setEditingId(null);
    setDraft(EMPTY_DRAFT);
  };
  const commit = () => {
    const name = draft.name.trim();
    const cmd = draft.cmd.trim();
    if (!name || !cmd) {
      cancel();
      return;
    }
    // Drop empty-key rows; keep order so the modal renders them as typed.
    const envDefaults = draft.envDefaults
      .map((v) => ({ key: v.key.trim(), default: v.default }))
      .filter((v) => v.key.length > 0);
    if (editingId === "__new") {
      upsert.mutate({
        id: newScriptId(name),
        name,
        cmd,
        group: draft.group,
        env_defaults: envDefaults,
      });
    } else if (editingId) {
      const target = scripts.find((s) => s.id === editingId);
      if (target) {
        upsert.mutate({
          ...target,
          name,
          cmd,
          group: draft.group,
          env_defaults: envDefaults,
        });
      }
    }
    cancel();
  };

  const totalScripts = scripts.length;
  const selectedCount = selected.size;

  return (
    <div className="p-[14px] overflow-y-auto h-full">
      <div className="flex items-center mb-[10px] gap-2">
        <span className="font-mono text-[10px] text-text-dim uppercase tracking-[0.6px]">
          {totalScripts} script{totalScripts === 1 ? "" : "s"}
          {selectedCount > 0 ? ` · ${selectedCount} selected` : ""}
        </span>
        <div className="flex-1" />
        {totalScripts > 0 && selectedCount > 0 && (
          <>
            <button
              type="button"
              onClick={clearSelection}
              className="inline-flex items-center gap-[5px] px-[8px] py-[3px] font-mono text-[10px] text-text-dim border border-line rounded-[3px] hover:text-text"
            >
              clear
            </button>
            <button
              type="button"
              onClick={runSelected}
              title={`Run ${selectedCount}`}
              className="inline-flex items-center gap-[5px] px-[8px] py-[3px] font-mono text-[10px] rounded-[3px] font-semibold"
              style={{
                background: "var(--accent)",
                color: "var(--accent-fg)",
                border: "none",
              }}
            >
              <Icon name="play" size={9} stroke="var(--accent-fg)" />
              run {selectedCount}
            </button>
          </>
        )}
        {totalScripts > 0 && selectedCount === 0 && (
          <>
            <button
              type="button"
              onClick={selectAll}
              className="inline-flex items-center gap-[5px] px-[8px] py-[3px] font-mono text-[10px] text-text-dim border border-line rounded-[3px] hover:text-text"
            >
              select all
            </button>
            <button
              type="button"
              onClick={runAll}
              title="Run all"
              className="inline-flex items-center gap-[5px] px-[8px] py-[3px] font-mono text-[10px] rounded-[3px] font-semibold"
              style={{
                background: "var(--accent)",
                color: "var(--accent-fg)",
                border: "none",
              }}
            >
              <Icon name="play" size={9} stroke="var(--accent-fg)" />
              run all
            </button>
          </>
        )}
        <button
          type="button"
          onClick={beginAdd}
          disabled={editingId !== null}
          title="Add script"
          className="inline-flex items-center gap-[5px] px-[8px] py-[3px] font-mono text-[10px] text-text-dim border border-line rounded-[3px] hover:text-text disabled:opacity-50"
        >
          <Icon name="plus" size={9} stroke="currentColor" />
          add
        </button>
      </div>

      {editingId === "__new" && (
        <ScriptEditor
          draft={draft}
          onDraftChange={setDraft}
          onSave={commit}
          onCancel={cancel}
        />
      )}

      {isLoading && !data && <TabSkeleton rows={4} />}
      {error && (
        <TabError
          message={error instanceof Error ? error.message : String(error)}
          onRetry={() => void refetch()}
        />
      )}
      {!isLoading &&
        !error &&
        scripts.length === 0 &&
        editingId !== "__new" && (
          <TabEmpty
            icon="play"
            title="No scripts configured"
            hint="Click + add to define a run/build/check command"
          />
        )}

      {GROUPS.map(({ key, label }) => {
        const items = scripts.filter((s) => s.group === key);
        if (items.length === 0) return null;
        return (
          <div key={key} className="mb-[14px]">
            <div className="font-mono text-[9px] text-text-dimmer uppercase tracking-[0.8px] py-[4px] mb-[4px]">
              {label}
            </div>
            {items.map((s) =>
              editingId === s.id ? (
                <ScriptEditor
                  key={s.id}
                  draft={draft}
                  onDraftChange={setDraft}
                  onSave={commit}
                  onCancel={cancel}
                />
              ) : (
                <ScriptRow
                  key={s.id}
                  script={s}
                  selected={selected.has(s.id)}
                  onToggleSelected={() => toggleSelect(s.id)}
                  onRun={() => runOne(s)}
                  onEdit={() => beginEdit(s)}
                  onDelete={() => remove.mutate(s.id)}
                />
              ),
            )}
          </div>
        );
      })}

      {runEnvFor && (
        <RunEnvModal
          script={runEnvFor}
          onCancel={() => setRunEnvFor(null)}
          onContinue={(env) => {
            const target = runEnvFor;
            setRunEnvFor(null);
            void runWithEnv(target, env);
          }}
        />
      )}
    </div>
  );
}

function ScriptRow({
  script: s,
  selected,
  onToggleSelected,
  onRun,
  onEdit,
  onDelete,
}: {
  script: Script;
  selected: boolean;
  onToggleSelected: () => void;
  onRun: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  return (
    <div
      className="script-row group flex items-center gap-2 px-[10px] py-[8px] rounded-[4px] mb-[3px] relative"
      style={{
        background: selected ? "var(--row-active)" : "var(--surface-2)",
        border: "1px solid " + (selected ? "var(--accent)" : "transparent"),
      }}
    >
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onToggleSelected();
        }}
        title={selected ? "Deselect" : "Select"}
        aria-label={selected ? "Deselect script" : "Select script"}
        aria-pressed={selected}
        className="w-[18px] h-[18px] inline-flex items-center justify-center"
        style={{ background: "transparent", border: "none", flexShrink: 0 }}
      >
        <Icon
          name={selected ? "square-check" : "square"}
          size={13}
          stroke={selected ? "var(--accent)" : "var(--text-dim)"}
        />
      </button>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-[6px]">
          <span className="text-[12px] font-semibold text-text font-sans">
            {s.name}
          </span>
          {s.default && (
            <span className="font-mono text-[9px] px-[5px] py-[1px] rounded-[2px] bg-row-active text-text-dim uppercase tracking-[0.5px]">
              default
            </span>
          )}
        </div>
        <div className="font-mono text-[10px] text-text-dimmer mt-[2px] truncate">
          {s.cmd}
        </div>
      </div>
      <div className="flex gap-[2px] flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            onEdit();
          }}
          title="Edit"
          aria-label={`Edit script ${s.name}`}
          className="w-[22px] h-[22px] inline-flex items-center justify-center bg-transparent border border-line rounded-[3px] text-text-dim"
        >
          <Icon name="note" size={11} stroke="currentColor" />
        </button>
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            onDelete();
          }}
          title="Delete"
          aria-label={`Delete script ${s.name}`}
          className="w-[22px] h-[22px] inline-flex items-center justify-center bg-transparent border border-line rounded-[3px] text-text-dim"
        >
          <Icon name="trash" size={11} stroke="currentColor" />
        </button>
      </div>
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onRun();
        }}
        title={`Run ${s.name}`}
        aria-label={`Run script ${s.name}`}
        className="script-run-btn w-[24px] h-[24px] inline-flex items-center justify-center bg-surface border border-line rounded-[4px] flex-shrink-0"
        style={{ color: "var(--accent)" }}
      >
        <Icon name="play" size={10} stroke="currentColor" />
      </button>
    </div>
  );
}

function ScriptEditor({
  draft,
  onDraftChange,
  onSave,
  onCancel,
}: {
  draft: Draft;
  onDraftChange: (d: Draft) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  // `cmd` accepts both a single-line invocation ("pnpm dev") and a
  // pasted multi-line bash script — the textarea grows to fit.
  const [pasteMode, setPasteMode] = useState<boolean>(() =>
    draft.cmd.includes("\n"),
  );

  const onKey = (
    e: KeyboardEvent<HTMLInputElement | HTMLSelectElement>,
  ) => {
    if (e.key === "Enter") {
      e.preventDefault();
      onSave();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  };
  const onCmdKey = (
    e: KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>,
  ) => {
    // In paste mode Enter inserts a newline; Cmd/Ctrl+Enter saves.
    if (e.key === "Enter" && !pasteMode) {
      e.preventDefault();
      onSave();
    } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      onSave();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  };

  const onCmdPaste = (e: ChangeEvent<HTMLInputElement>) => {
    // Auto-promote to paste mode the moment the user pastes something
    // that clearly isn't a single-line cmd (has a newline or leading #!).
    const v = e.target.value;
    if (!pasteMode && (v.includes("\n") || v.startsWith("#!"))) {
      setPasteMode(true);
    }
    onDraftChange({ ...draft, cmd: v });
  };

  const addEnv = () =>
    onDraftChange({
      ...draft,
      envDefaults: [...draft.envDefaults, { key: "", default: "" }],
    });
  const updateEnv = (i: number, patch: Partial<ScriptEnvVar>) => {
    const next = draft.envDefaults.slice();
    next[i] = { ...next[i], ...patch };
    onDraftChange({ ...draft, envDefaults: next });
  };
  const removeEnv = (i: number) => {
    const next = draft.envDefaults.slice();
    next.splice(i, 1);
    onDraftChange({ ...draft, envDefaults: next });
  };

  const cmdRows = Math.min(
    12,
    Math.max(3, draft.cmd.split("\n").length + 1),
  );

  return (
    <div className="p-[10px] mb-[3px] rounded-[4px] bg-surface border border-accent">
      <div className="flex gap-[6px] mb-[6px]">
        <input
          autoFocus
          value={draft.name}
          onChange={(e) => onDraftChange({ ...draft, name: e.target.value })}
          onKeyDown={onKey}
          placeholder="name (e.g. dev)"
          className="flex-1 min-w-0 bg-bg border border-line rounded-[3px] px-[8px] py-[5px] outline-none text-text font-mono text-[11px]"
        />
        <select
          value={draft.group}
          onChange={(e) =>
            onDraftChange({ ...draft, group: e.target.value as ScriptGroup })
          }
          onKeyDown={onKey}
          className="w-[84px] flex-shrink-0 cursor-pointer bg-bg border border-line rounded-[3px] px-[8px] py-[5px] outline-none text-text font-mono text-[11px]"
        >
          <option value="run">run</option>
          <option value="build">build</option>
          <option value="check">check</option>
          <option value="util">util</option>
        </select>
      </div>

      <div className="flex items-center justify-between mb-[4px]">
        <span className="font-mono text-[9px] text-text-dimmer uppercase tracking-[0.8px]">
          command
        </span>
        <button
          type="button"
          onClick={() => setPasteMode((v) => !v)}
          title={
            pasteMode
              ? "Switch to single-line input"
              : "Paste a multi-line bash script"
          }
          className="inline-flex items-center gap-[4px] px-[6px] py-[2px] font-mono text-[9px] text-text-dim border border-line rounded-[3px] hover:text-text"
        >
          {pasteMode ? "single-line" : "paste script"}
        </button>
      </div>
      {pasteMode ? (
        <textarea
          value={draft.cmd}
          onChange={(e) => onDraftChange({ ...draft, cmd: e.target.value })}
          onKeyDown={onCmdKey}
          placeholder={"#!/usr/bin/env bash\nset -euo pipefail\n..."}
          rows={cmdRows}
          className="w-full resize-y bg-bg border border-line rounded-[3px] px-[8px] py-[5px] mb-[8px] outline-none text-text font-mono text-[11px] leading-[1.4]"
        />
      ) : (
        <input
          value={draft.cmd}
          onChange={onCmdPaste}
          onKeyDown={onCmdKey}
          placeholder="command (e.g. pnpm dev)"
          className="w-full bg-bg border border-line rounded-[3px] px-[8px] py-[5px] mb-[8px] outline-none text-text font-mono text-[11px]"
        />
      )}

      <div className="flex items-center justify-between mb-[4px]">
        <span className="font-mono text-[9px] text-text-dimmer uppercase tracking-[0.8px]">
          env vars
          {draft.envDefaults.length > 0
            ? ` · ${draft.envDefaults.length}`
            : ""}
        </span>
        <button
          type="button"
          onClick={addEnv}
          title="Add env var"
          className="inline-flex items-center gap-[4px] px-[6px] py-[2px] font-mono text-[9px] text-text-dim border border-line rounded-[3px] hover:text-text"
        >
          <Icon name="plus" size={8} stroke="currentColor" />
          add
        </button>
      </div>
      {draft.envDefaults.length > 0 && (
        <div className="mb-[8px] flex flex-col gap-[4px]">
          {draft.envDefaults.map((v, i) => (
            <div key={i} className="flex gap-[4px] items-center">
              <input
                value={v.key}
                onChange={(e) => updateEnv(i, { key: e.target.value })}
                placeholder="KEY"
                className="w-[40%] min-w-0 bg-bg border border-line rounded-[3px] px-[8px] py-[4px] outline-none text-text font-mono text-[11px] uppercase"
              />
              <span className="font-mono text-[11px] text-text-dimmer">=</span>
              <input
                value={v.default}
                onChange={(e) => updateEnv(i, { default: e.target.value })}
                placeholder="default"
                className="flex-1 min-w-0 bg-bg border border-line rounded-[3px] px-[8px] py-[4px] outline-none text-text font-mono text-[11px]"
              />
              <button
                type="button"
                onClick={() => removeEnv(i)}
                title="Remove"
                aria-label={`Remove env var ${v.key || "row"}`}
                className="w-[22px] h-[22px] inline-flex items-center justify-center bg-transparent border border-line rounded-[3px] text-text-dim flex-shrink-0"
              >
                <Icon name="trash" size={10} stroke="currentColor" />
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="flex gap-[6px] justify-end">
        <button
          type="button"
          onClick={onCancel}
          className="inline-flex items-center px-[8px] py-[3px] font-mono text-[10px] text-text-dim border border-line rounded-[3px]"
        >
          cancel
        </button>
        <button
          type="button"
          onClick={onSave}
          className="inline-flex items-center px-[8px] py-[3px] font-mono text-[10px] bg-accent text-accent-fg rounded-[3px] font-semibold"
        >
          save
        </button>
      </div>
    </div>
  );
}

function RunEnvModal({
  script,
  onContinue,
  onCancel,
}: {
  script: Script;
  onContinue: (env: Array<[string, string]>) => void;
  onCancel: () => void;
}) {
  const [values, setValues] = useState<string[]>(() =>
    (script.env_defaults ?? []).map((v) => v.default),
  );
  const vars = script.env_defaults ?? [];

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      submit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  };

  const submit = () => {
    const env: Array<[string, string]> = vars.map((v, i) => [
      v.key,
      values[i] ?? v.default,
    ]);
    onContinue(env);
  };

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      style={{ background: "rgba(0,0,0,0.5)" }}
      onClick={onCancel}
    >
      <div
        className="w-[420px] max-w-[90vw] rounded-[6px] bg-surface border border-line p-[16px]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-[6px] mb-[12px]">
          <Icon name="play" size={12} stroke="var(--accent)" />
          <span className="text-[13px] font-semibold text-text font-sans">
            Run {script.name}
          </span>
        </div>
        <div className="font-mono text-[9px] text-text-dimmer uppercase tracking-[0.8px] mb-[8px]">
          environment
        </div>
        <div className="flex flex-col gap-[6px] mb-[14px]">
          {vars.map((v, i) => (
            <div key={v.key + i} className="flex items-center gap-[6px]">
              <span className="w-[32%] min-w-0 font-mono text-[11px] text-text-dim uppercase truncate">
                {v.key}
              </span>
              <span className="font-mono text-[11px] text-text-dimmer">=</span>
              <input
                autoFocus={i === 0}
                value={values[i] ?? ""}
                onChange={(e) => {
                  const next = values.slice();
                  next[i] = e.target.value;
                  setValues(next);
                }}
                onKeyDown={onKey}
                placeholder={v.default}
                className="flex-1 min-w-0 bg-bg border border-line rounded-[3px] px-[8px] py-[5px] outline-none text-text font-mono text-[11px]"
              />
            </div>
          ))}
        </div>
        <div className="flex gap-[6px] justify-end">
          <button
            type="button"
            onClick={onCancel}
            className="inline-flex items-center px-[10px] py-[4px] font-mono text-[11px] text-text-dim border border-line rounded-[3px]"
          >
            cancel
          </button>
          <button
            type="button"
            onClick={submit}
            title="Run (⌘/Ctrl + Enter)"
            className="inline-flex items-center gap-[5px] px-[10px] py-[4px] font-mono text-[11px] rounded-[3px] font-semibold"
            style={{
              background: "var(--accent)",
              color: "var(--accent-fg)",
              border: "none",
            }}
          >
            <Icon name="play" size={9} stroke="var(--accent-fg)" />
            continue
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
