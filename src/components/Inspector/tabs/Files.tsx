import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Virtuoso } from "react-virtuoso";
import { Icon } from "../../Icon";
import { TabEmpty, TabError, TabSkeleton } from "../TabStates";
import {
  gitCommit,
  gitPush,
  gitStash,
  listFiles,
  type GitActionResult,
} from "../../../ipc";
import { useUiStore } from "../../../state/store";
import type { FileNode, Project, FileStatus } from "../../../types";

// Atlas - Inspector / Files tab.

interface FilesProps {
  project: Project;
}

export function Files({ project }: FilesProps) {
  const [mode, setMode] = useState<"changed" | "all">("changed");
  const [prompt, setPrompt] = useState<null | "commit" | "stash">(null);
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();

  const { data, isLoading, error, refetch } = useQuery<FileNode[]>({
    queryKey: ["files", project.id, mode],
    queryFn: () => listFiles(project.id, mode === "changed"),
    // Files change frequently; keep the cache short.
    staleTime: 5_000,
    retry: false,
  });

  // After a mutating git action succeeds, drop the files cache + ping the
  const onActionSuccess = (
    kind: "commit" | "stash" | "push",
    result: GitActionResult,
  ) => {
    if (result.ok) {
      pushToast("success", `${labelFor(kind)} done`);
    } else {
      // Non-success (e.g. nothing to commit) - surface stderr tail.
      const msg = (result.stderr || result.stdout).trim().split("\n").pop() ?? "";
      pushToast("warn", `${labelFor(kind)}: ${msg || "failed"}`);
    }
    queryClient.invalidateQueries({ queryKey: ["files", project.id] });
    queryClient.invalidateQueries({ queryKey: ["projects"] });
  };

  const commitMut = useMutation({
    mutationFn: (message: string) => gitCommit(project.id, message),
    onSuccess: (res) => onActionSuccess("commit", res),
    onError: (err) => pushToast("error", `Commit failed: ${String(err)}`),
  });
  const stashMut = useMutation({
    mutationFn: (message: string) => gitStash(project.id, message),
    onSuccess: (res) => onActionSuccess("stash", res),
    onError: (err) => pushToast("error", `Stash failed: ${String(err)}`),
  });
  const pushMut = useMutation({
    mutationFn: () => gitPush(project.id),
    onSuccess: (res) => onActionSuccess("push", res),
    onError: (err) => pushToast("error", `Push failed: ${String(err)}`),
  });

  const tree = data ?? [];

  // Pre-compute summary counts for the M/+/- pill row.
  const counts = useMemo(() => {
    let M = 0,
      added = 0,
      deleted = 0,
      changed = 0;
    for (const n of tree) {
      if (n.kind !== "file") continue;
      if (n.status === "M") {
        M += 1;
        changed += 1;
      } else if (n.status === "+") {
        added += 1;
        changed += 1;
      } else if (n.status === "-") {
        deleted += 1;
        changed += 1;
      }
    }
    return { M, added, deleted, changed };
  }, [tree]);

  return (
    <div className="flex flex-col h-full">
      <div className="px-[14px] pt-[14px] pb-[10px] flex items-center gap-2 shrink-0">
        <ModePills mode={mode} onChange={setMode} />
        <div className="flex-1" />
        <span className="font-mono text-[10px] text-text-dim flex gap-[6px]">
          <span className="text-warn">{counts.M}M</span>
          <span className="text-accent">+{counts.added}</span>
          <span className="text-danger">−{counts.deleted}</span>
        </span>
      </div>

      <div className="flex-1 min-h-0 overflow-hidden">
        {isLoading && !data && <TabSkeleton rows={5} />}
        {error && (
          <TabError
            message={
              error instanceof Error ? error.message : String(error)
            }
            onRetry={() => void refetch()}
          />
        )}
        {!isLoading && !error && tree.length === 0 && (
          <TabEmpty
            icon="file"
            title={mode === "changed" ? "No changes" : "Empty tree"}
            hint={
              mode === "changed"
                ? "Working tree is clean"
                : "No files tracked yet"
            }
          />
        )}
        {tree.length > 0 && (
          <div className="px-[14px] h-full">
            <Virtuoso
              data={tree}
              className="h-full"
              computeItemKey={(_, n) => `${n.depth}:${n.path}`}
              itemContent={(_, n) => <FileRow node={n} />}
            />
          </div>
        )}
      </div>

      <div className="px-[14px] py-[10px] border-t border-line shrink-0 flex items-center gap-2 relative">
        <Icon name="git" size={11} stroke="var(--text-dim)" />
        <span className="font-mono text-[11px] text-text-dim flex-1">
          {counts.changed} files changed
          {project.ahead > 0 ? ` · ↑${project.ahead}` : ""}
          {project.behind > 0 ? ` ↓${project.behind}` : ""}
        </span>
        <button
          type="button"
          onClick={() => setPrompt("stash")}
          disabled={counts.changed === 0 || stashMut.isPending}
          title="Stash all changes"
          className="px-[8px] py-[3px] font-mono text-[10px] text-text border border-line rounded-[3px] hover:bg-row-active disabled:opacity-40 disabled:cursor-not-allowed"
        >
          stash
        </button>
        <button
          type="button"
          onClick={() => setPrompt("commit")}
          disabled={counts.changed === 0 || commitMut.isPending}
          title="Stage all + commit"
          className="px-[8px] py-[3px] font-mono text-[10px] bg-accent text-accent-fg rounded-[3px] font-semibold hover:opacity-90 disabled:opacity-40 disabled:cursor-not-allowed"
        >
          commit
        </button>
        <button
          type="button"
          onClick={() => pushMut.mutate()}
          disabled={pushMut.isPending}
          title={
            project.ahead > 0
              ? `Push ${project.ahead} commit${project.ahead === 1 ? "" : "s"} to remote`
              : "Push to remote"
          }
          aria-label="Push to remote"
          className="inline-flex items-center justify-center w-[24px] h-[22px] rounded-[3px] border border-line text-text hover:bg-row-active disabled:opacity-40 disabled:cursor-not-allowed"
        >
          <Icon name="arrow-up" size={12} />
        </button>

        {prompt === "commit" && (
          <MessagePrompt
            label="Commit"
            placeholder="Commit message…"
            busy={commitMut.isPending}
            onCancel={() => setPrompt(null)}
            onConfirm={(msg) => {
              commitMut.mutate(msg);
              setPrompt(null);
            }}
          />
        )}
        {prompt === "stash" && (
          <MessagePrompt
            label="Stash"
            placeholder="Stash message (optional)…"
            busy={stashMut.isPending}
            optional
            onCancel={() => setPrompt(null)}
            onConfirm={(msg) => {
              stashMut.mutate(msg);
              setPrompt(null);
            }}
          />
        )}
      </div>
    </div>
  );
}

function labelFor(kind: "commit" | "stash" | "push"): string {
  if (kind === "commit") return "Commit";
  if (kind === "stash") return "Stash";
  return "Push";
}

// Inline message prompt that floats above the Files footer. Enter confirms,
function MessagePrompt({
  label,
  placeholder,
  optional,
  busy,
  onCancel,
  onConfirm,
}: {
  label: string;
  placeholder: string;
  optional?: boolean;
  busy?: boolean;
  onCancel: () => void;
  onConfirm: (value: string) => void;
}) {
  const [value, setValue] = useState("");
  const inputRef = useRef<HTMLInputElement | null>(null);
  const panelRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onCancel();
      }
    };
    const onClick = (e: MouseEvent) => {
      if (panelRef.current && !panelRef.current.contains(e.target as Node)) {
        onCancel();
      }
    };
    const id = window.setTimeout(() => {
      window.addEventListener("mousedown", onClick);
      window.addEventListener("keydown", onKey);
    }, 0);
    return () => {
      window.clearTimeout(id);
      window.removeEventListener("mousedown", onClick);
      window.removeEventListener("keydown", onKey);
    };
  }, [onCancel]);

  const submit = () => {
    const trimmed = value.trim();
    if (!optional && !trimmed) return;
    onConfirm(trimmed);
  };

  return (
    <div
      ref={panelRef}
      onClick={(e) => e.stopPropagation()}
      style={{
        position: "absolute",
        bottom: "calc(100% + 6px)",
        right: 14,
        left: 14,
        padding: 8,
        background: "var(--palette-bg)",
        border: "1px solid var(--line)",
        borderRadius: 6,
        boxShadow: "0 20px 50px rgba(0,0,0,0.5)",
        backdropFilter: "blur(20px) saturate(180%)",
        WebkitBackdropFilter: "blur(20px) saturate(180%)",
        display: "flex",
        flexDirection: "column",
        gap: 8,
        zIndex: 20,
      }}
    >
      <div
        style={{
          fontSize: 11,
          fontFamily: "var(--mono)",
          color: "var(--text-dim)",
          textTransform: "uppercase",
          letterSpacing: 0.6,
        }}
      >
        {label}
      </div>
      <input
        ref={inputRef}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder={placeholder}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            submit();
          }
        }}
        disabled={busy}
        style={{
          padding: "6px 8px",
          fontSize: 12,
          background: "var(--bg)",
          border: "1px solid var(--line)",
          borderRadius: 4,
          color: "var(--text)",
          outline: "none",
          fontFamily: "var(--mono)",
        }}
      />
      <div style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
        <button
          type="button"
          onClick={onCancel}
          style={{
            padding: "4px 10px",
            fontSize: 12,
            borderRadius: 4,
            border: "1px solid var(--line)",
            background: "var(--surface-2)",
            color: "var(--text-dim)",
            cursor: "pointer",
          }}
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={submit}
          disabled={busy || (!optional && !value.trim())}
          style={{
            padding: "4px 12px",
            fontSize: 12,
            borderRadius: 4,
            border: "1px solid var(--accent)",
            background: "var(--accent)",
            color: "var(--accent-fg)",
            cursor: "pointer",
            fontWeight: 600,
            opacity: busy || (!optional && !value.trim()) ? 0.5 : 1,
          }}
        >
          {label}
        </button>
      </div>
    </div>
  );
}

function ModePills({
  mode,
  onChange,
}: {
  mode: "changed" | "all";
  onChange: (m: "changed" | "all") => void;
}) {
  const opts: ("changed" | "all")[] = ["changed", "all"];
  return (
    <div className="flex border border-line rounded-[4px] overflow-hidden">
      {opts.map((m, i) => {
        const active = mode === m;
        return (
          <button
            key={m}
            type="button"
            onClick={() => onChange(m)}
            className="px-[9px] py-[3px] font-mono text-[10px] uppercase tracking-[0.5px]"
            style={{
              background: active ? "var(--surface-2)" : "transparent",
              color: active ? "var(--text)" : "var(--text-dim)",
              borderRight: i === 0 ? "1px solid var(--line)" : undefined,
            }}
          >
            {m}
          </button>
        );
      })}
    </div>
  );
}

function FileRow({ node }: { node: FileNode }) {
  const isFile = node.kind === "file";
  const color = statusColor(node.status ?? null);
  return (
    <div
      className="flex items-center gap-[6px] py-[3px] rounded-[3px] hover:bg-row-active font-mono text-[11px]"
      style={{
        paddingLeft: node.depth * 12 + 4,
        paddingRight: 4,
        color: isFile ? color : "var(--text-dim)",
        cursor: isFile ? "pointer" : "default",
      }}
    >
      <Icon
        name={isFile ? "file" : "chevron"}
        size={10}
        stroke="currentColor"
      />
      <span
        className="flex-1 truncate"
        style={{ color: node.status ? "currentColor" : "var(--text)" }}
        title={node.path}
      >
        {node.name}
      </span>
      {node.delta && (
        <span className="text-[10px] text-text-dimmer">{node.delta}</span>
      )}
      {node.status && <StatusBadge status={node.status} />}
    </div>
  );
}

function StatusBadge({ status }: { status: FileStatus }) {
  if (!status) return null;
  const bg =
    status === "M"
      ? "oklch(0.78 0.15 80 / 0.18)"
      : status === "+"
        ? "oklch(0.78 0.17 145 / 0.18)"
        : "oklch(0.66 0.19 25 / 0.18)";
  const color = statusColor(status);
  const label = status === "-" ? "−" : status;
  return (
    <span
      className="inline-flex items-center justify-center text-[9px] font-semibold"
      style={{
        width: 14,
        height: 14,
        borderRadius: 2,
        background: bg,
        color,
      }}
    >
      {label}
    </span>
  );
}

function statusColor(status: FileStatus): string {
  if (status === "M") return "var(--warn)";
  if (status === "+") return "var(--accent)";
  if (status === "-") return "var(--danger)";
  return "var(--text-dim)";
}
