import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Virtuoso } from "react-virtuoso";
import ReactDiffViewer, { DiffMethod } from "react-diff-viewer-continued";
import Prism from "prismjs";
// Languages we want to highlight in the diff modal. Each side-effect import
// registers a grammar with the global `Prism` instance.
import "prismjs/components/prism-markup";
import "prismjs/components/prism-clike";
import "prismjs/components/prism-javascript";
import "prismjs/components/prism-typescript";
import "prismjs/components/prism-jsx";
import "prismjs/components/prism-tsx";
import "prismjs/components/prism-json";
import "prismjs/components/prism-css";
import "prismjs/components/prism-bash";
import "prismjs/components/prism-rust";
import "prismjs/components/prism-go";
import "prismjs/components/prism-python";
import "prismjs/components/prism-markdown";
import "prismjs/components/prism-yaml";
import "prismjs/components/prism-toml";
import "prismjs/components/prism-sql";
// IntelliJ Darcula theme for the highlighted tokens.
import "prism-themes/themes/prism-darcula.css";
import { Icon } from "../../Icon";
import { TabEmpty, TabError, TabSkeleton } from "../TabStates";
import {
  filesDiff,
  gitCommit,
  gitPush,
  gitStash,
  listFiles,
  type FileDiff,
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
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const [diffPath, setDiffPath] = useState<string | null>(null);
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

  // Filter out descendants of any collapsed directory. The flat list is
  // sorted by path so the comparison stays linear.
  const visibleTree = useMemo(() => {
    if (collapsed.size === 0) return tree;
    const out: FileNode[] = [];
    let hideUnder: string | null = null;
    for (const node of tree) {
      if (hideUnder !== null) {
        if (node.path.startsWith(hideUnder + "/")) continue;
        hideUnder = null;
      }
      out.push(node);
      if (node.kind === "dir" && collapsed.has(node.path)) {
        hideUnder = node.path;
      }
    }
    return out;
  }, [tree, collapsed]);

  const toggleDir = (path: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  // Ordered list of paths the diff modal can step through with the
  // prev/next arrows. Mirrors the visible tree order so navigation feels
  // tied to what the user just saw in the list.
  const changedPaths = useMemo(
    () =>
      tree
        .filter((n) => n.kind === "file" && !!n.status)
        .map((n) => n.path),
    [tree],
  );

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
              data={visibleTree}
              className="h-full"
              computeItemKey={(_, n) => `${n.depth}:${n.path}`}
              itemContent={(_, n) => (
                <FileRow
                  node={n}
                  isCollapsed={n.kind === "dir" && collapsed.has(n.path)}
                  onToggle={() => toggleDir(n.path)}
                  onOpenDiff={() => setDiffPath(n.path)}
                />
              )}
            />
          </div>
        )}
      </div>

      {diffPath && (
        <DiffModal
          projectId={project.id}
          path={diffPath}
          paths={changedPaths}
          onNavigate={setDiffPath}
          onClose={() => setDiffPath(null)}
        />
      )}

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

function FileRow({
  node,
  isCollapsed,
  onToggle,
  onOpenDiff,
}: {
  node: FileNode;
  isCollapsed: boolean;
  onToggle: () => void;
  onOpenDiff: () => void;
}) {
  const isFile = node.kind === "file";
  const color = statusColor(node.status ?? null);
  // Files with a git status are clickable and open the diff modal.
  const isClickable = isFile && !!node.status;
  return (
    <div
      role={isClickable || !isFile ? "button" : undefined}
      tabIndex={isClickable || !isFile ? 0 : -1}
      onClick={() => {
        if (!isFile) onToggle();
        else if (isClickable) onOpenDiff();
      }}
      onKeyDown={(e) => {
        if (e.key !== "Enter" && e.key !== " ") return;
        e.preventDefault();
        if (!isFile) onToggle();
        else if (isClickable) onOpenDiff();
      }}
      className="flex items-center gap-[6px] py-[3px] rounded-[3px] hover:bg-row-active font-mono text-[11px] focus:outline-none focus:bg-row-active"
      style={{
        paddingLeft: node.depth * 12 + 4,
        paddingRight: 4,
        color: isFile ? color : "var(--text-dim)",
        cursor: isClickable || !isFile ? "pointer" : "default",
      }}
    >
      {isFile ? (
        <Icon name="file" size={10} stroke="currentColor" />
      ) : (
        <Icon
          name="chevron"
          size={10}
          stroke="currentColor"
          style={{
            transform: isCollapsed ? "rotate(0deg)" : "rotate(90deg)",
            transition: "transform 120ms ease",
          }}
        />
      )}
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

// Modal for the per-file diff. The diff itself is rendered by
// `react-diff-viewer-continued`; we wrap it with a header that lets the
// user toggle between split-view and inline-view.
function DiffModal({
  projectId,
  path,
  paths,
  onNavigate,
  onClose,
}: {
  projectId: string;
  path: string;
  paths: string[];
  onNavigate: (path: string) => void;
  onClose: () => void;
}) {
  const [splitView, setSplitView] = useState(true);
  // Resolve the active theme so we can flip the diff viewer between its
  // dark and light variable sets - the dark line-number color washes out
  // against the light addition / deletion gutter backgrounds.
  const themePref = useUiStore((s) => s.theme);
  const isDark = useResolvedDark(themePref);
  const { data, isLoading, error } = useQuery<FileDiff>({
    queryKey: ["files-diff", projectId, path],
    queryFn: () => filesDiff(projectId, path),
    staleTime: 5_000,
    retry: false,
  });

  // Where the current file sits in the list of changed paths. -1 when the
  // current path was just resolved (e.g. after committing) and is no longer
  // in the changed list - we still show it but disable the arrows.
  const idx = paths.indexOf(path);
  const hasPrev = idx > 0;
  const hasNext = idx >= 0 && idx < paths.length - 1;
  const goPrev = () => {
    if (hasPrev) onNavigate(paths[idx - 1]);
  };
  const goNext = () => {
    if (hasNext) onNavigate(paths[idx + 1]);
  };

  // ---- in-file hunk navigation -------------------------------------------
  // After react-diff-viewer renders, walk the DOM to find rows that contain
  // an added or removed line and group adjacent ones into hunks. We then
  // scroll the chosen hunk's first row into view when the up/down arrows
  // are clicked. The data-diff-type attribute is wired in via `renderGutter`.
  const scrollAreaRef = useRef<HTMLDivElement | null>(null);
  const [hunkRows, setHunkRows] = useState<HTMLElement[]>([]);
  const [currentHunk, setCurrentHunk] = useState(0);

  useEffect(() => {
    if (!data || data.isBinary) {
      setHunkRows([]);
      setCurrentHunk(0);
      return;
    }
    let cancelled = false;
    let attempts = 0;
    const collect = () => {
      if (cancelled) return;
      const root = scrollAreaRef.current;
      if (!root) return;
      const allRows = Array.from(root.querySelectorAll<HTMLElement>("tr"));
      // react-diff-viewer renders a "marker" cell per line whose only
      // visible glyph is `+`, `-`, or empty. Treat any row that contains
      // such a marker cell as part of a hunk.
      const isChangeRow = (row: HTMLElement) => {
        const tds = row.querySelectorAll(":scope > td");
        for (const td of tds) {
          const t = (td.textContent ?? "").trim();
          if (t === "+" || t === "-") return true;
        }
        return false;
      };
      // Diff computation runs in a web worker; rows may not exist yet.
      if (allRows.length === 0 && attempts < 30) {
        attempts += 1;
        requestAnimationFrame(collect);
        return;
      }
      const hunks: HTMLElement[] = [];
      let inHunk = false;
      for (const row of allRows) {
        if (isChangeRow(row)) {
          if (!inHunk) hunks.push(row);
          inHunk = true;
        } else {
          inHunk = false;
        }
      }
      // If we got rows back but still no hunks (e.g. content not painted
      // yet), retry briefly.
      if (hunks.length === 0 && attempts < 30) {
        attempts += 1;
        requestAnimationFrame(collect);
        return;
      }
      setHunkRows(hunks);
      setCurrentHunk(0);
    };
    requestAnimationFrame(collect);
    return () => {
      cancelled = true;
    };
  }, [data, splitView]);

  const scrollToHunk = (i: number) => {
    const row = hunkRows[i];
    if (!row) return;
    row.scrollIntoView({ behavior: "smooth", block: "center" });
    setCurrentHunk(i);
  };
  const hasPrevHunk = hunkRows.length > 0 && currentHunk > 0;
  const hasNextHunk =
    hunkRows.length > 0 && currentHunk < hunkRows.length - 1;
  const goPrevHunk = () => {
    if (hasPrevHunk) scrollToHunk(currentHunk - 1);
  };
  const goNextHunk = () => {
    if (hasNextHunk) scrollToHunk(currentHunk + 1);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
        return;
      }
      // Alt+←/→ steps between changed files; Alt+↑/↓ jumps between hunks
      // inside the current file. Plain arrow keys stay free for scrolling.
      if (e.altKey && e.key === "ArrowLeft") {
        e.preventDefault();
        goPrev();
      } else if (e.altKey && e.key === "ArrowRight") {
        e.preventDefault();
        goNext();
      } else if (e.altKey && e.key === "ArrowUp") {
        e.preventDefault();
        goPrevHunk();
      } else if (e.altKey && e.key === "ArrowDown") {
        e.preventDefault();
        goNextHunk();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    onClose,
    hasPrev,
    hasNext,
    idx,
    paths,
    hasPrevHunk,
    hasNextHunk,
    currentHunk,
    hunkRows,
  ]);

  return (
    <div
      onClick={onClose}
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.55)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 100,
        padding: 32,
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: "min(1200px, 96vw)",
          height: "min(820px, 92vh)",
          background: "var(--surface)",
          border: "1px solid var(--line)",
          borderRadius: 8,
          boxShadow: "0 30px 80px rgba(0,0,0,0.6)",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            padding: "10px 14px",
            borderBottom: "1px solid var(--line)",
            background: "var(--surface-2)",
            flexShrink: 0,
          }}
        >
          <button
            type="button"
            onClick={goPrev}
            disabled={!hasPrev}
            aria-label="Previous change"
            title="Previous change (Alt+←)"
            className="inline-flex items-center justify-center w-[22px] h-[22px] rounded-[3px] border border-line text-text-dim hover:bg-row-active hover:text-text disabled:opacity-30 disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-text-dim"
          >
            <Icon name="chevron" size={11} style={{ transform: "rotate(180deg)" }} />
          </button>
          <button
            type="button"
            onClick={goNext}
            disabled={!hasNext}
            aria-label="Next change"
            title="Next change (Alt+→)"
            className="inline-flex items-center justify-center w-[22px] h-[22px] rounded-[3px] border border-line text-text-dim hover:bg-row-active hover:text-text disabled:opacity-30 disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-text-dim"
          >
            <Icon name="chevron" size={11} />
          </button>
          {idx >= 0 && paths.length > 1 && (
            <span className="font-mono text-[10px] text-text-dim tabular-nums">
              {idx + 1}/{paths.length}
            </span>
          )}
          <span style={{ width: 1, alignSelf: "stretch", background: "var(--line)", margin: "0 2px" }} />
          <button
            type="button"
            onClick={goPrevHunk}
            disabled={!hasPrevHunk}
            aria-label="Previous change in file"
            title="Previous change in file (Alt+↑)"
            className="inline-flex items-center justify-center w-[22px] h-[22px] rounded-[3px] border border-line text-text-dim hover:bg-row-active hover:text-text disabled:opacity-30 disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-text-dim"
          >
            <Icon name="arrow-up" size={11} />
          </button>
          <button
            type="button"
            onClick={goNextHunk}
            disabled={!hasNextHunk}
            aria-label="Next change in file"
            title="Next change in file (Alt+↓)"
            className="inline-flex items-center justify-center w-[22px] h-[22px] rounded-[3px] border border-line text-text-dim hover:bg-row-active hover:text-text disabled:opacity-30 disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-text-dim"
          >
            <Icon name="arrow-down" size={11} />
          </button>
          {hunkRows.length > 0 && (
            <span className="font-mono text-[10px] text-text-dim tabular-nums">
              {currentHunk + 1}/{hunkRows.length}
            </span>
          )}
          <Icon name="file" size={12} stroke="var(--text-dim)" />
          <span
            className="font-mono text-[12px] truncate"
            style={{ color: "var(--text)", flex: 1 }}
            title={path}
          >
            {path}
          </span>
          {data?.status && <StatusBadge status={data.status as FileStatus} />}
          <ViewModePills splitView={splitView} onChange={setSplitView} />
          <button
            type="button"
            onClick={onClose}
            aria-label="Close diff"
            className="inline-flex items-center justify-center w-[22px] h-[22px] rounded-[3px] border border-line text-text-dim hover:bg-row-active hover:text-text"
            style={{ fontFamily: "var(--mono)", fontSize: 14, lineHeight: 1 }}
          >
            ×
          </button>
        </div>

        <div ref={scrollAreaRef} style={{ flex: 1, minHeight: 0, overflow: "auto" }}>
          {isLoading && (
            <div style={{ padding: 16 }}>
              <TabSkeleton rows={6} />
            </div>
          )}
          {error && (
            <div style={{ padding: 16 }}>
              <TabError
                message={
                  error instanceof Error ? error.message : String(error)
                }
              />
            </div>
          )}
          {data && data.isBinary && (
            <div style={{ padding: 24 }}>
              <TabEmpty
                icon="file"
                title="Binary file"
                hint="Diff is not shown for binary content."
              />
            </div>
          )}
          {data && !data.isBinary && (
            <ReactDiffViewer
              oldValue={data.oldContent}
              newValue={data.newContent}
              splitView={splitView}
              compareMethod={DiffMethod.WORDS}
              useDarkTheme={isDark}
              showDiffOnly={false}
              hideSummary
              leftTitle={data.status === "+" ? "(new file)" : "HEAD"}
              rightTitle={data.status === "-" ? "(deleted)" : "Working tree"}
              renderContent={(source) => (
                <span
                  className="diff-syntax"
                  dangerouslySetInnerHTML={{
                    __html: highlightSource(source, prismLangFor(path)),
                  }}
                />
              )}
              styles={{
                variables: {
                  dark: {
                    diffViewerBackground: "var(--surface)",
                    diffViewerColor: "var(--text)",
                    gutterBackground: "var(--surface-2)",
                    gutterColor: "var(--text-dimmer)",
                    addedBackground: "rgba(46, 160, 67, 0.18)",
                    addedColor: "inherit",
                    removedBackground: "rgba(248, 81, 73, 0.18)",
                    removedColor: "inherit",
                    wordAddedBackground: "rgba(46, 160, 67, 0.45)",
                    wordRemovedBackground: "rgba(248, 81, 73, 0.45)",
                    addedGutterBackground: "rgba(46, 160, 67, 0.28)",
                    removedGutterBackground: "rgba(248, 81, 73, 0.28)",
                    addedGutterColor: "rgba(220, 235, 220, 0.85)",
                    removedGutterColor: "rgba(245, 220, 220, 0.85)",
                    codeFoldGutterBackground: "var(--surface-2)",
                    codeFoldBackground: "var(--surface-2)",
                    emptyLineBackground: "var(--surface)",
                  },
                  light: {
                    diffViewerBackground: "var(--surface)",
                    diffViewerColor: "var(--text)",
                    gutterBackground: "var(--surface-2)",
                    gutterColor: "var(--text-dim)",
                    addedBackground: "rgba(40, 167, 69, 0.16)",
                    addedColor: "inherit",
                    removedBackground: "rgba(220, 53, 69, 0.16)",
                    removedColor: "inherit",
                    wordAddedBackground: "rgba(40, 167, 69, 0.38)",
                    wordRemovedBackground: "rgba(220, 53, 69, 0.38)",
                    addedGutterBackground: "rgba(40, 167, 69, 0.28)",
                    removedGutterBackground: "rgba(220, 53, 69, 0.28)",
                    addedGutterColor: "rgba(20, 90, 40, 0.85)",
                    removedGutterColor: "rgba(120, 30, 40, 0.85)",
                    codeFoldGutterBackground: "var(--surface-2)",
                    codeFoldBackground: "var(--surface-2)",
                    emptyLineBackground: "var(--surface)",
                  },
                },
              }}
            />
          )}
        </div>
      </div>
    </div>
  );
}

function ViewModePills({
  splitView,
  onChange,
}: {
  splitView: boolean;
  onChange: (split: boolean) => void;
}) {
  const opts: { label: string; value: boolean }[] = [
    { label: "split", value: true },
    { label: "inline", value: false },
  ];
  return (
    <div className="flex border border-line rounded-[4px] overflow-hidden">
      {opts.map((o, i) => {
        const active = o.value === splitView;
        return (
          <button
            key={o.label}
            type="button"
            onClick={() => onChange(o.value)}
            className="px-[9px] py-[3px] font-mono text-[10px] uppercase tracking-[0.5px]"
            style={{
              background: active ? "var(--surface)" : "transparent",
              color: active ? "var(--text)" : "var(--text-dim)",
              borderRight: i === 0 ? "1px solid var(--line)" : undefined,
            }}
          >
            {o.label}
          </button>
        );
      })}
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

// Map a path's extension (or basename) to a Prism language id, falling back
// to plain text when nothing matches. The diff viewer calls `renderContent`
// per line/segment so the lookup happens once per modal open.
const EXT_LANG: Record<string, string> = {
  ts: "typescript",
  tsx: "tsx",
  js: "javascript",
  jsx: "jsx",
  mjs: "javascript",
  cjs: "javascript",
  json: "json",
  json5: "json",
  jsonc: "json",
  css: "css",
  scss: "css",
  sass: "css",
  less: "css",
  html: "markup",
  htm: "markup",
  xml: "markup",
  svg: "markup",
  vue: "markup",
  rs: "rust",
  go: "go",
  py: "python",
  md: "markdown",
  mdx: "markdown",
  yml: "yaml",
  yaml: "yaml",
  toml: "toml",
  sh: "bash",
  bash: "bash",
  zsh: "bash",
  sql: "sql",
};

const FILENAME_LANG: Record<string, string> = {
  Dockerfile: "bash",
  Makefile: "bash",
  ".bashrc": "bash",
  ".zshrc": "bash",
};

function prismLangFor(path: string): string | null {
  const base = path.split("/").pop() ?? path;
  if (FILENAME_LANG[base]) return FILENAME_LANG[base];
  const dot = base.lastIndexOf(".");
  if (dot < 0) return null;
  const ext = base.slice(dot + 1).toLowerCase();
  return EXT_LANG[ext] ?? null;
}

function highlightSource(source: string, lang: string | null): string {
  if (!lang) return escapeHtml(source);
  const grammar = Prism.languages[lang];
  if (!grammar) return escapeHtml(source);
  try {
    return Prism.highlight(source, grammar, lang);
  } catch {
    return escapeHtml(source);
  }
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

// Resolve the user's theme pref to a concrete dark/light boolean. Mirrors
// the mapping in App.tsx where `system` consults the OS preference.
function useResolvedDark(pref: "dark" | "light" | "system"): boolean {
  const get = () => {
    if (pref === "dark") return true;
    if (pref === "light") return false;
    return window.matchMedia("(prefers-color-scheme: dark)").matches;
  };
  const [isDark, setIsDark] = useState<boolean>(get);
  useEffect(() => {
    setIsDark(get());
    if (pref !== "system") return;
    const mql = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => setIsDark(mql.matches);
    mql.addEventListener("change", onChange);
    return () => mql.removeEventListener("change", onChange);
  }, [pref]);
  return isDark;
}

