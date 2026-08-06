import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Icon } from "../../Icon";
import { TabEmpty, TabError, TabSkeleton } from "../TabStates";
import {
  localExcludesGet,
  localExcludesSet,
  type LocalExcludes,
} from "../../../ipc";
import { useUiStore } from "../../../state/store";
import type { Project } from "../../../types";

// Atlas - Inspector / Ignores tab. Manages the Atlas-owned block inside
// `.git/info/exclude` - per-repo ignore rules git never commits or pushes,
// so `.atlas/` and AI scratch files stay out of git without touching the
// project's `.gitignore`.

const ATLAS_PATTERN = ".atlas/";

interface IgnoresProps {
  project: Project;
}

export function Ignores({ project }: IgnoresProps) {
  const pushToast = useUiStore((s) => s.pushToast);
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState("");

  const { data, isLoading, error, refetch } = useQuery<LocalExcludes>({
    queryKey: ["local-excludes", project.id],
    queryFn: () => localExcludesGet(project.id),
    staleTime: 5_000,
    retry: false,
  });

  const setMut = useMutation({
    mutationFn: (patterns: string[]) => localExcludesSet(project.id, patterns),
    onSuccess: (next) => {
      queryClient.setQueryData(["local-excludes", project.id], next);
      // Ignore rules change what counts as dirty - refresh git-derived views.
      queryClient.invalidateQueries({ queryKey: ["files", project.id] });
      queryClient.invalidateQueries({ queryKey: ["projects"] });
    },
    onError: (err) =>
      pushToast("error", `Update ignores failed: ${String(err)}`),
  });

  if (isLoading && !data) return <TabSkeleton rows={4} />;
  if (error) {
    return (
      <TabError
        message={error instanceof Error ? error.message : String(error)}
        onRetry={() => void refetch()}
      />
    );
  }
  if (!data) return null;
  if (!data.isGitRepo) {
    return (
      <TabEmpty
        icon="git"
        title="Not a git repository"
        hint="Local ignores live in .git/info/exclude"
      />
    );
  }

  const patterns = data.patterns;
  const atlasInBlock = patterns.includes(ATLAS_PATTERN);
  const userPatterns = patterns.filter((p) => p !== ATLAS_PATTERN);
  const busy = setMut.isPending;

  const addPattern = (raw: string) => {
    const trimmed = raw.trim();
    if (!trimmed || patterns.includes(trimmed)) return;
    setMut.mutate([...patterns, trimmed]);
    setDraft("");
  };
  const removePattern = (p: string) =>
    setMut.mutate(patterns.filter((x) => x !== p));
  const toggleAtlas = () => {
    if (atlasInBlock) removePattern(ATLAS_PATTERN);
    else setMut.mutate([ATLAS_PATTERN, ...patterns]);
  };

  return (
    <div className="flex flex-col h-full overflow-y-auto">
      <div className="px-[14px] pt-[14px] pb-[10px] text-[11px] text-text-dim leading-relaxed">
        Rules here are written to{" "}
        <span className="font-mono text-text">.git/info/exclude</span> — git
        ignores the matches locally, but nothing is added to{" "}
        <span className="font-mono">.gitignore</span> and nothing is ever
        committed or shared.
      </div>

      {/* .atlas/ toggle */}
      <div className="px-[14px] py-[8px]">
        <button
          type="button"
          onClick={toggleAtlas}
          disabled={busy}
          className="w-full flex items-center gap-[8px] px-[10px] py-[8px] border border-line rounded-[5px] hover:bg-row-active disabled:opacity-50 text-left"
        >
          <Icon
            name={atlasInBlock ? "square-check" : "square"}
            size={14}
            stroke={atlasInBlock ? "var(--accent)" : "var(--text-dim)"}
          />
          <span className="flex-1 min-w-0">
            <span className="block text-[12px] text-text">
              Keep <span className="font-mono">.atlas/</span> out of git
            </span>
            <span className="block text-[10px] text-text-dim">
              Atlas notes, todos and pilot data stay local to this machine
            </span>
          </span>
          {data.atlasIgnored && !atlasInBlock && (
            <span
              className="font-mono text-[9px] text-text-dimmer shrink-0"
              title="Another rule (e.g. .gitignore or global excludes) already ignores .atlas/"
            >
              already ignored
            </span>
          )}
        </button>
      </div>

      {/* De-index warning: excludes only affect untracked files. */}
      {data.atlasTracked && (
        <div
          className="mx-[14px] my-[4px] p-[10px] rounded-[5px] text-[11px] leading-relaxed"
          style={{
            border: "1px solid var(--warn)",
            background: "oklch(0.78 0.15 80 / 0.10)",
            color: "var(--warn)",
          }}
        >
          <span className="font-mono">.atlas/</span> is already committed in
          this repo, so ignore rules won&apos;t hide it. Untrack it once with{" "}
          <span className="font-mono">git rm -r --cached .atlas</span> (keeps
          the files on disk), then commit.
        </div>
      )}

      {/* Extra local patterns - AI scratch files, diagrams, etc. */}
      <div className="px-[14px] pt-[10px] pb-[4px] font-mono text-[10px] uppercase tracking-[0.6px] text-text-dim">
        Local patterns
      </div>
      <div className="px-[14px] flex flex-col gap-[2px]">
        {userPatterns.length === 0 && (
          <div className="py-[6px] text-[11px] text-text-dimmer">
            No extra patterns. Add globs for AI scratch files, e.g.{" "}
            <span className="font-mono">*.excalidraw</span> or{" "}
            <span className="font-mono">scratch-*.md</span>.
          </div>
        )}
        {userPatterns.map((p) => (
          <div
            key={p}
            className="group flex items-center gap-[6px] px-[8px] py-[4px] rounded-[3px] hover:bg-row-active font-mono text-[11px] text-text"
          >
            <Icon name="git" size={10} stroke="var(--text-dim)" />
            <span className="flex-1 truncate" title={p}>
              {p}
            </span>
            <button
              type="button"
              onClick={() => removePattern(p)}
              disabled={busy}
              title={`Remove ${p}`}
              aria-label={`Remove ${p}`}
              className="opacity-0 group-hover:opacity-100 inline-flex items-center justify-center w-[18px] h-[18px] rounded-[3px] text-text-dim hover:text-danger disabled:opacity-30"
            >
              <Icon name="trash" size={11} stroke="currentColor" />
            </button>
          </div>
        ))}
      </div>

      {/* Add pattern */}
      <div className="px-[14px] py-[10px] flex items-center gap-[6px]">
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addPattern(draft);
            }
          }}
          placeholder="Add pattern (gitignore syntax)…"
          disabled={busy}
          className="flex-1 min-w-0 px-[8px] py-[5px] font-mono text-[11px] text-text rounded-[4px] outline-none"
          style={{
            background: "var(--bg)",
            border: "1px solid var(--line)",
          }}
        />
        <button
          type="button"
          onClick={() => addPattern(draft)}
          disabled={busy || !draft.trim()}
          title="Add pattern"
          aria-label="Add pattern"
          className="inline-flex items-center justify-center w-[26px] h-[26px] rounded-[4px] border border-line text-text-dim hover:text-text hover:bg-row-active disabled:opacity-40 disabled:cursor-not-allowed"
        >
          <Icon name="plus" size={12} />
        </button>
      </div>
    </div>
  );
}
